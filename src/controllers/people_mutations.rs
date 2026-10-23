use chrono::NaiveDate;
use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    controllers::people::{
        ensure_reference, validate_employee, Employee, EmployeeInput, EmployeeRow,
    },
    database::sql,
    people::{policy::require_people_manage, security},
    request_meta::RequestMeta,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmployeeMutationInput {
    pub input: EmployeeInput,
    pub reason_code: String,
}

#[derive(FromQueryResult)]
struct IdRow {
    id: Uuid,
}

#[utoipa::path(put, path = "/api/people/employees/{id}", params(("id" = Uuid, Path)), security(("bearer" = [])), request_body = EmployeeMutationInput, responses((status = 200, body = Employee)))]
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
    let reason = payload.reason_code.trim();
    if reason.is_empty() || reason.len() > 80 {
        return bad_request("A reason code is required.");
    }
    let input = &payload.input;
    ensure_reference(&ctx.db, "people_departments", input.department_id).await?;
    ensure_reference(&ctx.db, "people_grades", input.grade_id).await?;
    ensure_reference(&ctx.db, "people_locations", input.location_id).await?;
    ensure_reference(&ctx.db, "people_employees", input.manager_id).await?;
    if let Some(manager) = input.manager_id {
        #[derive(FromQueryResult)]
        struct CycleRow {
            exists: bool,
        }
        let cycle = sql::one::<CycleRow>(
            &ctx.db,
            "SELECT EXISTS(WITH RECURSIVE chain(id) AS (SELECT $1::uuid UNION ALL SELECT e.manager_id FROM people_employees e JOIN chain c ON e.id=c.id WHERE e.manager_id IS NOT NULL) SELECT 1 FROM chain WHERE id=$2) AS exists",
            vec![manager.into(), id.into()],
        ).await?;
        if cycle.exists {
            return bad_request("Manager assignment would create a hierarchy cycle.");
        }
    }
    let before = sql::one::<EmployeeRow>(
        &ctx.db,
        &format!(
            "SELECT {} FROM people_employees WHERE id=$1 AND archived_at IS NULL FOR UPDATE",
            super::people::EMPLOYEE_COLUMNS
        ),
        vec![id.into()],
    )
    .await?;
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
        .filter(|value| !value.trim().is_empty())
        .map(|value| security::encrypt("employee.phone", value.trim()))
        .transpose()
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let lookup = security::email_lookup(&input.work_email)
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let start = NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| Error::BadRequest("start_date must be YYYY-MM-DD".to_owned()))?;
    let end = input
        .end_date
        .as_deref()
        .map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .transpose()
        .map_err(|_| Error::BadRequest("end_date must be YYYY-MM-DD".to_owned()))?;
    let reason = reason.to_owned();
    let before_data = before_view;
    let actor = auth.user.id;
    let number = input.employee_number.trim().to_owned();
    let employment = input.employment_type.clone();
    let status = input.status.clone();
    let department_id = input.department_id;
    let grade_id = input.grade_id;
    let location_id = input.location_id;
    let manager_id = input.manager_id;
    let (row, after_data) = ctx.db.transaction::<_, (EmployeeRow, serde_json::Value), Error>(|tx| {
        Box::pin(async move {
            let row = sql::one::<EmployeeRow>(
                tx,
                &format!("UPDATE people_employees SET employee_number=$1,first_name_cipher=$2,last_name_cipher=$3,work_email_cipher=$4,email_lookup=$5,phone_cipher=$6,department_id=$7,grade_id=$8,location_id=$9,manager_id=$10,employment_type=$11,status=$12,start_date=$13,end_date=$14,updated_at=now() WHERE id=$15 AND archived_at IS NULL RETURNING {}", super::people::EMPLOYEE_COLUMNS),
                vec![number.into(), first.into(), last.into(), email.into(), lookup.into(), phone.into(), department_id.into(), grade_id.into(), location_id.into(), manager_id.into(), employment.into(), status.into(), start.into(), end.into(), id.into()],
            ).await?;
            let after_data = redacted(&row);
            sql::execute(tx,
                "INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,before_data,after_data,reason_code) VALUES ($1,$2,'updated',ARRAY['employee']::text[],$3,$4,$5)",
                vec![id.into(), actor.into(), before_data.into(), after_data.clone().into(), reason.into()],
            ).await?;
            Ok((row, after_data))
        })
    }).await.map_err(|error| {
        tracing::error!(%error, "employee update transaction failed");
        Error::string("Employee update failed")
    })?;
    let employee = Employee::try_from(row)?;
    audit::record(
        &ctx.db,
        Some(&auth.user),
        &meta,
        Entry::new("people.employee.updated")
            .target("employee", id)
            .summary(format!(
                "changed_fields=employee; history_id={}",
                after_data["id"].as_str().unwrap_or_default()
            )),
    )
    .await?;
    format::json(employee)
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
    let value = if archive { "now()" } else { "NULL" };
    let row = sql::one::<IdRow>(&ctx.db,
        &format!("UPDATE people_employees SET archived_at={value},updated_at=now() WHERE id=$1 RETURNING id"), vec![id.into()]).await?;
    sql::execute(&ctx.db,
        "INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,reason_code) VALUES ($1,$2,$3,ARRAY['archived_at']::text[],$4)",
        vec![row.id.into(), auth.user.id.into(), (if archive { "archived" } else { "restored" }).into(), (if archive { "hr_archive" } else { "hr_restore" }).into()]).await?;
    audit::record(
        &ctx.db,
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
