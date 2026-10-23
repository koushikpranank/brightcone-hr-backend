use chrono::NaiveDate;
use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::AsyncConnection;
use diesel_async::RunQueryDsl;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::Database,
    people::policy::require_people_manage,
    people::security,
    request_meta::RequestMeta,
};

#[derive(Debug, QueryableByName)]
pub(crate) struct EmployeeRow {
    #[diesel(sql_type = SqlUuid)]
    pub(crate) id: Uuid,
    #[diesel(sql_type = Text)]
    pub(crate) employee_number: String,
    #[diesel(sql_type = Text)]
    pub(crate) first_name_cipher: String,
    #[diesel(sql_type = Text)]
    pub(crate) last_name_cipher: String,
    #[diesel(sql_type = Text)]
    pub(crate) work_email_cipher: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub(crate) phone_cipher: Option<String>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub(crate) department_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub(crate) grade_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub(crate) location_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub(crate) manager_id: Option<Uuid>,
    #[diesel(sql_type = Text)]
    pub(crate) employment_type: String,
    #[diesel(sql_type = Text)]
    pub(crate) status: String,
    #[diesel(sql_type = Text)]
    pub(crate) start_date: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub(crate) end_date: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Employee {
    pub(crate) id: Uuid,
    pub(crate) employee_number: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) work_email: String,
    pub(crate) phone: Option<String>,
    pub(crate) department_id: Option<Uuid>,
    pub(crate) grade_id: Option<Uuid>,
    pub(crate) location_id: Option<Uuid>,
    pub(crate) manager_id: Option<Uuid>,
    pub(crate) employment_type: String,
    pub(crate) status: String,
    pub(crate) start_date: String,
    pub(crate) end_date: Option<String>,
}

impl TryFrom<EmployeeRow> for Employee {
    type Error = Error;
    fn try_from(row: EmployeeRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            employee_number: row.employee_number,
            first_name: security::decrypt("employee.first_name", &row.first_name_cipher)
                .map_err(|_| Error::string("People data is unavailable"))?,
            last_name: security::decrypt("employee.last_name", &row.last_name_cipher)
                .map_err(|_| Error::string("People data is unavailable"))?,
            work_email: security::decrypt("employee.work_email", &row.work_email_cipher)
                .map_err(|_| Error::string("People data is unavailable"))?,
            phone: row
                .phone_cipher
                .map(|v| {
                    security::decrypt("employee.phone", &v)
                        .map_err(|_| Error::string("People data is unavailable"))
                })
                .transpose()?,
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmployeeInput {
    pub(crate) employee_number: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) work_email: String,
    pub(crate) phone: Option<String>,
    pub(crate) department_id: Option<Uuid>,
    pub(crate) grade_id: Option<Uuid>,
    pub(crate) location_id: Option<Uuid>,
    pub(crate) manager_id: Option<Uuid>,
    pub(crate) employment_type: String,
    pub(crate) status: String,
    pub(crate) start_date: String,
    pub(crate) end_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EmployeeListQuery {
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(QueryableByName)]
struct ExistsRow {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    exists: bool,
}

pub(crate) async fn ensure_reference(
    conn: &mut diesel_async::AsyncPgConnection,
    table: &'static str,
    id: Option<Uuid>,
) -> Result<()> {
    let Some(id) = id else {
        return Ok(());
    };
    let query = match table {
        "people_departments" => "SELECT EXISTS(SELECT 1 FROM people_departments WHERE id=$1 AND archived_at IS NULL) AS exists",
        "people_grades" => "SELECT EXISTS(SELECT 1 FROM people_grades WHERE id=$1 AND archived_at IS NULL) AS exists",
        "people_locations" => "SELECT EXISTS(SELECT 1 FROM people_locations WHERE id=$1 AND archived_at IS NULL) AS exists",
        _ => "SELECT EXISTS(SELECT 1 FROM people_employees WHERE id=$1 AND status='active' AND archived_at IS NULL) AS exists",
    };
    let valid = sql_query(query)
        .bind::<SqlUuid, _>(id)
        .get_result::<ExistsRow>(conn)
        .await
        .map_err(Database::map_error)?
        .exists;
    if valid {
        Ok(())
    } else {
        bad_request("An assigned organization record or manager is unavailable.")
    }
}

#[utoipa::path(get, path = "/api/people/employees", security(("bearer" = [])), responses((status = 200, body = [Employee])))]
#[debug_handler]
pub async fn list_employees(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Query(query): Query<EmployeeListQuery>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 100);
    let offset = (page.saturating_sub(1) * page_size) as i64;
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let lookup = search
        .filter(|value| value.contains('@'))
        .and_then(|value| security::email_lookup(value).ok());
    let pattern = search.map(|value| format!("%{value}%"));
    let rows = sql_query("SELECT id, employee_number, first_name_cipher, last_name_cipher, work_email_cipher, phone_cipher, department_id, grade_id, location_id, manager_id, employment_type, status, start_date::text AS start_date, end_date::text AS end_date FROM people_employees WHERE archived_at IS NULL AND ($1::text IS NULL OR employee_number ILIKE $1 OR status ILIKE $1 OR email_lookup = $2) ORDER BY employee_number LIMIT $3 OFFSET $4")
        .bind::<Nullable<Text>, _>(pattern)
        .bind::<Nullable<Text>, _>(lookup)
        .bind::<diesel::sql_types::BigInt, _>(page_size as i64)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load::<EmployeeRow>(&mut *conn).await.map_err(Database::map_error)?;
    let items = rows
        .into_iter()
        .map(Employee::try_from)
        .collect::<Result<Vec<_>>>()?;
    format::json(items)
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
    let encrypted_first = security::encrypt("employee.first_name", input.first_name.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let encrypted_last = security::encrypt("employee.last_name", input.last_name.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let encrypted_email = security::encrypt("employee.work_email", input.work_email.trim())
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let encrypted_phone = input
        .phone
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|value| security::encrypt("employee.phone", value))
        .transpose()
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let lookup = security::email_lookup(&input.work_email)
        .map_err(|_| Error::string("People data encryption is unavailable"))?;
    let start_date = NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| Error::BadRequest("start_date must be YYYY-MM-DD".to_owned()))?;
    let end_date = input
        .end_date
        .as_deref()
        .map(|date| {
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| Error::BadRequest("end_date must be YYYY-MM-DD".to_owned()))
        })
        .transpose()?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    ensure_reference(&mut conn, "people_departments", input.department_id).await?;
    ensure_reference(&mut conn, "people_grades", input.grade_id).await?;
    ensure_reference(&mut conn, "people_locations", input.location_id).await?;
    ensure_reference(&mut conn, "people_employees", input.manager_id).await?;
    let employee_number = input.employee_number.trim().to_owned();
    let employment_type = input.employment_type;
    let status = input.status;
    let actor_id = auth.user.id;
    let row = conn.transaction::<EmployeeRow, diesel::result::Error, _>(async move |conn| {
        let row = sql_query("INSERT INTO people_employees(employee_number, first_name_cipher, last_name_cipher, work_email_cipher, email_lookup, phone_cipher, department_id, grade_id, location_id, manager_id, employment_type, status, start_date, end_date) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING id, employee_number, first_name_cipher, last_name_cipher, work_email_cipher, phone_cipher, department_id, grade_id, location_id, manager_id, employment_type, status, start_date::text AS start_date, end_date::text AS end_date")
        .bind::<Text,_>(&employee_number).bind::<Text,_>(encrypted_first).bind::<Text,_>(encrypted_last)
        .bind::<Text,_>(encrypted_email).bind::<Text,_>(lookup).bind::<Nullable<Text>,_>(encrypted_phone)
        .bind::<Nullable<SqlUuid>,_>(input.department_id).bind::<Nullable<SqlUuid>,_>(input.grade_id)
        .bind::<Nullable<SqlUuid>,_>(input.location_id).bind::<Nullable<SqlUuid>,_>(input.manager_id)
        .bind::<Text,_>(&employment_type).bind::<Text,_>(&status).bind::<diesel::sql_types::Date,_>(start_date)
        .bind::<Nullable<diesel::sql_types::Date>,_>(end_date)
        .get_result::<EmployeeRow>(&mut *conn).await?;
        sql_query("INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields) VALUES ($1,$2,'created',ARRAY['employee_number','first_name','last_name','work_email','phone','department_id','grade_id','location_id','manager_id','employment_type','status','start_date','end_date']::text[])")
            .bind::<SqlUuid,_>(row.id).bind::<BigInt,_>(actor_id).execute(&mut *conn).await?;
        Ok(row)
    }).await.map_err(Database::map_error)?;
    let employee = Employee::try_from(row)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.employee.created")
            .target("employee", employee.id)
            .summary("changed_fields=employee_number,identity,organization,status"),
    )
    .await?;
    format::render().status(201).json(employee)
}

