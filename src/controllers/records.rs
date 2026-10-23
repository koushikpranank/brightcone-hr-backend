use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    authz::AuthUser,
    database::sql,
    models::error::ModelError,
    records,
    rule_engine::{self, RuleEventPayload},
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordInput {
    pub source_id: String,
    pub data: serde_json::Value,
}
#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordUpdateInput {
    pub data: serde_json::Value,
}
#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct Record {
    pub id: Uuid,
    pub record_type: String,
    pub source_id: String,
    pub data: serde_json::Value,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Serialize, ToSchema)]
pub struct RecordPage {
    pub items: Vec<Record>,
}
#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct HistoryEntry {
    pub id: i64,
    pub record_id: Uuid,
    pub action: String,
    pub actor_user_id: i64,
    pub before_data: Option<serde_json::Value>,
    pub after_data: Option<serde_json::Value>,
    pub created_at: String,
}
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub include_archived: Option<bool>,
}

const RECORD_COLUMNS: &str = "id,record_type,source_id,data,archived_at::text AS archived_at,created_at::text AS created_at,updated_at::text AS updated_at";

fn require_hr(auth: &AuthUser) -> Result<()> {
    auth.require("records:manage").map(|_| ())
}
fn validate_type(kind: &str) -> Result<()> {
    if records::is_registered(kind) && !is_people_record(kind) {
        Ok(())
    } else {
        Err(Error::BadRequest("Unknown record type.".to_owned()))
    }
}
fn is_people_record(kind: &str) -> bool {
    kind.starts_with("employee")
        || matches!(kind, "department" | "grade" | "location" | "job_title")
}
fn validate_input(kind: &str, input: &RecordInput) -> Result<serde_json::Value> {
    if input.source_id.trim().is_empty() || input.source_id.len() > 128 {
        return Err(Error::BadRequest(
            "source_id must be 1 to 128 characters.".to_owned(),
        ));
    }
    records::validate_record(kind, &input.data).map_err(Error::BadRequest)
}
fn record_model_error(error: ModelError) -> Error {
    match error {
        ModelError::EntityAlreadyExists => Error::CustomError(
            axum::http::StatusCode::CONFLICT,
            loco_rs::controller::ErrorDetail::new("conflict", "Record source_id already exists."),
        ),
        other => Error::from(other),
    }
}
fn map_transaction_error(error: sea_orm::TransactionError<ModelError>) -> Error {
    match error {
        sea_orm::TransactionError::Transaction(error) => record_model_error(error),
        sea_orm::TransactionError::Connection(error) => {
            tracing::error!(?error, "record transaction connection failed");
            Error::InternalServerError
        }
    }
}

