use brightcone_hr_backend::{app::App, authz::Role, models::users};
use loco_rs::testing::prelude::*;
use serde_json::{json, Value};
use serial_test::serial;

use super::support::*;

#[tokio::test]
#[serial]
async fn me_requires_a_token() {
    request::<App, _, _>(|request, _ctx| async move {
        assert_eq!(request.get("/api/me").await.status_code(), 401);
        assert_eq!(
            get_as(&request, "/api/me", "not.a.token")
                .await
                .status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn me_returns_role_and_permissions_but_no_secrets() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Manager).await;
        let (token, _) = sign_in(&request, "a@brightcone.ai").await;

        let response = get_as(&request, "/api/me", &token).await;
        assert_eq!(response.status_code(), 200);
        let body: Value = response.json();
        assert_eq!(body["role"], "manager");
        assert_eq!(body["email"], "a@brightcone.ai");
        let permissions = body["permissions"].as_array().unwrap();
        assert!(permissions.contains(&json!("approvals:decide:team")));
        assert!(!permissions.contains(&json!("audit:read:all")));
        let text = response.text();
        assert!(!text.contains("password") && !text.contains("api_key"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_checks_the_current_password_and_the_rules() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (token, _) = sign_in(&request, "a@brightcone.ai").await;
        let put = |current: &str, new: &str| {
            let (n, v) = bearer(&token);
            request
                .put("/api/me/password")
                .add_header(n, v)
                .json(&json!({"current_password": current, "new_password": new}))
        };

        assert_eq!(
            put("wrong current password", "a brand new password 42")
                .await
                .status_code(),
            400
        );
        assert_eq!(put(PASSWORD, "short").await.status_code(), 400);
        assert_eq!(put(PASSWORD, "password1234").await.status_code(), 400);
        assert_eq!(put(PASSWORD, PASSWORD).await.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn change_password_signs_out_other_devices_but_keeps_this_one() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (token, _) = sign_in(&request, "a@brightcone.ai").await;
        let (_, other_device) = sign_in(&request, "a@brightcone.ai").await;

        let (n, v) = bearer(&token);
        let response = request
            .put("/api/me/password")
            .add_header(n, v)
            .json(&json!({"current_password": PASSWORD, "new_password": "a brand new password 42"}))
            .await;
        assert_eq!(response.status_code(), 200);

        // This device got a new session; the other one is revoked.
        let this_device = refresh_secret(&response);
        assert_eq!(
            refresh_with(&request, &other_device).await.status_code(),
            401
        );
        assert_eq!(
            refresh_with(&request, &this_device).await.status_code(),
            200
        );
        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_disabled_account_loses_access_immediately() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (token, _) = sign_in(&request, "a@brightcone.ai").await;
        assert_eq!(get_as(&request, "/api/me", &token).await.status_code(), 200);

        user.into_active_model()
            .set_status(&ctx.db, "disabled")
            .await
            .unwrap();
        assert_eq!(get_as(&request, "/api/me", &token).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_user_who_must_change_their_password_can_only_change_it() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::HrAdmin).await;
        user.into_active_model()
            .set_password(&ctx.db, PASSWORD, true)
            .await
            .unwrap();
        let (token, _) = sign_in(&request, "a@brightcone.ai").await;

        assert_eq!(get_as(&request, "/api/me", &token).await.status_code(), 200);
        let blocked = get_as(&request, "/api/audit", &token).await;
        assert_eq!(blocked.status_code(), 403);
        assert_eq!(blocked.json::<Value>()["error"], "password_change_required");

        let (n, v) = bearer(&token);
        let changed = request
            .put("/api/me/password")
            .add_header(n, v)
            .json(&json!({"current_password": PASSWORD, "new_password": "a brand new password 42"}))
            .await;
        assert_eq!(changed.status_code(), 200);
        assert_eq!(
            get_as(&request, "/api/audit", &access_token(&changed))
                .await
                .status_code(),
            200
        );
        let user = users::Model::find_by_email(&ctx.db, "a@brightcone.ai")
            .await
            .unwrap();
        assert!(!user.must_change_password);
    })
    .await;
}
