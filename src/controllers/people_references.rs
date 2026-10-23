use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    controllers::people::ensure_reference,
    database::sql,
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

#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct ReferenceRow {
    pub id: Uuid,
    code: String,
    label: String,
    parent_id: Option<Uuid>,
}

#[derive(FromQueryResult)]
struct Exists {
    exists: bool,
}

#[derive(FromQueryResult)]
struct IdRow {
    pub id: Uuid,
}

async fn list(auth: AuthUser, ctx: AppContext, table: &'static str) -> Result<Response> {
    require_people_manage(&auth)?;
    let query = match table {
        "people_departments" => "SELECT id,code,name AS label,parent_id FROM people_departments WHERE archived_at IS NULL ORDER BY lower(name)",
        "people_grades" => "SELECT id,code,label,NULL::uuid AS parent_id FROM people_grades WHERE archived_at IS NULL ORDER BY rank",
        _ => "SELECT id,code,label,NULL::uuid AS parent_id FROM people_locations WHERE archived_at IS NULL ORDER BY lower(label)",
    };
    format::json(sql::all::<ReferenceRow>(&ctx.db, query, Vec::new()).await?)
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
    ensure_reference(&ctx.db, "people_departments", input.parent_id).await?;
    let row = sql::one::<ReferenceRow>(&ctx.db,
        "INSERT INTO people_departments(code,name,parent_id) VALUES ($1,$2,$3) RETURNING id,code,name AS label,parent_id",
        vec![input.code.trim().into(), input.name.trim().into(), input.parent_id.into()]).await?;
    audit::record(
        &ctx.db,
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
    let row = sql::one::<ReferenceRow>(&ctx.db,
        "INSERT INTO people_grades(code,label,rank) VALUES ($1,$2,$3) RETURNING id,code,label,NULL::uuid AS parent_id",
        vec![input.code.trim().into(), input.label.trim().into(), input.rank.into()]).await?;
    audit::record(
        &ctx.db,
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
    let row = sql::one::<ReferenceRow>(&ctx.db,
        "INSERT INTO people_locations(code,label,country_code,region) VALUES ($1,$2,$3,$4) RETURNING id,code,label,NULL::uuid AS parent_id",
        vec![input.code.trim().into(), input.label.trim().into(), input.country_code.to_uppercase().into(), input.region.into()]).await?;
    audit::record(
        &ctx.db,
        Some(&auth.user),
        &meta,
        Entry::new("people.location.created")
            .target("location", row.id)
            .summary("changed_fields=code,label,country_code,region"),
    )
    .await?;
    format::render().status(201).json(row)
}

macro_rules! reference_archive_routes {
    ($archive:ident, $restore:ident, $table:literal, $archive_path:literal, $restore_path:literal) => {
        #[utoipa::path(post, path = $archive_path, params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
        #[debug_handler]
        pub async fn $archive(auth: AuthUser, State(ctx): State<AppContext>, meta: RequestMeta, Path(id): Path<Uuid>) -> Result<Response> {
            archive_reference(auth, ctx, meta, $table, id, true).await
        }
        #[utoipa::path(post, path = $restore_path, params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
        #[debug_handler]
        pub async fn $restore(auth: AuthUser, State(ctx): State<AppContext>, meta: RequestMeta, Path(id): Path<Uuid>) -> Result<Response> {
            archive_reference(auth, ctx, meta, $table, id, false).await
        }
    };
}

reference_archive_routes!(
    archive_department,
    restore_department,
    "people_departments",
    "/api/people/departments/{id}/archive",
    "/api/people/departments/{id}/restore"
);
reference_archive_routes!(
    archive_grade,
    restore_grade,
    "people_grades",
    "/api/people/grades/{id}/archive",
    "/api/people/grades/{id}/restore"
);
reference_archive_routes!(
    archive_location,
    restore_location,
    "people_locations",
    "/api/people/locations/{id}/archive",
    "/api/people/locations/{id}/restore"
);

async fn archive_reference(
    auth: AuthUser,
    ctx: AppContext,
    meta: RequestMeta,
    table: &str,
    id: Uuid,
    archive: bool,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let (dependency_query, label) = match table {
        "people_departments" => ("SELECT EXISTS(SELECT 1 FROM people_employees WHERE department_id=$1 AND archived_at IS NULL) OR EXISTS(SELECT 1 FROM people_departments WHERE parent_id=$1 AND archived_at IS NULL) AS exists", "department"),
        "people_grades" => ("SELECT EXISTS(SELECT 1 FROM people_employees WHERE grade_id=$1 AND archived_at IS NULL) AS exists", "grade"),
        "people_locations" => ("SELECT EXISTS(SELECT 1 FROM people_employees WHERE location_id=$1 AND archived_at IS NULL) AS exists", "location"),
        _ => return bad_request("Invalid reference type."),
    };
    if archive
        && sql::one::<Exists>(&ctx.db, dependency_query, vec![id.into()])
            .await?
            .exists
    {
        return bad_request("Archive the dependent employee or child records first.");
    }
    let value = if archive { "now()" } else { "NULL" };
    let parent_guard = if !archive && table == "people_departments" {
        " AND (parent_id IS NULL OR EXISTS (SELECT 1 FROM people_departments p WHERE p.id=people_departments.parent_id AND p.archived_at IS NULL))"
    } else {
        ""
    };
    let query = format!("UPDATE {table} SET archived_at={value},updated_at=now() WHERE id=$1{parent_guard} RETURNING id");
    let archived_id = sql::one::<IdRow>(&ctx.db, &query, vec![id.into()])
        .await?
        .id;
    audit::record(
        &ctx.db,
        Some(&auth.user),
        &meta,
        Entry::new(if archive {
            "people.reference.archived"
        } else {
            "people.reference.restored"
        })
        .target(label, archived_id)
        .summary("changed_fields=archived_at"),
    )
    .await?;
    format::render().status(204).json(())
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
