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
    authz::AuthUser,
    database::Database,
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

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct Record {
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
    #[diesel(sql_type = Text)]
    pub created_at: String,
    #[diesel(sql_type = Text)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecordPage {
    pub items: Vec<Record>,
}

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct HistoryEntry {
    #[diesel(sql_type = BigInt)]
    pub id: i64,
    #[diesel(sql_type = SqlUuid)]
    pub record_id: Uuid,
    #[diesel(sql_type = Text)]
    pub action: String,
    #[diesel(sql_type = BigInt)]
    pub actor_user_id: i64,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub before_data: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub after_data: Option<serde_json::Value>,
    #[diesel(sql_type = Text)]
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub include_archived: Option<bool>,
}

fn require_hr(auth: &AuthUser) -> Result<()> {
    auth.require("records:manage").map(|_| ())
}

fn validate_type(record_type: &str) -> Result<()> {
    if records::is_registered(record_type) && !is_people_record(record_type) {
        Ok(())
    } else {
        Err(Error::BadRequest("Unknown record type.".to_owned()))
    }
}

fn is_people_record(kind: &str) -> bool {
    kind.starts_with("employee")
        || matches!(kind, "department" | "grade" | "location" | "job_title")
}

fn validate_input(record_type: &str, input: &RecordInput) -> Result<serde_json::Value> {
    if input.source_id.trim().is_empty() || input.source_id.len() > 128 {
        return Err(Error::BadRequest(
            "source_id must be 1 to 128 characters.".to_owned(),
        ));
    }
    records::validate_record(record_type, &input.data).map_err(Error::BadRequest)
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
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let items = sql_query("SELECT id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at FROM hr_records WHERE record_type = $1 AND ($2 OR archived_at IS NULL) ORDER BY created_at DESC LIMIT $3")
        .bind::<Text, _>(&kind).bind::<diesel::sql_types::Bool, _>(query.include_archived.unwrap_or(false))
        .bind::<BigInt, _>(limit).load::<Record>(&mut *conn).await.map_err(Database::map_error)?;
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
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let actor = auth.user.id;
    let (row, event_id) = conn.transaction::<(Record, Option<Uuid>), ModelError, _>(async move |conn| {
        let row = sql_query("INSERT INTO hr_records(record_type, source_id, data, created_by, updated_by) VALUES ($1,$2,$3,$4,$4) RETURNING id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at")
            .bind::<Text, _>(&kind).bind::<Text, _>(&source_id).bind::<Jsonb, _>(&data).bind::<BigInt, _>(actor)
            .get_result::<Record>(&mut *conn).await?;
        sql_query("INSERT INTO hr_record_history(record_id, action, actor_user_id, after_data) VALUES ($1,'created',$2,$3)")
            .bind::<SqlUuid, _>(row.id).bind::<BigInt, _>(actor).bind::<Jsonb, _>(&data)
            .execute(&mut *conn).await?;
        for claim in records::unique_claims(&kind, &data) {
            sql_query("INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES ($1,$2,$3,$4)")
                .bind::<Text,_>(&kind).bind::<Text,_>(&claim.field_key).bind::<Text,_>(&claim.normalized_value)
                .bind::<SqlUuid,_>(row.id).execute(&mut *conn).await?;
        }
        let event_id = if let Some(key) = rule_engine::event_key(&kind, "created", None, &data) {
            rule_engine::record_event(conn, key, &RuleEventPayload {
                record_id: row.id,
                record_type: kind.clone(),
                actor_user_id: actor,
            })
            .await?
        } else {
            None
        };
        Ok((row, event_id))
    }).await.map_err(record_model_error)?;
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
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let actor = auth.user.id;
    let (row, event_id) = conn.transaction::<(Record, Option<Uuid>), ModelError, _>(async move |conn| {
        let old = sql_query("SELECT id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at FROM hr_records WHERE id=$1 FOR UPDATE")
            .bind::<SqlUuid, _>(id).get_result::<Record>(&mut *conn).await?;
        if is_people_record(&old.record_type) {
            return Err(ModelError::Validation("People records require the typed People API".to_owned()));
        }
        let new_data = match input {
            Some(value) => records::validate_record(&old.record_type, &value)
                .map_err(ModelError::Validation)?,
            None => old.data.clone(),
        };
        if action == "updated" {
            sql_query("DELETE FROM hr_record_unique_claims WHERE record_id=$1")
                .bind::<SqlUuid,_>(id).execute(&mut *conn).await?;
        }
        let updated = match action {
            "updated" => sql_query("UPDATE hr_records SET data=$2, updated_by=$3, updated_at=now() WHERE id=$1 RETURNING id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at").bind::<SqlUuid,_>(id).bind::<Jsonb,_>(&new_data).bind::<BigInt,_>(actor).get_result::<Record>(&mut *conn).await?,
            "archived" => sql_query("UPDATE hr_records SET archived_at=now(), updated_by=$2, updated_at=now() WHERE id=$1 AND archived_at IS NULL RETURNING id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at").bind::<SqlUuid,_>(id).bind::<BigInt,_>(actor).get_result::<Record>(&mut *conn).await?,
            _ => sql_query("UPDATE hr_records SET archived_at=NULL, updated_by=$2, updated_at=now() WHERE id=$1 AND archived_at IS NOT NULL RETURNING id, record_type, source_id, data, archived_at::text AS archived_at, created_at::text AS created_at, updated_at::text AS updated_at").bind::<SqlUuid,_>(id).bind::<BigInt,_>(actor).get_result::<Record>(&mut *conn).await?,
        };
        if action == "updated" {
            for claim in records::unique_claims(&old.record_type, &new_data) {
                sql_query("INSERT INTO hr_record_unique_claims(record_type,field_key,normalized_value,record_id) VALUES ($1,$2,$3,$4)")
                    .bind::<Text,_>(&old.record_type).bind::<Text,_>(&claim.field_key).bind::<Text,_>(&claim.normalized_value)
                    .bind::<SqlUuid,_>(id).execute(&mut *conn).await?;
            }
        }
        sql_query("INSERT INTO hr_record_history(record_id, action, actor_user_id, before_data, after_data) VALUES ($1,$2,$3,$4,$5)")
            .bind::<SqlUuid,_>(id).bind::<Text,_>(action).bind::<BigInt,_>(actor).bind::<Jsonb,_>(&old.data).bind::<Jsonb,_>(&new_data)
            .execute(&mut *conn).await?;
        let event_id = if let Some(key) = rule_engine::event_key(
            &old.record_type,
            action,
            Some(&old.data),
            &new_data,
        ) {
            rule_engine::record_event(conn, key, &RuleEventPayload {
            record_id: updated.id,
            record_type: old.record_type.clone(),
            actor_user_id: actor,
            })
            .await?
        } else {
            None
        };
        Ok((updated, event_id))
    }).await.map_err(record_model_error)?;
    rule_engine::dispatch(&ctx, event_id).await;
    format::json(row)
}

