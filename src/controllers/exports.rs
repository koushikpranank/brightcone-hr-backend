use std::collections::HashSet;

use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{BigInt, Jsonb, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    authz::AuthUser, controllers::records::Record, database::Database, models::error::ModelError,
    records,
};

const EXPORT_LIMIT: i64 = 10_000;
const MAX_EXPORT_DATA_BYTES: i64 = 1_000_000;

#[derive(QueryableByName)]
struct ExportStats {
    #[diesel(sql_type = BigInt)]
    records: i64,
    #[diesel(sql_type = BigInt)]
    history: i64,
    #[diesel(sql_type = BigInt)]
    data_bytes: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BackupDocument {
    pub format_version: u32,
    pub records: Vec<BackupRecord>,
    pub history: Vec<BackupHistory>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, QueryableByName)]
pub struct BackupRecord {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub record_type: String,
    #[diesel(sql_type = Text)]
    pub source_id: String,
    #[diesel(sql_type = Jsonb)]
    pub data: serde_json::Value,
    #[diesel(sql_type = Nullable<Text>)]
    pub archived_at: Option<String>,
    #[diesel(sql_type = BigInt)]
    pub created_by: i64,
    #[diesel(sql_type = BigInt)]
    pub updated_by: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BackupImport {
    pub format_version: u32,
    pub records: Vec<BackupRecord>,
    #[serde(default)]
    pub history: Vec<BackupHistory>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, QueryableByName)]
