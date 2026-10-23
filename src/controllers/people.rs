use chrono::NaiveDate;
use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::sql,
    people::{policy::require_people_manage, security},
    request_meta::RequestMeta,
};

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct EmployeeInput {
    pub employee_number: String,
    pub first_name: String,
    pub last_name: String,
    pub work_email: String,
    pub phone: Option<String>,
    pub department_id: Option<Uuid>,
    pub grade_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub employment_type: String,
    pub status: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Employee {
    pub id: Uuid,
    pub employee_number: String,
    pub first_name: String,
    pub last_name: String,
    pub work_email: String,
    pub phone: Option<String>,
    pub department_id: Option<Uuid>,
    pub grade_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub employment_type: String,
    pub status: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

#[derive(Debug, FromQueryResult)]
pub struct EmployeeRow {
    pub id: Uuid,
    pub employee_number: String,
    pub first_name_cipher: String,
    pub last_name_cipher: String,
    pub work_email_cipher: String,
    pub phone_cipher: Option<String>,
    pub department_id: Option<Uuid>,
    pub grade_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub employment_type: String,
    pub status: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

impl TryFrom<EmployeeRow> for Employee {
    type Error = Error;

    fn try_from(row: EmployeeRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            employee_number: row.employee_number,
            first_name: security::decrypt("employee.first_name", &row.first_name_cipher)
                .map_err(|_| Error::string("Employee data could not be decrypted"))?,
            last_name: security::decrypt("employee.last_name", &row.last_name_cipher)
                .map_err(|_| Error::string("Employee data could not be decrypted"))?,
            work_email: security::decrypt("employee.work_email", &row.work_email_cipher)
                .map_err(|_| Error::string("Employee data could not be decrypted"))?,
            phone: row
                .phone_cipher
                .as_deref()
                .map(|value| security::decrypt("employee.phone", value))
                .transpose()
                .map_err(|_| Error::string("Employee data could not be decrypted"))?,
            department_id: row.department_id,
            grade_id: row.grade_id,
            location_id: row.location_id,
            manager_id: row.manager_id,
            employment_type: row.employment_type,
            status: row.status,
            start_date: row.start_date,
            end_date: row.end_date,
        })
    }
}

#[derive(Debug, FromQueryResult)]
struct Exists {
    exists: bool,
}

pub async fn ensure_reference(
    connection: &impl sea_orm::ConnectionTrait,
    table: &str,
    id: Option<Uuid>,
) -> Result<()> {
    let Some(id) = id else { return Ok(()) };
    let table = match table {
        "people_departments" | "people_grades" | "people_locations" | "people_employees" => table,
        _ => return Err(Error::BadRequest("Invalid reference type.".to_owned())),
    };
    let row = sql::one::<Exists>(
        connection,
        &format!(
            "SELECT EXISTS(SELECT 1 FROM {table} WHERE id=$1 AND archived_at IS NULL) AS exists"
        ),
        vec![id.into()],
    )
    .await?;
    if row.exists {
        Ok(())
    } else {
        Err(Error::BadRequest(
            "Reference does not exist or is archived.".to_owned(),
        ))
    }
}

pub fn validate_employee(input: &EmployeeInput) -> Result<()> {
    let email_valid = input.work_email.len() <= 254
        && input
            .work_email
            .split_once('@')
            .is_some_and(|(name, domain)| {
                !name.is_empty() && domain.contains('.') && !domain.starts_with('.')
            });
    let valid_type = matches!(
        input.employment_type.as_str(),
        "full_time" | "part_time" | "contract" | "intern"
    );
    let valid_status = matches!(
        input.status.as_str(),
        "draft" | "preboarding" | "active" | "on_leave" | "probation" | "notice" | "exited"
    );
    if input.employee_number.trim().is_empty()
        || input.employee_number.len() > 64
        || input.first_name.trim().is_empty()
        || input.first_name.len() > 120
        || input.last_name.trim().is_empty()
        || input.last_name.len() > 120
        || !email_valid
        || !valid_type
        || !valid_status
    {
        return Err(Error::BadRequest(
            "Check the employee fields and allowed values.".to_owned(),
        ));
    }
    let start = NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| Error::BadRequest("start_date must be YYYY-MM-DD".to_owned()))?;
    if let Some(end) = input.end_date.as_deref() {
        let end = NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .map_err(|_| Error::BadRequest("end_date must be YYYY-MM-DD".to_owned()))?;
        if end < start {
            return Err(Error::BadRequest(
                "end_date cannot precede start_date".to_owned(),
            ));
        }
    }
    if input.status == "active"
        && (input.department_id.is_none()
            || input.grade_id.is_none()
            || input.location_id.is_none())
    {
        return Err(Error::BadRequest(
            "Active employees need a department, grade, and location.".to_owned(),
        ));
    }
    if input.status == "exited" && input.end_date.is_none() {
        return Err(Error::BadRequest(
            "Exited employees need an end date.".to_owned(),
        ));
    }
    Ok(())
}

