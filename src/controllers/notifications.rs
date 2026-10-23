use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{authz::AuthUser, database::sql};

#[derive(Debug, Serialize, ToSchema, FromQueryResult)]
pub struct Notification {
    pub id: Uuid,
    pub rule_key: String,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    pub unread_count: i64,
}

#[derive(FromQueryResult)]
struct CountRow {
    count: i64,
}

#[utoipa::path(get, path = "/api/notifications", security(("bearer" = [])), responses((status = 200, body = NotificationPage)))]
#[debug_handler]
pub async fn list(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    let items = sql::all::<Notification>(
        &ctx.db,
        "SELECT id,rule_key,title,body,created_at::text AS created_at,read_at::text AS read_at FROM hr_notifications WHERE user_id=$1 ORDER BY created_at DESC LIMIT 50",
        vec![auth.user.id.into()],
    )
    .await?;
    let unread_count = sql::one::<CountRow>(
        &ctx.db,
        "SELECT count(*)::bigint AS count FROM hr_notifications WHERE user_id=$1 AND read_at IS NULL",
        vec![auth.user.id.into()],
    )
    .await?
    .count;
    format::json(NotificationPage {
        items,
        unread_count,
    })
}

#[utoipa::path(post, path = "/api/notifications/{id}/read", security(("bearer" = [])), responses((status = 200, body = Notification)))]
#[debug_handler]
pub async fn mark_read(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let notification = sql::optional::<Notification>(
        &ctx.db,
        "UPDATE hr_notifications SET read_at=COALESCE(read_at,now()) WHERE id=$1 AND user_id=$2 RETURNING id,rule_key,title,body,created_at::text AS created_at,read_at::text AS read_at",
        vec![id.into(), auth.user.id.into()],
    )
    .await?
    .ok_or(Error::NotFound)?;
    format::json(notification)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/notifications")
        .add("/", get(list))
        .add("/{id}/read", post(mark_read))
}