#[utoipa::path(get, path = "/api/records/{record_type}", security(("bearer" = [])), params(("record_type" = String, Path), ("limit" = Option<i64>, Query), ("include_archived" = Option<bool>, Query)), responses((status = 200, body = RecordPage)))]
#[debug_handler]
pub async fn list(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(kind): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Response> {
    require_hr(&auth)?;
    validate_type(&kind)?;
    let items = sql::all::<Record>(&ctx.db,
        &format!("SELECT {RECORD_COLUMNS} FROM hr_records WHERE record_type=$1 AND ($2 OR archived_at IS NULL) ORDER BY created_at DESC LIMIT $3"),
        vec![kind.into(), query.include_archived.unwrap_or(false).into(), query.limit.unwrap_or(100).clamp(1, 500).into()]).await?;
    format::json(RecordPage { items })
}

#[utoipa::path(post, path = "/api/records/{record_type}", security(("bearer" = [])), request_body = RecordInput, responses((status = 201, body = Record)))]
#[debug_handler]
pub async fn create(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(kind): Path<String>,
    Json(input): Json<RecordInput>,
) -> Result<Response> {
    require_hr(&auth)?;
    validate_type(&kind)?;
    let data = validate_input(&kind, &input)?;
    let source_id = input.source_id;
    let actor = auth.user.id;
    let (row, event_id) = ctx.db.transaction::<_, (Record, Option<Uuid>), ModelError>(|tx| {
        Box::pin(async move {
            let row = sql::one::<Record>(tx, &format!("INSERT INTO hr_records(record_type,source_id,data,created_by,updated_by) VALUES($1,$2,$3,$4,$4) RETURNING {RECORD_COLUMNS}"),
                vec![kind.clone().into(), source_id.into(), data.clone().into(), actor.into()]).await?;
            sql::execute(tx, "INSERT INTO hr_record_history(record_id,action,actor_user_id,after_data) VALUES($1,'created',$2,$3)",
                vec![row.id.into(), actor.into(), data.clone().into()]).await?;
            for claim in records::unique_claims(&kind, &data) {
                sql::execute(tx, "INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES($1,$2,$3,$4)",
                    vec![kind.clone().into(), claim.field_key.into(), claim.normalized_value.into(), row.id.into()]).await?;
            }
            let event_id = if let Some(key) = rule_engine::event_key(&kind, "created", None, &data) {
                rule_engine::record_event(tx, key, &RuleEventPayload { record_id: row.id, record_type: kind, actor_user_id: actor }).await?
            } else { None };
            Ok((row, event_id))
        })
    }).await.map_err(map_transaction_error)?;
    rule_engine::dispatch(&ctx, event_id).await;
    format::render().status(201).json(row)
}

async fn change(
    auth: AuthUser,
    ctx: AppContext,
    id: Uuid,
    input: Option<serde_json::Value>,
    action: &'static str,
) -> Result<Response> {
    require_hr(&auth)?;
    let actor = auth.user.id;
    let (updated, event_id) = ctx.db.transaction::<_, (Record, Option<Uuid>), ModelError>(|tx| {
        Box::pin(async move {
            let old = sql::one::<Record>(tx, &format!("SELECT {RECORD_COLUMNS} FROM hr_records WHERE id=$1 FOR UPDATE"), vec![id.into()]).await?;
            if is_people_record(&old.record_type) { return Err(ModelError::Validation("People records require the typed People API".to_owned())); }
            let new_data = match input { Some(value) => records::validate_record(&old.record_type, &value).map_err(ModelError::Validation)?, None => old.data.clone() };
            if action == "updated" { sql::execute(tx, "DELETE FROM hr_record_unique_claims WHERE record_id=$1", vec![id.into()]).await?; }
            let statement = match action {
                "updated" => format!("UPDATE hr_records SET data=$2,updated_by=$3,updated_at=now() WHERE id=$1 RETURNING {RECORD_COLUMNS}"),
                "archived" => format!("UPDATE hr_records SET archived_at=now(),updated_by=$2,updated_at=now() WHERE id=$1 AND archived_at IS NULL RETURNING {RECORD_COLUMNS}"),
                _ => format!("UPDATE hr_records SET archived_at=NULL,updated_by=$2,updated_at=now() WHERE id=$1 AND archived_at IS NOT NULL RETURNING {RECORD_COLUMNS}"),
            };
            let values = if action == "updated" { vec![id.into(), new_data.clone().into(), actor.into()] } else { vec![id.into(), actor.into()] };
            let updated = sql::one::<Record>(tx, &statement, values).await?;
            if action == "updated" {
                for claim in records::unique_claims(&old.record_type, &new_data) {
                    sql::execute(tx, "INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES($1,$2,$3,$4)",
                        vec![old.record_type.clone().into(), claim.field_key.into(), claim.normalized_value.into(), id.into()]).await?;
                }
            }
            sql::execute(tx, "INSERT INTO hr_record_history(record_id,action,actor_user_id,before_data,after_data) VALUES($1,$2,$3,$4,$5)",
                vec![id.into(), action.into(), actor.into(), old.data.clone().into(), new_data.clone().into()]).await?;
            let event_id = if let Some(key) = rule_engine::event_key(&old.record_type, action, Some(&old.data), &new_data) {
                rule_engine::record_event(tx, key, &RuleEventPayload { record_id: id, record_type: old.record_type, actor_user_id: actor }).await?
            } else { None };
            Ok((updated, event_id))
        })
    }).await.map_err(map_transaction_error)?;
    rule_engine::dispatch(&ctx, event_id).await;
    format::json(updated)
}

#[utoipa::path(put, path = "/api/records/id/{id}", security(("bearer" = [])), request_body = RecordUpdateInput, responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn update(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
    Json(input): Json<RecordUpdateInput>,
) -> Result<Response> {
    change(auth, ctx, id, Some(input.data), "updated").await
}
#[utoipa::path(post, path = "/api/records/id/{id}/archive", security(("bearer" = [])), responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn archive(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    change(auth, ctx, id, None, "archived").await
}
#[utoipa::path(post, path = "/api/records/id/{id}/restore", security(("bearer" = [])), responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn restore(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    change(auth, ctx, id, None, "restored").await
}

#[derive(FromQueryResult)]
struct RecordTypeRow {
    record_type: String,
}
#[utoipa::path(get, path = "/api/records/id/{id}/history", security(("bearer" = [])), responses((status = 200, body = [HistoryEntry])))]
#[debug_handler]
pub async fn history(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    require_hr(&auth)?;
    let record = sql::one::<RecordTypeRow>(
        &ctx.db,
        "SELECT record_type FROM hr_records WHERE id=$1",
        vec![id.into()],
    )
    .await?;
    if is_people_record(&record.record_type) {
        return Err(Error::NotFound);
    }
    let rows = sql::all::<HistoryEntry>(&ctx.db, "SELECT id,record_id,action,actor_user_id,before_data,after_data,created_at::text AS created_at FROM hr_record_history WHERE record_id=$1 ORDER BY id DESC LIMIT 500", vec![id.into()]).await?;
    format::json(rows)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/records")
        .add("/{record_type}", get(list).post(create))
        .add("/id/{id}", put(update))
        .add("/id/{id}/archive", post(archive))
        .add("/id/{id}/restore", post(restore))
        .add("/id/{id}/history", get(history))
}
