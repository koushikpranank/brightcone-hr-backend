use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{Integer, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::RunQueryDsl;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    controllers::people::ensure_reference,
    database::Database,
    people::policy::require_people_manage,
    request_meta::RequestMeta,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct DepartmentInput {
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GradeInput {
    pub(crate) code: String,
    pub(crate) label: String,
    pub(crate) rank: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LocationInput {
    pub(crate) code: String,
    pub(crate) label: String,
    pub(crate) country_code: String,
    pub(crate) region: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct ReferenceRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    code: String,
    #[diesel(sql_type = Text)]
    label: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    parent_id: Option<Uuid>,
}

async fn list(auth: AuthUser, ctx: AppContext, table: &'static str) -> Result<Response> {
    require_people_manage(&auth)?;
    let query = match table {
        "people_departments" => "SELECT id,code,name AS label,parent_id FROM people_departments WHERE archived_at IS NULL ORDER BY lower(name)",
        "people_grades" => "SELECT id,code,label,NULL::uuid AS parent_id FROM people_grades WHERE archived_at IS NULL ORDER BY rank",
        _ => "SELECT id,code,label,NULL::uuid AS parent_id FROM people_locations WHERE archived_at IS NULL ORDER BY lower(label)",
    };
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let rows = sql_query(query)
        .load::<ReferenceRow>(&mut *conn)
        .await
        .map_err(Database::map_error)?;
    format::json(rows)
}

#[utoipa::path(get, path = "/api/people/departments", security(("bearer" = [])), responses((status = 200, body = [ReferenceRow])))]
#[debug_handler]
pub async fn departments(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    list(auth, ctx, "people_departments").await
}

#[utoipa::path(get, path = "/api/people/grades", security(("bearer" = [])), responses((status = 200, body = [ReferenceRow])))]
#[debug_handler]
pub async fn grades(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    list(auth, ctx, "people_grades").await
}

#[utoipa::path(get, path = "/api/people/locations", security(("bearer" = [])), responses((status = 200, body = [ReferenceRow])))]
#[debug_handler]
pub async fn locations(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    list(auth, ctx, "people_locations").await
}

#[utoipa::path(post, path = "/api/people/departments", security(("bearer" = [])), request_body = DepartmentInput, responses((status = 201, body = ReferenceRow)))]
#[debug_handler]
pub async fn create_department(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Json(input): Json<DepartmentInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    if input.code.trim().is_empty()
        || input.code.len() > 32
        || input.name.trim().is_empty()
        || input.name.len() > 120
    {
        return bad_request("Check the department code and name.");
    }
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    ensure_reference(&mut conn, "people_departments", input.parent_id).await?;
    let row = sql_query("INSERT INTO people_departments(code,name,parent_id) VALUES ($1,$2,$3) RETURNING id,code,name AS label,parent_id")
        .bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.name.trim()).bind::<Nullable<SqlUuid>,_>(input.parent_id)
        .get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.department.created")
            .target("department", row.id)
            .summary("changed_fields=code,name,parent_id"),
    )
    .await?;
    format::render().status(201).json(row)
}

#[utoipa::path(post, path = "/api/people/grades", security(("bearer" = [])), request_body = GradeInput, responses((status = 201, body = ReferenceRow)))]
#[debug_handler]
pub async fn create_grade(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Json(input): Json<GradeInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    if input.code.trim().is_empty()
        || input.code.len() > 32
        || input.label.trim().is_empty()
        || input.rank < 0
    {
        return bad_request("Check the grade code, label, and rank.");
    }
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let row = sql_query("INSERT INTO people_grades(code,label,rank) VALUES ($1,$2,$3) RETURNING id,code,label,NULL::uuid AS parent_id")
        .bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.label.trim()).bind::<Integer,_>(input.rank)
        .get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.grade.created")
            .target("grade", row.id)
            .summary("changed_fields=code,label,rank"),
    )
    .await?;
    format::render().status(201).json(row)
}

