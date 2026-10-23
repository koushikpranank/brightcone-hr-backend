use axum::http::header;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    authz::AuthUser,
    controllers::people::{validate_employee, EmployeeInput},
    people::policy::require_people_manage,
};

const HEADERS: [&str; 13] = [
    "employee_number",
    "first_name",
    "last_name",
    "work_email",
    "phone",
    "department_id",
    "grade_id",
    "location_id",
    "manager_id",
    "employment_type",
    "status",
    "start_date",
    "end_date",
];

#[derive(Debug, Deserialize, ToSchema)]
pub struct CsvPayload {
    pub csv: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CsvIssue {
    pub row: usize,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CsvDryRun {
    pub valid: bool,
    pub rows: usize,
    pub issues: Vec<CsvIssue>,
}

#[utoipa::path(post, path = "/api/people/employees/import/dry-run", security(("bearer" = [])), request_body = CsvPayload, responses((status = 200, body = CsvDryRun)))]
#[debug_handler]
pub async fn dry_run(
    auth: AuthUser,
    State(_ctx): State<AppContext>,
    Json(payload): Json<CsvPayload>,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let (rows, mut issues) = parse_rows(&payload.csv);
    for (index, values) in rows.iter().enumerate() {
        if values.len() != HEADERS.len() {
            issues.push(CsvIssue {
                row: index + 2,
                message: "Expected 13 columns.".to_owned(),
            });
            continue;
        }
        let input = to_input(values);
        if let Err(error) = validate_employee(&input) {
            issues.push(CsvIssue {
                row: index + 2,
                message: error.to_string(),
            });
        }
    }
    let rows_count = rows.len();
    format::json(CsvDryRun {
        valid: issues.is_empty(),
        rows: rows_count,
        issues,
    })
}

#[utoipa::path(get, path = "/api/people/employees/import/template.csv", security(("bearer" = [])), responses((status = 200, content_type = "text/csv")))]
#[debug_handler]
pub async fn template(auth: AuthUser, State(_ctx): State<AppContext>) -> Result<Response> {
    require_people_manage(&auth)?;
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        format!("{}\n", HEADERS.join(",")),
    )
        .into_response())
}

fn parse_rows(csv: &str) -> (Vec<Vec<String>>, Vec<CsvIssue>) {
    let mut lines = csv.lines();
    let headers = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    let mut issues = Vec::new();
    if headers != HEADERS {
        issues.push(CsvIssue {
            row: 1,
            message: "Header must match the employee template.".to_owned(),
        });
    }
    let rows = lines
        .filter(|line| !line.trim().is_empty())
        .map(parse_line)
        .collect();
    (rows, issues)
}

fn parse_line(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                values.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(character),
        }
    }
    values.push(current.trim().to_owned());
    values
}

fn to_input(values: &[String]) -> EmployeeInput {
    let uuid = |value: &str| {
        (!value.is_empty())
            .then(|| uuid::Uuid::parse_str(value).ok())
            .flatten()
    };
    EmployeeInput {
        employee_number: values[0].clone(),
        first_name: values[1].clone(),
        last_name: values[2].clone(),
        work_email: values[3].clone(),
        phone: (!values[4].is_empty()).then(|| values[4].clone()),
        department_id: uuid(&values[5]),
        grade_id: uuid(&values[6]),
        location_id: uuid(&values[7]),
        manager_id: uuid(&values[8]),
        employment_type: values[9].clone(),
        status: values[10].clone(),
        start_date: values[11].clone(),
        end_date: (!values[12].is_empty()).then(|| values[12].clone()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people/employees/import")
        .add("/dry-run", post(dry_run))
        .add("/template.csv", get(template))
}