pub struct BackupHistory {
    #[diesel(sql_type = SqlUuid)]
    pub record_id: Uuid,
    #[diesel(sql_type = Text)]
    pub action: String,
    #[diesel(sql_type = BigInt)]
    pub source_actor_user_id: i64,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub before_data: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub after_data: Option<serde_json::Value>,
    #[diesel(sql_type = Text)]
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

#[utoipa::path(get, path = "/api/records/export.csv", security(("bearer" = [])), responses((status = 200, description = "CSV export")))]
#[debug_handler]
pub async fn csv(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Query(query): Query<super::records::ListQuery>,
) -> Result<Response> {
    require_export_access(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let stats = sql_query("SELECT count(*)::bigint AS records, 0::bigint AS history, COALESCE(sum(pg_column_size(data)),0)::bigint AS data_bytes FROM hr_records WHERE record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title') AND ($1 OR archived_at IS NULL)")
        .bind::<diesel::sql_types::Bool,_>(query.include_archived.unwrap_or(false))
        .get_result::<ExportStats>(&mut *conn).await.map_err(Database::map_error)?;
    if stats.records > EXPORT_LIMIT || stats.data_bytes > MAX_EXPORT_DATA_BYTES {
        return Err(payload_too_large());
    }
    let rows = sql_query("SELECT id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at FROM hr_records WHERE record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title') AND ($1 OR archived_at IS NULL) ORDER BY record_type, created_at DESC LIMIT $2")
        .bind::<diesel::sql_types::Bool,_>(query.include_archived.unwrap_or(false))
        .bind::<BigInt,_>(EXPORT_LIMIT).load::<Record>(&mut *conn).await.map_err(Database::map_error)?;
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
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let stats = sql_query("SELECT (SELECT count(*)::bigint FROM hr_records WHERE record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title')) AS records, (SELECT count(*)::bigint FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.record_type NOT LIKE 'employee%' AND r.record_type NOT IN ('department','grade','location','job_title')) AS history, ((SELECT COALESCE(sum(pg_column_size(data)),0) FROM hr_records WHERE record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title')) + (SELECT COALESCE(sum(COALESCE(pg_column_size(h.before_data),0)+COALESCE(pg_column_size(h.after_data),0)),0) FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.record_type NOT LIKE 'employee%' AND r.record_type NOT IN ('department','grade','location','job_title')))::bigint AS data_bytes")
        .get_result::<ExportStats>(&mut *conn).await.map_err(Database::map_error)?;
    if stats.records > EXPORT_LIMIT
        || stats.history > 100_000
        || stats.data_bytes > MAX_EXPORT_DATA_BYTES
    {
        return Err(payload_too_large());
    }
    let rows = sql_query("SELECT id, record_type, source_id, data, archived_at::text AS archived_at, COALESCE(created_by,0) AS created_by, COALESCE(updated_by,0) AS updated_by FROM hr_records WHERE record_type NOT LIKE 'employee%' AND record_type NOT IN ('department','grade','location','job_title') ORDER BY id LIMIT $1")
        .bind::<BigInt,_>(EXPORT_LIMIT).load::<BackupRecord>(&mut *conn).await.map_err(Database::map_error)?;
    let history = sql_query("SELECT h.record_id, h.action, COALESCE(h.source_actor_user_id, h.actor_user_id) AS source_actor_user_id, h.before_data, h.after_data, h.created_at::text AS created_at FROM hr_record_history h JOIN hr_records r ON r.id=h.record_id WHERE r.record_type NOT LIKE 'employee%' AND r.record_type NOT IN ('department','grade','location','job_title') ORDER BY h.id")
        .load::<BackupHistory>(&mut *conn).await.map_err(Database::map_error)?;
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
        || backup.records.len() > usize::try_from(EXPORT_LIMIT).unwrap_or(10_000)
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
    let record_ids = backup
        .records
        .iter()
        .map(|row| row.id)
        .collect::<HashSet<_>>();
    for item in &backup.history {
        if !record_ids.contains(&item.record_id)
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
    let actor = auth.user.id;
    let total = backup.records.len() as i64;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let imported = conn.transaction::<i64, ModelError, _>(async move |conn| {
        let mut count = 0_i64;
        let mut inserted_records = Vec::new();
        for row in backup.records {
            let inserted = sql_query("INSERT INTO hr_records(id, record_type, source_id, data, archived_at, created_by, updated_by) VALUES ($1,$2,$3,$4,$5::timestamptz,$6,$6) ON CONFLICT DO NOTHING")
                .bind::<SqlUuid,_>(row.id).bind::<Text,_>(&row.record_type).bind::<Text,_>(&row.source_id)
                .bind::<Jsonb,_>(&row.data).bind::<Nullable<Text>,_>(row.archived_at)
                .bind::<BigInt,_>(actor)
                .execute(&mut *conn).await?;
            if inserted > 0 {
                inserted_records.push((row.id, row.data.clone()));
                for claim in records::unique_claims(&row.record_type, &row.data) {
                    sql_query("INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES ($1,$2,$3,$4)")
                        .bind::<Text,_>(&row.record_type).bind::<Text,_>(&claim.field_key).bind::<Text,_>(&claim.normalized_value)
                        .bind::<SqlUuid,_>(row.id).execute(&mut *conn).await?;
                }
                count += 1;
            }
        }
        let mut history_records = HashSet::new();
        for entry in backup.history {
            if inserted_records.iter().any(|(id, _)| *id == entry.record_id) {
                sql_query("INSERT INTO hr_record_history(record_id, action, actor_user_id, source_actor_user_id, before_data, after_data, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7::timestamptz)")
                    .bind::<SqlUuid,_>(entry.record_id).bind::<Text,_>(&entry.action).bind::<BigInt,_>(actor)
                    .bind::<BigInt,_>(entry.source_actor_user_id).bind::<Nullable<Jsonb>,_>(entry.before_data)
                    .bind::<Nullable<Jsonb>,_>(entry.after_data).bind::<Text,_>(&entry.created_at)
                    .execute(&mut *conn).await?;
                history_records.insert(entry.record_id);
            }
        }
        for (id, data) in inserted_records {
            if !history_records.contains(&id) {
                sql_query("INSERT INTO hr_record_history(record_id, action, actor_user_id, after_data) VALUES ($1,'created',$2,$3)")
                    .bind::<SqlUuid,_>(id).bind::<BigInt,_>(actor).bind::<Jsonb,_>(&data)
                    .execute(&mut *conn).await?;
            }
        }
        Ok(count)
    }).await.map_err(Error::from)?;
    let mut response = format::json(
        serde_json::json!({"imported": imported, "duplicates_skipped": total - imported}),
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