pub(crate) fn validate_employee(input: &EmployeeInput) -> Result<()> {
    if input.employee_number.trim().is_empty()
        || input.employee_number.len() > 64
        || input.first_name.trim().is_empty()
        || input.first_name.len() > 100
        || input.last_name.trim().is_empty()
        || input.last_name.len() > 100
        || !valid_email(&input.work_email)
        || input.work_email.len() > 254
        || input
            .phone
            .as_deref()
            .is_some_and(|phone| !phone.trim().is_empty() && !valid_phone(phone))
    {
        return bad_request("Check the employee number, name, and work email.");
    }
    if !["full_time", "part_time", "contract", "intern"].contains(&input.employment_type.as_str())
        || ![
            "draft",
            "preboarding",
            "active",
            "on_leave",
            "probation",
            "notice",
            "exited",
        ]
        .contains(&input.status.as_str())
    {
        return bad_request("Employment type or status is not supported.");
    }
    if input.manager_id == Some(Uuid::nil()) {
        return bad_request("Manager is invalid.");
    }
    if input.status == "active"
        && (input.department_id.is_none()
            || input.grade_id.is_none()
            || input.location_id.is_none())
    {
        return bad_request("Active employees require a department, grade, and location.");
    }
    if input.status == "exited" && input.end_date.is_none() {
        return bad_request("Exited employees require an end date.");
    }
    let start = NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| Error::BadRequest("start_date must be YYYY-MM-DD".to_owned()))?;
    if let Some(date) = &input.end_date {
        let end = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| Error::BadRequest("end_date must be YYYY-MM-DD".to_owned()))?;
        if end < start {
            return bad_request("End date cannot precede start date.");
        }
    }
    Ok(())
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.trim().split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value.chars().any(char::is_whitespace)
        && value.len() <= 254
}

fn valid_phone(value: &str) -> bool {
    let digits = value.strip_prefix('+').unwrap_or_default();
    (8..=15).contains(&digits.len()) && digits.chars().all(|digit| digit.is_ascii_digit())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people")
        .add("/employees", get(list_employees).post(create_employee))
}