pub(super) const EMPLOYEE_COLUMNS: &str = "id,employee_number,first_name_cipher,last_name_cipher,work_email_cipher,phone_cipher,department_id,grade_id,location_id,manager_id,employment_type,status,start_date::text AS start_date,end_date::text AS end_date";

#[utoipa::path(get, path = "/api/people/employees", security(("bearer" = [])), responses((status = 200, body = [Employee])))]
#[debug_handler]
pub async fn list_employees(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    require_people_manage(&auth)?;
    let rows = sql::all::<EmployeeRow>(
        &ctx.db,
        &format!("SELECT {EMPLOYEE_COLUMNS} FROM people_employees WHERE archived_at IS NULL ORDER BY employee_number LIMIT 500"),
        Vec::new(),
    )
    .await?;
    let employees = rows
        .into_iter()
        .map(Employee::try_from)
        .collect::<Result<Vec<_>>>()?;
    format::json(employees)
}

#[utoipa::path(post, path = "/api/people/employees", security(("bearer" = [])), request_body = EmployeeInput, responses((status = 201, body = Employee)))]
#[debug_handler]
pub async fn create_employee(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Json(input): Json<EmployeeInput>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    validate_employee(&input)?;
    ensure_reference(&ctx.db, "people_departments", input.department_id).await?;
    ensure_reference(&ctx.db, "people_grades", input.grade_id).await?;
    ensure_reference(&ctx.db, "people_locations", input.location_id).await?;
    ensure_reference(&ctx.db, "people_employees", input.manager_id).await?;

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
    let actor = auth.user.id;
    let history_data = serde_json::json!({
        "employee_number": input.employee_number.clone(),
        "department_id": input.department_id,
        "grade_id": input.grade_id,
        "location_id": input.location_id,
        "manager_id": input.manager_id,
        "employment_type": input.employment_type.clone(),
        "status": input.status.clone(),
        "start_date": input.start_date.clone(),
        "end_date": input.end_date.clone(),
        "first_name": "[REDACTED]",
        "last_name": "[REDACTED]",
        "work_email": "[REDACTED]",
        "phone": "[REDACTED]"
    });
    let row = ctx
        .db
        .transaction::<_, EmployeeRow, Error>(|transaction| {
            Box::pin(async move {
                let row = sql::one::<EmployeeRow>(
                    transaction,
                    &format!("INSERT INTO people_employees(employee_number,first_name_cipher,last_name_cipher,work_email_cipher,email_lookup,phone_cipher,department_id,grade_id,location_id,manager_id,employment_type,status,start_date,end_date) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING {EMPLOYEE_COLUMNS}"),
                    vec![
                        input.employee_number.trim().into(), first.into(), last.into(), email.into(),
                        lookup.into(), phone.into(), input.department_id.into(), input.grade_id.into(),
                        input.location_id.into(), input.manager_id.into(), input.employment_type.into(),
                        input.status.into(), start.into(), end.into(),
                    ],
                )
                .await?;
                sql::execute(
                    transaction,
                    "INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,after_data) VALUES ($1,$2,'created',ARRAY['employee']::text[],$3)",
                    vec![row.id.into(), actor.into(), history_data.into()],
                )
                .await?;
                Ok(row)
            })
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "employee creation transaction failed");
            Error::string("Employee creation failed")
        })?;
    let employee = Employee::try_from(row)?;
    audit::record(
        &ctx.db,
        Some(&auth.user),
        &meta,
        Entry::new("people.employee.created")
            .target("employee", employee.id)
            .summary("changed_fields=employee"),
    )
    .await?;
    format::render().status(201).json(employee)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people/employees")
        .add("/", get(list_employees).post(create_employee))
}
