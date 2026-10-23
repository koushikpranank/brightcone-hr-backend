use chrono::NaiveDate;
use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{BigInt, Date, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use loco_rs::prelude::*;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    controllers::people::{
        ensure_reference, validate_employee, Employee, EmployeeInput, EmployeeRow,
    },
    database::Database,
    people::{policy::require_people_manage, security},
    request_meta::RequestMeta,
};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct EmployeeMutationInput {
    pub input: EmployeeInput,
    pub reason_code: String,
}

#[utoipa::path(put, path = "/api/people/employees/{id}", params(("id" = Uuid, Path, description = "Employee id")), security(("bearer" = [])), request_body = EmployeeMutationInput, responses((status = 200, body = Employee)))]
#[debug_handler]
pub async fn update(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
    Json(payload): Json<EmployeeMutationInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    validate_employee(&payload.input)?;
    if payload.reason_code.trim().is_empty() || payload.reason_code.len() > 80 {
        return bad_request("A reason code is required.");
    }
    let input = &payload.input;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    ensure_reference(&mut conn, "people_departments", input.department_id).await?;
    ensure_reference(&mut conn, "people_grades", input.grade_id).await?;
    ensure_reference(&mut conn, "people_locations", input.location_id).await?;
    ensure_reference(&mut conn, "people_employees", input.manager_id).await?;
    if let Some(manager) = input.manager_id {
        let cycle: (bool,) = sql_query("SELECT EXISTS(WITH RECURSIVE chain(id) AS (SELECT $1::uuid UNION ALL SELECT e.manager_id FROM people_employees e JOIN chain c ON e.id=c.id WHERE e.manager_id IS NOT NULL) SELECT 1 FROM chain WHERE id=$2) AS exists")
            .bind::<SqlUuid,_>(manager).bind::<SqlUuid,_>(id).get_result::<CycleRow>(&mut *conn).await.map(|r| (r.exists,)).map_err(Database::map_error)?;
        if cycle.0 {
            return bad_request("Manager assignment would create a hierarchy cycle.");
        }
    }
    let before = sql_query("SELECT id,employee_number,first_name_cipher,last_name_cipher,work_email_cipher,phone_cipher,department_id,grade_id,location_id,manager_id,employment_type,status,start_date::text AS start_date,end_date::text AS end_date FROM people_employees WHERE id=$1 AND archived_at IS NULL FOR UPDATE")
        .bind::<SqlUuid,_>(id).get_result::<EmployeeRow>(&mut *conn).await.map_err(Database::map_error)?;
    let before_view = redacted(&before);
    let first = security::encrypt("employee.first_name", input.first_name.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let last = security::encrypt("employee.last_name", input.last_name.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let email = security::encrypt("employee.work_email", input.work_email.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let phone = input
        .phone
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|v| security::encrypt("employee.phone", v))
        .transpose()
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let lookup = security::email_lookup(&input.work_email)
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let start = NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| Error::BadRequest("start_date must be YYYY-MM-DD".to_owned()))?;
    let end = input
        .end_date
        .as_deref()
        .map(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| Error::BadRequest("end_date must be YYYY-MM-DD".to_owned()))?;
    let number = input.employee_number.trim().to_owned();
    let employment = input.employment_type.clone();
    let status = input.status.clone();
    let actor = auth.user.id;
    let reason = payload.reason_code.trim().to_owned();
    let row = conn.transaction::<EmployeeRow, diesel::result::Error, _>(async move |conn| {
        let row=sql_query("UPDATE people_employees SET employee_number=$1,first_name_cipher=$2,last_name_cipher=$3,work_email_cipher=$4,email_lookup=$5,phone_cipher=$6,department_id=$7,grade_id=$8,location_id=$9,manager_id=$10,employment_type=$11,status=$12,start_date=$13,end_date=$14,updated_at=now() WHERE id=$15 AND archived_at IS NULL RETURNING id,employee_number,first_name_cipher,last_name_cipher,work_email_cipher,phone_cipher,department_id,grade_id,location_id,manager_id,employment_type,status,start_date::text AS start_date,end_date::text AS end_date").bind::<Text,_>(&number).bind::<Text,_>(first).bind::<Text,_>(last).bind::<Text,_>(email).bind::<Text,_>(lookup).bind::<Nullable<Text>,_>(phone).bind::<Nullable<SqlUuid>,_>(input.department_id).bind::<Nullable<SqlUuid>,_>(input.grade_id).bind::<Nullable<SqlUuid>,_>(input.location_id).bind::<Nullable<SqlUuid>,_>(input.manager_id).bind::<Text,_>(&employment).bind::<Text,_>(&status).bind::<Date,_>(start).bind::<Nullable<Date>,_>(end).bind::<SqlUuid,_>(id).get_result::<EmployeeRow>(&mut *conn).await?;
        sql_query("INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,before_data,after_data,reason_code) VALUES ($1,$2,'updated',$3,$4,$5,$6)").bind::<SqlUuid,_>(id).bind::<BigInt,_>(actor).bind::<diesel::sql_types::Array<Text>,_>(vec!["employee".to_owned()]).bind::<diesel::sql_types::Jsonb,_>(before_view).bind::<diesel::sql_types::Jsonb,_>(redacted(&row)).bind::<Text,_>(&reason).execute(&mut *conn).await?; Ok(row)
    }).await.map_err(Database::map_error)?;
    let employee = Employee::try_from(row)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.employee.updated")
            .target("employee", id)
            .summary("changed_fields=employee"),
    )
    .await?;
    format::json(employee)
}

