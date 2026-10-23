//! The signed-in user's own profile and password (ticket P1-07).
use loco_rs::{hash, prelude::*};

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    models::{refresh_tokens, users::check_password_strength},
    request_meta::RequestMeta,
    session::session_response,
    settings::AppSettings,
    views::auth::{ChangePasswordParams, MeResponse},
};

/// Who am I and what may I do? Called by the frontend after sign-in and after a page reload.
#[utoipa::path(
    get,
    path = "/api/me",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "The signed-in user", body = MeResponse),
        (status = 401, description = "Not signed in"),
    )
)]
#[debug_handler]
pub async fn current(auth: AuthUser, State(_ctx): State<AppContext>) -> Result<Response> {
    // Every signed-in user may read their own profile, even while a password change is pending.
    format::json(MeResponse::new(&auth.user))
}

/// Changes the password and signs out every other device. Returns a fresh session for this one.
#[utoipa::path(
    put,
    path = "/api/me/password",
    security(("bearer" = [])),
    request_body = ChangePasswordParams,
    responses(
        (status = 200, description = "Password changed", body = crate::views::auth::SessionResponse),
        (status = 400, description = "Current password wrong, or new password too weak"),
        (status = 401, description = "Not signed in"),
    )
)]
#[debug_handler]
pub async fn change_password(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
    Json(params): Json<ChangePasswordParams>,
) -> Result<Response> {
    let settings = AppSettings::from_ctx(&ctx);
    let hash_value = auth.user.password.clone();
    let current = params.current_password.clone();
    let correct = tokio::task::spawn_blocking(move || hash::verify_password(&current, &hash_value))
        .await
        .unwrap_or(false);
    if !correct {
        return bad_request("Your current password is not correct.");
    }
    if let Err(message) = check_password_strength(&params.new_password) {
        return bad_request(message);
    }
    if params.new_password == params.current_password {
        return bad_request("Choose a password different from the current one.");
    }

    let user = auth
        .user
        .into_active_model()
        .set_password(&ctx.db, &params.new_password, false)
        .await?;
    refresh_tokens::Model::revoke_all_for_user(&ctx.db, user.id).await?;
    let issued =
        refresh_tokens::Model::issue(&ctx.db, &settings, user.id, None, false, meta.ip.clone())
            .await?;
    audit::record(
        &ctx.db,
        Some(&user),
        &meta,
        Entry::new("user.password_changed").target("user", user.pid),
    )
    .await?;
    session_response(&ctx, &settings, &user, &issued)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/me")
        .add("/", get(current))
        .add("/password", put(change_password))
}