#[utoipa::path(put, path = "/api/records/{id}", security(("bearer" = [])), request_body = RecordUpdateInput, responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn update(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
    Json(input): Json<RecordUpdateInput>,
) -> Result<Response> {
    change(auth, ctx, id, Some(input.data), "updated").await
}

#[utoipa::path(post, path = "/api/records/{id}/archive", security(("bearer" = [])), responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn archive(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    change(auth, ctx, id, None, "archived").await
}

#[utoipa::path(post, path = "/api/records/{id}/restore", security(("bearer" = [])), responses((status = 200, body = Record)))]
#[debug_handler]
pub async fn restore(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    change(auth, ctx, id, None, "restored").await
}

#[utoipa::path(get, path = "/api/records/{id}/history", security(("bearer" = [])), responses((status = 200, body = [HistoryEntry])))]
#[debug_handler]
pub async fn history(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    require_hr(&auth)?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let record = sql_query("SELECT record_type, ''::text AS source_id, '{}'::jsonb AS data, NULL::text AS archived_at, ''::text AS created_at, ''::text AS updated_at, id FROM hr_records WHERE id=$1")
        .bind::<SqlUuid,_>(id).get_result::<Record>(&mut *conn).await.map_err(Database::map_error)?;
    if is_people_record(&record.record_type) {
        return Err(Error::NotFound);
    }
    let rows = sql_query("SELECT id, record_id, action, actor_user_id, before_data, after_data, created_at::text AS created_at FROM hr_record_history WHERE record_id=$1 ORDER BY id DESC LIMIT 500")
        .bind::<SqlUuid,_>(id).load::<HistoryEntry>(&mut *conn).await.map_err(Database::map_error)?;
    format::json(rows)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/records")
        .add("/{record_type}", get(list).post(create))
        .add("/{id}", put(update))
        .add("/{id}/archive", post(archive))
        .add("/{id}/restore", post(restore))
        .add("/{id}/history", get(history))
}
