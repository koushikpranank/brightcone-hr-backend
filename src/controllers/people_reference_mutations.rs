use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    controllers::people_references::{DepartmentInput, GradeInput, LocationInput, ReferenceRow},
    database::Database,
    people::policy::require_people_manage,
    request_meta::RequestMeta,
};
use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{Integer, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::RunQueryDsl;
use loco_rs::prelude::*;
use uuid::Uuid;

#[utoipa::path(put, path = "/api/people/departments/{id}", params(("id" = Uuid, Path)), security(("bearer" = [])), request_body = DepartmentInput, responses((status = 200, body = ReferenceRow)))]
#[debug_handler]
pub async fn department(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
    Json(input): Json<DepartmentInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return bad_request("Check the department code and name.");
    }
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    if let Some(parent) = input.parent_id {
        if parent==id || sql_query("SELECT EXISTS(WITH RECURSIVE chain(id) AS (SELECT $1::uuid UNION ALL SELECT parent_id FROM people_departments d JOIN chain c ON d.id=c.id WHERE d.parent_id IS NOT NULL) SELECT 1 FROM chain WHERE id=$2) AS exists").bind::<SqlUuid,_>(parent).bind::<SqlUuid,_>(id).get_result::<Exists>(&mut *conn).await.map_err(Database::map_error)?.exists { return bad_request("Department assignment would create a hierarchy cycle."); }
    }
    let row=sql_query("UPDATE people_departments SET code=$1,name=$2,parent_id=$3,updated_at=now() WHERE id=$4 AND archived_at IS NULL RETURNING id,code,name AS label,parent_id").bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.name.trim()).bind::<Nullable<SqlUuid>,_>(input.parent_id).bind::<SqlUuid,_>(id).get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.department.updated")
            .target("department", id)
            .summary("changed_fields=code,name,parent_id"),
    )
    .await?;
    format::json(row)
}

#[utoipa::path(put, path = "/api/people/grades/{id}", params(("id" = Uuid, Path)), security(("bearer" = [])), request_body = GradeInput, responses((status = 200, body = ReferenceRow)))]
#[debug_handler]
pub async fn grade(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
    Json(input): Json<GradeInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let row=sql_query("UPDATE people_grades SET code=$1,label=$2,rank=$3,updated_at=now() WHERE id=$4 AND archived_at IS NULL RETURNING id,code,label,NULL::uuid AS parent_id").bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.label.trim()).bind::<Integer,_>(input.rank).bind::<SqlUuid,_>(id).get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.grade.updated")
            .target("grade", id)
            .summary("changed_fields=code,label,rank"),
    )
    .await?;
    format::json(row)
}

#[utoipa::path(put, path = "/api/people/locations/{id}", params(("id" = Uuid, Path)), security(("bearer" = [])), request_body = LocationInput, responses((status = 200, body = ReferenceRow)))]
#[debug_handler]
pub async fn location(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
    Json(input): Json<LocationInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let row=sql_query("UPDATE people_locations SET code=$1,label=$2,country_code=$3,region=$4,updated_at=now() WHERE id=$5 AND archived_at IS NULL RETURNING id,code,label,NULL::uuid AS parent_id").bind::<Text,_>(input.code.trim()).bind::<Text,_>(input.label.trim()).bind::<Text,_>(input.country_code.to_uppercase()).bind::<Nullable<Text>,_>(input.region).bind::<SqlUuid,_>(id).get_result::<ReferenceRow>(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.location.updated")
            .target("location", id)
            .summary("changed_fields=code,label,country_code,region"),
    )
    .await?;
    format::json(row)
}

#[derive(QueryableByName)]
struct Exists {
    #[diesel(sql_type=diesel::sql_types::Bool)]
    exists: bool,
}
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people")
        .add("/departments/{id}", put(department))
        .add("/grades/{id}", put(grade))
        .add("/locations/{id}", put(location))
}
