use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::sql,
    request_meta::RequestMeta,
    workers::rule_dispatch::{RuleDispatchArgs, RuleDispatchWorker},
};

#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct Rule {
    pub rule_key: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RuleUpdate {
    pub enabled: bool,
}

#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct RuleMetrics {
    pub pending: i64,
    pub complete: i64,
    pub failed: i64,
}

#[derive(FromQueryResult)]
struct RuleKeyRow {
    action_key: String,
}

#[utoipa::path(get, path = "/api/rules", security(("bearer" = [])), responses((status = 200, body = [Rule])))]
#[debug_handler]
pub async fn list(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    auth.require("rules:manage")?;
    let rules = sql::all::<Rule>(
        &ctx.db,
        "SELECT rule_key, enabled FROM hr_rules ORDER BY rule_key",
        Vec::new(),
    )
    .await?;
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
    let rule = sql::optional::<Rule>(
        &ctx.db,
        "UPDATE hr_rules SET enabled=$2, updated_by=$3, updated_at=now() WHERE rule_key=$1 RETURNING rule_key, enabled",
        vec![key.into(), body.enabled.into(), auth.user.id.into()],
    )
    .await?
    .ok_or(Error::NotFound)?;
    audit::record(
        &ctx.db,
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
    let counts = sql::one::<RuleMetrics>(
        &ctx.db,
        "SELECT count(*) FILTER (WHERE status='pending')::bigint AS pending, count(*) FILTER (WHERE status='complete')::bigint AS complete, count(*) FILTER (WHERE status='failed')::bigint AS failed FROM hr_rule_events",
        Vec::new(),
    )
    .await?;
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
    let event = sql::optional::<RuleKeyRow>(
        &ctx.db,
        "UPDATE hr_rule_events SET status='pending', attempts=0, last_error=NULL, completed_at=NULL WHERE event_id=$1 AND status='failed' RETURNING action_key",
        vec![id.into()],
    )
    .await?
    .ok_or(Error::NotFound)?;
    audit::record(
        &ctx.db,
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

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/rules")
        .add("/", get(list))
        .add("/metrics", get(metrics))
        .add("/events/{id}/retry", post(retry))
        .add("/{rule_key}", put(update))
}