#[derive(QueryableByName)]
struct CycleRow {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    exists: bool,
}

#[utoipa::path(post, path = "/api/people/employees/{id}/archive", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn archive(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    set_archived(auth, ctx, meta, id, true).await
}

#[utoipa::path(post, path = "/api/people/employees/{id}/restore", params(("id" = Uuid, Path)), security(("bearer" = [])), responses((status = 204)))]
#[debug_handler]
pub async fn restore(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    set_archived(auth, ctx, meta, id, false).await
}

async fn set_archived(
    auth: AuthUser,
    ctx: AppContext,
    meta: RequestMeta,
    id: Uuid,
    archive: bool,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let value = if archive { "now()" } else { "NULL" };
    let sql = format!(
        "UPDATE people_employees SET archived_at={value},updated_at=now() WHERE id=$1 RETURNING id"
    );
    let row = sql_query(&sql)
        .bind::<SqlUuid, _>(id)
        .get_result::<IdRow>(&mut *conn)
        .await
        .map_err(Database::map_error)?;
    sql_query("INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,reason_code) VALUES ($1,$2,$3,ARRAY['archived_at']::text[],$4)").bind::<SqlUuid,_>(row.id).bind::<BigInt,_>(auth.user.id).bind::<Text,_>(if archive{"archived"}else{"restored"}).bind::<Text,_>(if archive{"hr_archive"}else{"hr_restore"}).execute(&mut *conn).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new(if archive {
            "people.employee.archived"
        } else {
            "people.employee.restored"
        })
        .target("employee", id),
    )
    .await?;
    format::render().status(204).json(())
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type=SqlUuid)]
    id: Uuid,
}
fn redacted(row: &EmployeeRow) -> serde_json::Value {
    json!({"id":row.id,"employee_number":row.employee_number,"department_id":row.department_id,"grade_id":row.grade_id,"location_id":row.location_id,"manager_id":row.manager_id,"employment_type":row.employment_type,"status":row.status,"start_date":row.start_date,"end_date":row.end_date,"first_name":"[REDACTED]","last_name":"[REDACTED]","work_email":"[REDACTED]","phone":"[REDACTED]"})
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people/employees")
        .add("/{id}", put(update))
        .add("/{id}/archive", post(archive))
        .add("/{id}/restore", post(restore))
}
