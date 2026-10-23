use std::collections::HashSet;

use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{authz::AuthUser, controllers::records::Record, database::sql, records};

const EXPORT_LIMIT: i64 = 10_000;
const MAX_EXPORT_DATA_BYTES: i64 = 1_000_000;

#[derive(FromQueryResult)]
struct ExportStats {
    records: i64,
    history: i64,
    data_bytes: i64,
}
#[derive(Debug, Serialize, ToSchema)]
pub struct BackupDocument {
    pub format_version: u32,
    pub records: Vec<BackupRecord>,
    pub history: Vec<BackupHistory>,
}
#[derive(Debug, Serialize, Deserialize, ToSchema, FromQueryResult)]
pub struct BackupRecord {
    pub id: Uuid,
    pub record_type: String,
    pub source_id: String,
    pub data: serde_json::Value,
    pub archived_at: Option<String>,
    pub created_by: i64,
    pub updated_by: i64,
}
#[derive(Debug, Deserialize, ToSchema)]
pub struct BackupImport {
    pub format_version: u32,
    pub records: Vec<BackupRecord>,
    #[serde(default)]
    pub history: Vec<BackupHistory>,
}
#[derive(Debug, Serialize, Deserialize, ToSchema, FromQueryResult)]
pub struct BackupHistory {
    pub record_id: Uuid,
    pub action: String,
    pub source_actor_user_id: i64,
    pub before_data: Option<serde_json::Value>,
    pub after_data: Option<serde_json::Value>,
    pub created_at: String,
}

fn require_export_access(auth: &AuthUser) -> Result<()> {
    auth.require("backup:manage").map(|_| ())
}
fn no_store(response: &mut Response) {
    response.headers_mut().insert(
        "cache-control",
        "no-store, private".parse().expect("static header"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        "nosniff".parse().expect("static header"),
    );
}
fn payload_too_large() -> Error {
    Error::CustomError(
        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
        loco_rs::controller::ErrorDetail::new(
            "export_too_large",
            "Export exceeds the 10 MB download limit.",
        ),
    )
}
fn json_download(value: &impl Serialize) -> Result<Response> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InternalServerError)?;
    if bytes.len() > 10_000_000 {
        return Err(payload_too_large());
    }
    let mut response = Response::builder()
        .header("content-type", "application/json; charset=utf-8")
        .header(
            "content-disposition",
            "attachment; filename=brightcone-backup.json",
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|_| Error::InternalServerError)?;
    no_store(&mut response);
    Ok(response)
}
const NON_PEOPLE: &str = "record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title')";

