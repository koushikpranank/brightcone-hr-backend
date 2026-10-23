//! Reading the audit log (ticket P1-09). HR only. Pages are keyed by id, not by offset, so the
//! log stays fast however long it grows.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{authz::AuthUser, models::_entities::audit_logs};

const DEFAULT_LIMIT: u64 = 50;
const MAX_LIMIT: u64 = 200;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Return entries older than this id (the `next_before_id` of the previous page).
    before_id: Option<i64>,
    limit: Option<u64>,
    /// For example `auth.` for everything about signing in.
    action_prefix: Option<String>,
    actor_user_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEntryResponse {
    pub id: i64,
    pub at: String,
    pub actor_email: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub summary: Option<String>,
    pub ip: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditPage {
    pub items: Vec<AuditEntryResponse>,
    /// Pass this as `before_id` to get the next page. Absent on the last page.
    pub next_before_id: Option<i64>,
}

/// Who did what and when, newest first.
#[utoipa::path(
    get,
    path = "/api/audit",
    security(("bearer" = [])),
    params(
        ("before_id" = Option<i64>, Query, description = "Page cursor"),
        ("limit" = Option<u64>, Query, description = "1 to 200, default 50"),
        ("action_prefix" = Option<String>, Query, description = "Filter, e.g. auth."),
        ("actor_user_id" = Option<i64>, Query, description = "Filter by who did it"),
    ),
    responses((status = 200, body = AuditPage), (status = 403, description = "HR Admin only"))
)]
#[debug_handler]
pub async fn list(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Query(q): Query<AuditQuery>,
) -> Result<Response> {
    auth.require("audit:read")?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let mut select = audit_logs::Entity::find().order_by_desc(audit_logs::Column::Id);
    if let Some(before) = q.before_id {
        select = select.filter(audit_logs::Column::Id.lt(before));
    }
    if let Some(prefix) = q
        .action_prefix
        .as_deref()
        .filter(|p| !p.is_empty() && p.len() <= 64)
    {
        select = select.filter(audit_logs::Column::Action.starts_with(prefix));
    }
    if let Some(actor) = q.actor_user_id {
        select = select.filter(audit_logs::Column::ActorUserId.eq(actor));
    }

    // One extra row tells us whether another page exists.
    let mut rows = select.limit(limit + 1).all(&ctx.db).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_before_id = if has_more {
        rows.last().map(|r| r.id)
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(|r| AuditEntryResponse {
            id: r.id,
            at: r.created_at.to_rfc3339(),
            actor_email: r.actor_email,
            action: r.action,
            target_type: r.target_type,
            target_id: r.target_id,
            summary: r.summary,
            ip: r.ip,
        })
        .collect();
    format::json(AuditPage {
        items,
        next_before_id,
    })
}

pub fn routes() -> Routes {
    Routes::new().prefix("/api/audit").add("/", get(list))
}
