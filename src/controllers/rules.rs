use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{BigInt, Bool, Text, Uuid as SqlUuid},
};
use diesel_async::RunQueryDsl;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::Database,
    request_meta::RequestMeta,
    workers::rule_dispatch::{RuleDispatchArgs, RuleDispatchWorker},
};

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct Rule {
    #[diesel(sql_type = Text)]
    pub rule_key: String,
    #[diesel(sql_type = Bool)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RuleUpdate {
    pub enabled: bool,
}

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct RuleMetrics {
    #[diesel(sql_type = BigInt)]
    pub pending: i64,
    #[diesel(sql_type = BigInt)]
    pub complete: i64,
    #[diesel(sql_type = BigInt)]
    pub failed: i64,
}

#[utoipa::path(get, path = "/api/rules", security(("bearer" = [])), responses((status = 200, body = [Rule])))]
#[debug_handler]
pub async fn list(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    auth.require("rules:manage")?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let rules = sql_query("SELECT rule_key, enabled FROM hr_rules ORDER BY rule_key")
        .load::<Rule>(&mut *conn)
        .await
        .map_err(Database::map_error)?;
    format::json(rules)
}

#[utoipa::path(put, path = "/api/rules/{rule_key}", security(("bearer" = [])), request_body = RuleUpdate, responses((status = 200, body = Rule)))]
#[debug_handler]
pub async fn update(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(key): Path<String>,
    Json(body): Json<RuleUpdate>,
) -> Result<Response> {
    auth.require("rules:manage")?;
    if key.len() > 64 {
        return Err(Error::BadRequest("Unknown rule.".to_owned()));
    }
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let mut rows = sql_query("UPDATE hr_rules SET enabled=$2, updated_by=$3, updated_at=now() WHERE rule_key=$1 RETURNING rule_key, enabled")
        .bind::<Text,_>(key).bind::<Bool,_>(body.enabled).bind::<diesel::sql_types::BigInt,_>(auth.user.id)
        .load::<Rule>(&mut *conn).await.map_err(Database::map_error)?;
    let Some(rule) = rows.pop() else {
        return Err(Error::NotFound);
    };
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new(if rule.enabled {
            "rule.enabled"
        } else {
            "rule.disabled"
        })
        .target("rule", rule.rule_key.clone())
        .summary(format!("Rule {}", rule.rule_key)),
    )
    .await?;
    format::json(rule)
}

#[utoipa::path(get, path = "/api/rules/metrics", security(("bearer" = [])), responses((status = 200, body = RuleMetrics)))]
#[debug_handler]
pub async fn metrics(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    auth.require("rules:manage")?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let counts = sql_query("SELECT count(*) FILTER (WHERE status='pending')::bigint AS pending, count(*) FILTER (WHERE status='complete')::bigint AS complete, count(*) FILTER (WHERE status='failed')::bigint AS failed FROM hr_rule_events")
        .get_result::<RuleMetrics>(&mut *conn).await.map_err(Database::map_error)?;
    format::json(counts)
}

#[utoipa::path(post, path = "/api/rules/events/{id}/retry", security(("bearer" = [])), responses((status = 202, description = "Retry queued")))]
#[debug_handler]
pub async fn retry(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    auth.require("rules:manage")?;
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let updated = sql_query("UPDATE hr_rule_events SET status='pending', attempts=0, last_error=NULL, completed_at=NULL WHERE event_id=$1 AND status='failed' RETURNING action_key")
        .bind::<SqlUuid,_>(id).get_result::<RuleKeyRow>(&mut *conn).await;
    let event = updated.map_err(|error| match error {
        diesel::result::Error::NotFound => Error::NotFound,
        other => Database::map_error(other),
    })?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("rule.retry")
            .target("rule_event", id)
            .summary(format!("Retry {}", event.action_key)),
    )
    .await?;
    RuleDispatchWorker::perform_later(&ctx, RuleDispatchArgs { event_id: id }).await?;
    format::render()
        .status(202)
        .json(serde_json::json!({"status":"queued"}))
}

#[derive(diesel::QueryableByName)]
struct RuleKeyRow {
    #[diesel(sql_type = Text)]
    action_key: String,
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/rules")
        .add("/", get(list))
        .add("/metrics", get(metrics))
        .add("/events/{id}/retry", post(retry))
        .add("/{rule_key}", put(update))
}
