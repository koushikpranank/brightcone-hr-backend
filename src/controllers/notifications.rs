use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::RunQueryDsl;
use loco_rs::prelude::*;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{authz::AuthUser, database::Database};

#[derive(Debug, Serialize, ToSchema, QueryableByName)]
pub struct Notification {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub rule_key: String,
    #[diesel(sql_type = Text)]
    pub title: String,
    #[diesel(sql_type = Text)]
    pub body: String,
    #[diesel(sql_type = Text)]
    pub created_at: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub read_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    pub unread_count: i64,
}

#[utoipa::path(get, path = "/api/notifications", security(("bearer" = [])), responses((status = 200, body = NotificationPage)))]
#[debug_handler]
pub async fn list(auth: AuthUser, State(ctx): State<AppContext>) -> Result<Response> {
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let items = sql_query("SELECT id,rule_key,title,body,created_at::text AS created_at,read_at::text AS read_at FROM hr_notifications WHERE user_id=$1 ORDER BY created_at DESC LIMIT 50")
        .bind::<BigInt,_>(auth.user.id).load::<Notification>(&mut *conn).await.map_err(Database::map_error)?;
    let unread_count = sql_query("SELECT count(*)::bigint AS count FROM hr_notifications WHERE user_id=$1 AND read_at IS NULL")
        .bind::<BigInt,_>(auth.user.id).get_result::<CountRow>(&mut *conn).await.map_err(Database::map_error)?.count;
    format::json(NotificationPage {
        items,
        unread_count,
    })
}

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[utoipa::path(post, path = "/api/notifications/{id}/read", security(("bearer" = [])), responses((status = 200, body = Notification)))]
#[debug_handler]
pub async fn mark_read(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let notification = sql_query("UPDATE hr_notifications SET read_at=COALESCE(read_at,now()) WHERE id=$1 AND user_id=$2 RETURNING id,rule_key,title,body,created_at::text AS created_at,read_at::text AS read_at")
        .bind::<SqlUuid,_>(id).bind::<BigInt,_>(auth.user.id)
        .get_result::<Notification>(&mut *conn).await.map_err(|error| match error {
            diesel::result::Error::NotFound => Error::NotFound,
            other => Database::map_error(other),
        })?;
    format::json(notification)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/notifications")
        .add("/", get(list))
        .add("/{id}/read", post(mark_read))
}