#[utoipa::path(post, path = "/api/people/locations", security(("bearer" = [])), request_body = LocationInput, responses((status = 201, body = ReferenceRow)))]
#[debug_handler]
pub async fn create_location(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Json(input): Json<LocationInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    if input.code.trim().is_empty()
        || input.code.len() > 32
        || input.label.trim().is_empty()
        || input.label.len() > 120
        || input.country_code.len() != 2
        || !input.country_code.chars().all(|c| c.is_ascii_alphabetic())
    {
        return bad_request("Check the location code, label, and ISO country code.");
    }
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let row = sql_query("INSERT INTO people_locations(code,label,country_code,region) VALUES ($1,$2,$3,$4) RETURNING id,code,label,NULL::uuid AS parent_id")
        .bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.label.trim()).bind::<Text,_>(input.country_code.to_uppercase()).bind::<Nullable<Text>,_>(input.region)
        .get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.location.created")
            .target("location", row.id)
            .summary("changed_fields=code,label,country_code,region"),
    )
    .await?;
    format::render().status(201).json(row)
}

#[utoipa::path(post, path = "/api/people/departments/{id}/archive", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn archive_department(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_departments", id, true).await
}

#[utoipa::path(post, path = "/api/people/departments/{id}/restore", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn restore_department(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_departments", id, false).await
}

#[utoipa::path(post, path = "/api/people/grades/{id}/archive", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn archive_grade(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_grades", id, true).await
}

#[utoipa::path(post, path = "/api/people/grades/{id}/restore", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn restore_grade(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_grades", id, false).await
}

#[utoipa::path(post, path = "/api/people/locations/{id}/archive", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn archive_location(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_locations", id, true).await
}

#[utoipa::path(post, path = "/api/people/locations/{id}/restore", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn restore_location(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    archive_reference(auth, ctx, meta, "people_locations", id, false).await
}

async fn archive_reference(
    auth: AuthUser,
    ctx: AppContext,
    meta: RequestMeta,
    table: &str,
    id: Uuid,
    archive: bool,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    if archive {
        let query=match table { "people_departments" => "SELECT EXISTS(SELECT 1 FROM people_employees WHERE department_id=$1 AND archived_at IS NULL) OR EXISTS(SELECT 1 FROM people_departments WHERE parent_id=$1 AND archived_at IS NULL) AS exists", "people_grades" => "SELECT EXISTS(SELECT 1 FROM people_employees WHERE grade_id=$1 AND archived_at IS NULL) AS exists", _ => "SELECT EXISTS(SELECT 1 FROM people_employees WHERE location_id=$1 AND archived_at IS NULL) AS exists" };
        if sql_query(query)
            .bind::<SqlUuid, _>(id)
            .get_result::<Exists>(&mut *conn)
            .await
            .map_err(Database::map_error)?
            .exists
        {
            return bad_request("Archive the dependent employee or child records first.");
        }
    }
    let value = if archive { "now()" } else { "NULL" };
    let parent_guard = if !archive && table == "people_departments" {
        " AND (parent_id IS NULL OR EXISTS (SELECT 1 FROM people_departments p WHERE p.id=people_departments.parent_id AND p.archived_at IS NULL))"
    } else {
        ""
    };
    let sql = format!("UPDATE {table} SET archived_at={value},updated_at=now() WHERE id=$1{parent_guard} RETURNING id");
    let changed = sql_query(&sql)
        .bind::<SqlUuid, _>(id)
        .get_result::<Id>(&mut *conn)
        .await
        .map_err(Database::map_error)?;
    let _ = changed.id;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new(if archive {
            "people.reference.archived"
        } else {
            "people.reference.restored"
        })
        .target(table, id)
        .summary("changed_fields=archived_at"),
    )
    .await?;
    format::render().status(204).json(())
}

#[derive(QueryableByName)]
struct Exists {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    exists: bool,
}
#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people")
        .add("/departments", get(departments).post(create_department))
        .add("/departments/{id}/archive", post(archive_department))
        .add("/departments/{id}/restore", post(restore_department))
        .add("/grades", get(grades).post(create_grade))
        .add("/grades/{id}/archive", post(archive_grade))
        .add("/grades/{id}/restore", post(restore_grade))
        .add("/locations", get(locations).post(create_location))
        .add("/locations/{id}/archive", post(archive_location))
        .add("/locations/{id}/restore", post(restore_location))
}
