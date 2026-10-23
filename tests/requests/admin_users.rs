use axum_test::TestServer;
use brightcone_hr_backend::{app::App, authz::Role, models::users};
use loco_rs::app::AppContext;
use loco_rs::testing::prelude::*;
use serde_json::{json, Value};
use serial_test::serial;

use super::support::*;

async fn hr_token(request: &TestServer, ctx: &AppContext) -> String {
    make_user(ctx, "hr@brightcone.ai", Role::HrAdmin).await;
    sign_in(request, "hr@brightcone.ai").await.0
}

async fn post_as(
    request: &TestServer,
    path: &str,
    token: &str,
    body: Value,
) -> axum_test::TestResponse {
    let (n, v) = bearer(token);
    request.post(path).add_header(n, v).json(&body).await
}

#[tokio::test]
#[serial]
async fn hr_creates_an_account_with_a_temporary_password_that_must_be_changed() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let created = post_as(
            &request,
            "/api/admin/users",
            &token,
            json!({"email": "New.Person@brightcone.ai", "name": "New Person", "role": "employee"}),
        )
        .await;
        assert_eq!(created.status_code(), 200, "{}", created.text());
        let body: Value = created.json();
        let temporary = body["temporary_password"].as_str().unwrap().to_owned();
        assert_eq!(body["user"]["email"], "new.person@brightcone.ai");
        assert_eq!(body["user"]["must_change_password"], true);

        // The password is only ever in this one response, never in the database in clear.
        let stored = users::Model::find_by_email(&ctx.db, "new.person@brightcone.ai")
            .await
            .unwrap();
        assert_ne!(stored.password, temporary);

        let login = login_with(&request, "new.person@brightcone.ai", &temporary, false).await;
        assert_eq!(login.status_code(), 200);
        assert_eq!(login.json::<Value>()["user"]["must_change_password"], true);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn creating_a_duplicate_or_a_bad_role_is_refused() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let body = json!({"email": "hr@brightcone.ai", "name": "Someone", "role": "employee"});
        assert_eq!(
            post_as(&request, "/api/admin/users", &token, body)
                .await
                .status_code(),
            409
        );
        let body = json!({"email": "x@brightcone.ai", "name": "Someone", "role": "superuser"});
        assert_eq!(
            post_as(&request, "/api/admin/users", &token, body)
                .await
                .status_code(),
            400
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unlock_restores_a_locked_account() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let victim = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        for _ in 0..5 {
            login_with(&request, "a@brightcone.ai", "wrong password here", false).await;
        }
        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 403);

        let path = format!("/api/admin/users/{}/unlock", victim.pid);
        assert_eq!(
            post_as(&request, &path, &token, json!({}))
                .await
                .status_code(),
            200
        );
        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn disabling_signs_the_person_out_everywhere() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let victim = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (access, refresh) = sign_in(&request, "a@brightcone.ai").await;

        let path = format!("/api/admin/users/{}/disable", victim.pid);
        assert_eq!(
            post_as(&request, &path, &token, json!({}))
                .await
                .status_code(),
            200
        );

        assert_eq!(
            get_as(&request, "/api/me", &access).await.status_code(),
            401
        );
        assert_eq!(refresh_with(&request, &refresh).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn the_last_hr_admin_cannot_be_demoted_disabled_or_self_locked() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let me = users::Model::find_by_email(&ctx.db, "hr@brightcone.ai")
            .await
            .unwrap();

        let disable = format!("/api/admin/users/{}/disable", me.pid);
        assert_eq!(
            post_as(&request, &disable, &token, json!({}))
                .await
                .status_code(),
            400
        );

        let (n, v) = bearer(&token);
        let demote = request
            .put(&format!("/api/admin/users/{}/role", me.pid))
            .add_header(n, v)
            .json(&json!({"role": "employee"}))
            .await;
        assert_eq!(demote.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_second_hr_admin_makes_a_demotion_possible_and_it_is_audited() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let other = make_user(&ctx, "other.hr@brightcone.ai", Role::HrAdmin).await;

        let (n, v) = bearer(&token);
        let demote = request
            .put(&format!("/api/admin/users/{}/role", other.pid))
            .add_header(n, v)
            .json(&json!({"role": "manager"}))
            .await;
        assert_eq!(demote.status_code(), 200);

        let log = get_as(&request, "/api/audit?action_prefix=user.role", &token).await;
        let items = log.json::<Value>()["items"].clone();
        assert_eq!(items[0]["action"], "user.role_changed");
        assert_eq!(items[0]["summary"], "hr_admin -> manager");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_password_reset_returns_a_temporary_password_and_revokes_sessions() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let victim = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, refresh) = sign_in(&request, "a@brightcone.ai").await;

        let path = format!("/api/admin/users/{}/reset-password", victim.pid);
        let reset = post_as(&request, &path, &token, json!({})).await;
        assert_eq!(reset.status_code(), 200);
        let temporary = reset.json::<Value>()["temporary_password"]
            .as_str()
            .unwrap()
            .to_owned();

        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 401);
        assert_eq!(
            login_with(&request, "a@brightcone.ai", &temporary, false)
                .await
                .status_code(),
            200
        );
        assert_eq!(refresh_with(&request, &refresh).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unknown_accounts_are_404_and_employees_are_403() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let nobody = "/api/admin/users/00000000-0000-0000-0000-000000000000/unlock";
        assert_eq!(
            post_as(&request, nobody, &token, json!({}))
                .await
                .status_code(),
            404
        );
        assert_eq!(
            post_as(
                &request,
                "/api/admin/users/not-a-uuid/unlock",
                &token,
                json!({})
            )
            .await
            .status_code(),
            404
        );

        make_user(&ctx, "emp@brightcone.ai", Role::Employee).await;
        let (emp, _) = sign_in(&request, "emp@brightcone.ai").await;
        let body = json!({"email": "x@brightcone.ai", "name": "Someone", "role": "hr_admin"});
        assert_eq!(
            post_as(&request, "/api/admin/users", &emp, body)
                .await
                .status_code(),
            403
        );
    })
    .await;
}
