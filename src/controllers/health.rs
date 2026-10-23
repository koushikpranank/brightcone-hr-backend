use loco_rs::prelude::*;

use crate::views::health::HealthResponse;

/// Liveness plus a database check. Used by monitoring and the frontend start page.
/// Returns 503 when the database does not answer so load balancers stop sending traffic.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Server and database are up", body = HealthResponse),
        (status = 503, description = "Database is not answering", body = HealthResponse),
    )
)]
#[debug_handler]
pub async fn health(State(ctx): State<AppContext>) -> Result<Response> {
    let database_up = ctx.db.execute_unprepared("SELECT 1").await.is_ok();
    let status = if database_up { 200 } else { 503 };
    format::render()
        .status(status)
        .json(HealthResponse::new(database_up))
}

pub fn routes() -> Routes {
    Routes::new().prefix("/api/health").add("/", get(health))
}