#[utoipa::path(get, path = "/api/records/export.csv", security(("bearer" = [])), responses((status = 200, description = "CSV export")))]
#[debug_handler]
pub async fn csv(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Query(query): Query<super::records::ListQuery>,
) -> Result<Response> {
    require_export_access(&auth)?;
    let stats = sql::one::<ExportStats>(&ctx.db, &format!("SELECT count(*)::bigint AS records,0::bigint AS history,COALESCE(sum(pg_column_size(data)),0)::bigint AS data_bytes FROM hr_records WHERE {NON_PEOPLE} AND ($1 OR archived_at IS NULL)"), vec![query.include_archived.unwrap_or(false).into()]).await?;
    if stats.records > EXPORT_LIMIT || stats.data_bytes > MAX_EXPORT_DATA_BYTES {
        return Err(payload_too_large());
    }
    let rows = sql::all::<Record>(&ctx.db, &format!("SELECT id,record_type,source_id,data,archived_at::text AS archived_at,created_at::text AS created_at,updated_at::text AS updated_at FROM hr_records WHERE {NON_PEOPLE} AND ($1 OR archived_at IS NULL) ORDER BY record_type,created_at DESC LIMIT $2"), vec![query.include_archived.unwrap_or(false).into(), EXPORT_LIMIT.into()]).await?;
    let mut body =
        String::from("id,record_type,source_id,data,archived_at,created_at,updated_at\r\n");
    for row in rows {
        let values = [
            row.id.to_string(),
            row.record_type,
            row.source_id,
            row.data.to_string(),
            row.archived_at.unwrap_or_default(),
            row.created_at,
            row.updated_at,
        ];
        body.push_str(
            &values
                .iter()
                .map(|value| csv_cell(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        body.push_str("\r\n");
        if body.len() > 10_000_000 {
            return Err(payload_too_large());
        }
    }
    let mut response = Response::builder()
        .header("content-type", "text/csv; charset=utf-8")
        .header(
            "content-disposition",
            "attachment; filename=brightcone-records.csv",
        )
        .body(axum::body::Body::from(body))
        .map_err(|_| Error::string("Could not generate export."))?;
    no_store(&mut response);
    Ok(response)
}
fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[utoipa::path(get, path = "/api/backup", security(("bearer" = [])), responses((status = 200, body = BackupDocument)))]
#[debug_handler]
pub async fn backup(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    require_export_access(&auth)?;
    let stats = sql::one::<ExportStats>(&ctx.db, &format!("SELECT (SELECT count(*)::bigint FROM hr_records WHERE {NON_PEOPLE}) AS records,(SELECT count(*)::bigint FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.{NON_PEOPLE}) AS history,((SELECT COALESCE(sum(pg_column_size(data)),0) FROM hr_records WHERE {NON_PEOPLE})+(SELECT COALESCE(sum(COALESCE(pg_column_size(h.before_data),0)+COALESCE(pg_column_size(h.after_data),0)),0) FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.{NON_PEOPLE}))::bigint AS data_bytes"), vec![]).await?;
    if stats.records > EXPORT_LIMIT
        || stats.history > 100_000
        || stats.data_bytes > MAX_EXPORT_DATA_BYTES
    {
        return Err(payload_too_large());
    }
    let rows = sql::all::<BackupRecord>(&ctx.db, &format!("SELECT id,record_type,source_id,data,archived_at::text AS archived_at,COALESCE(created_by,0) AS created_by,COALESCE(updated_by,0) AS updated_by FROM hr_records WHERE {NON_PEOPLE} ORDER BY id LIMIT $1"), vec![EXPORT_LIMIT.into()]).await?;
    let history = sql::all::<BackupHistory>(&ctx.db, &format!("SELECT h.record_id,h.action,COALESCE(h.source_actor_user_id,h.actor_user_id) AS source_actor_user_id,h.before_data,h.after_data,h.created_at::text AS created_at FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.{NON_PEOPLE} ORDER BY h.id"), vec![]).await?;
    json_download(&BackupDocument {
        format_version: 2,
        records: rows,
        history,
    })
}

#[utoipa::path(post, path = "/api/backup/import", security(("bearer" = [])), request_body = BackupImport, responses((status = 200, description = "Import summary")))]
#[debug_handler]
pub async fn import(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Json(mut backup): Json<BackupImport>,
) -> Result<Response> {
    require_export_access(&auth)?;
    if !(1..=2).contains(&backup.format_version)
        || backup.records.len() > EXPORT_LIMIT as usize
        || backup.history.len() > 100_000
    {
        return Err(Error::BadRequest(
            "Unsupported backup version or record limit exceeded.".to_owned(),
        ));
    }
    for row in &mut backup.records {
        if row.record_type.starts_with("employee")
            || matches!(
                row.record_type.as_str(),
                "department" | "grade" | "location" | "job_title"
            )
        {
            return Err(Error::BadRequest(
                "People records must be imported through the People API.".to_owned(),
            ));
        }
        if row.source_id.trim().is_empty() || row.source_id.len() > 128 {
            return Err(Error::BadRequest(
                "Backup contains an invalid source_id.".to_owned(),
            ));
        }
        row.data = records::validate_record(&row.record_type, &row.data)
            .map_err(|_| Error::BadRequest("Backup contains an invalid record.".to_owned()))?;
    }
    let ids = backup
        .records
        .iter()
        .map(|row| row.id)
        .collect::<HashSet<_>>();
    for item in &backup.history {
        if !ids.contains(&item.record_id)
            || !matches!(
                item.action.as_str(),
                "created" | "updated" | "archived" | "restored"
            )
            || item
                .before_data
                .as_ref()
                .is_some_and(|data| !data.is_object())
            || item
                .after_data
                .as_ref()
                .is_some_and(|data| !data.is_object())
        {
            return Err(Error::BadRequest(
                "Backup contains invalid history.".to_owned(),
            ));
        }
    }
    let total = backup.records.len() as i64;
    let actor = auth.user.id;
    let imported = ctx.db.transaction::<_, i64, sea_orm::DbErr>(|tx| {
        Box::pin(async move {
            let mut count = 0_i64; let mut inserted = Vec::new();
            for row in backup.records {
                let rows = sql::execute(tx, "INSERT INTO hr_records(id,record_type,source_id,data,archived_at,created_by,updated_by) VALUES($1,$2,$3,$4,$5::timestamptz,$6,$6) ON CONFLICT DO NOTHING",
                    vec![row.id.into(), row.record_type.clone().into(), row.source_id.into(), row.data.clone().into(), row.archived_at.into(), actor.into()]).await?;
                if rows > 0 {
                    inserted.push((row.id, row.data.clone()));
                    for claim in records::unique_claims(&row.record_type, &row.data) {
                        sql::execute(tx, "INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES($1,$2,$3,$4)", vec![row.record_type.clone().into(), claim.field_key.into(), claim.normalized_value.into(), row.id.into()]).await?;
                    }
                    count += 1;
                }
            }
            let mut history_records = HashSet::new();
            for entry in backup.history {
                if inserted.iter().any(|(id, _)| *id == entry.record_id) {
                    sql::execute(tx, "INSERT INTO hr_record_history(record_id,action,actor_user_id,source_actor_user_id,before_data,after_data,created_at) VALUES($1,$2,$3,$4,$5,$6,$7::timestamptz)",
                        vec![entry.record_id.into(), entry.action.into(), actor.into(), entry.source_actor_user_id.into(), entry.before_data.into(), entry.after_data.into(), entry.created_at.into()]).await?;
                    history_records.insert(entry.record_id);
                }
            }
            for (id, data) in inserted {
                if !history_records.contains(&id) { sql::execute(tx, "INSERT INTO hr_record_history(record_id,action,actor_user_id,after_data) VALUES($1,'created',$2,$3)", vec![id.into(), actor.into(), data.into()]).await?; }
            }
            Ok(count)
        })
    }).await.map_err(|error| { tracing::error!(%error, "backup import transaction failed"); Error::string("Backup import failed") })?;
    let mut response = format::json(
        serde_json::json!({"imported": imported, "duplicates_skipped": total-imported}),
    )?;
    no_store(&mut response);
    Ok(response)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api")
        .add("/records/export.csv", get(csv))
        .add("/backup", get(backup))
        .add("/backup/import", post(import))
}
