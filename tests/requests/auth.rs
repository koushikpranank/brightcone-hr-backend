use brightcone_hr_backend::{
    app::App,
    authz::Role,
    models::{
        password_resets,
        refresh_tokens::hash_token,
        users::{self},
    },
};
use loco_rs::testing::prelude::*;
use sea_orm::{EntityTrait, IntoActiveModel};
use serde_json::{json, Value};
use serial_test::serial;

use super::support::*;

#[tokio::test]
#[serial]
async fn login_returns_access_token_and_a_hardened_refresh_cookie() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "rahul.varma@brightcone.ai", Role::Employee).await;

        let response = login_with(&request, "rahul.varma@brightcone.ai", PASSWORD, true).await;
        assert_eq!(response.status_code(), 200);

        let body: Value = response.json();
        assert_eq!(body["token_type"], "Bearer");
        assert_eq!(body["expires_in"], 900);
        assert_eq!(body["user"]["role"], "employee");
        // The refresh token is never in the body, only in the cookie.
        assert!(body.get("refresh_token").is_none());

        let cookie = set_cookie_line(&response);
        for part in [
            "HttpOnly",
            "SameSite=Strict",
            "Path=/api/auth",
            "Secure",
            "Max-Age=",
        ] {
            assert!(cookie.contains(part), "cookie is missing {part}: {cookie}");
        }
        assert_eq!(response.headers()["cache-control"], "no-store");

        // Only the hash is stored.
        let secret = refresh_secret(&response);
        let stored = brightcone_hr_backend::models::refresh_tokens::Entity::find()
            .all(&ctx.db)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].token_hash, hash_token(&secret));
        assert_ne!(stored[0].token_hash, secret);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_browser_session_login_has_no_max_age() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let response = login_with(&request, "a@brightcone.ai", PASSWORD, false).await;
        assert!(!set_cookie_line(&response).contains("Max-Age"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn email_is_case_insensitive() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "Rahul.Varma@Brightcone.ai", Role::Employee).await;
        let response = login_with(&request, "  RAHUL.varma@brightcone.AI ", PASSWORD, false).await;
        assert_eq!(response.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn wrong_password_and_unknown_email_look_identical() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;

        let wrong = login_with(
            &request,
            "a@brightcone.ai",
            "not the password at all",
            false,
        )
        .await;
        let unknown = login_with(&request, "nobody@brightcone.ai", PASSWORD, false).await;

        assert_eq!(wrong.status_code(), 401);
        assert_eq!(unknown.status_code(), 401);
        assert_eq!(wrong.text(), unknown.text());
        assert!(wrong.headers().get("set-cookie").is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn five_wrong_passwords_lock_the_account_even_for_the_right_password() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        for attempt in 1..=5 {
            let r = login_with(&request, "a@brightcone.ai", "wrong password here", false).await;
            assert_eq!(r.status_code(), 401, "attempt {attempt}");
        }

        let locked = users::Model::find_by_email(&ctx.db, "a@brightcone.ai")
            .await
            .unwrap();
        assert_eq!(locked.status, "locked");
        assert!(locked.locked_at.is_some());

        let right = login(&request, "a@brightcone.ai").await;
        assert_eq!(right.status_code(), 403);
        assert_eq!(right.json::<Value>()["error"], "account_locked");
        assert_eq!(user.failed_logins, 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_good_login_resets_the_failure_counter() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        for _ in 0..4 {
            login_with(&request, "a@brightcone.ai", "wrong password here", false).await;
        }
        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 200);
        let user = users::Model::find_by_email(&ctx.db, "a@brightcone.ai")
            .await
            .unwrap();
        assert_eq!(user.failed_logins, 0);
        assert!(user.last_login_at.is_some());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn disabled_account_cannot_sign_in() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        user.into_active_model()
            .set_status(&ctx.db, "disabled")
            .await
            .unwrap();

        let response = login(&request, "a@brightcone.ai").await;
        assert_eq!(response.status_code(), 403);
        assert_eq!(response.json::<Value>()["error"], "account_disabled");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn refresh_rotates_the_cookie_and_returns_a_new_access_token() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, first) = sign_in(&request, "a@brightcone.ai").await;

        let response = refresh_with(&request, &first).await;
        assert_eq!(response.status_code(), 200);
        let second = refresh_secret(&response);
        assert_ne!(first, second);

        let me = get_as(&request, "/api/me", &access_token(&response)).await;
        assert_eq!(me.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn reusing_an_old_refresh_token_kills_the_whole_session() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, first) = sign_in(&request, "a@brightcone.ai").await;
        let second = refresh_secret(&refresh_with(&request, &first).await);

        // An attacker (or a stale tab) replays the first token.
        let replay = refresh_with(&request, &first).await;
        assert_eq!(replay.status_code(), 401);

        // The legitimate newest token is now dead too.
        assert_eq!(refresh_with(&request, &second).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn refresh_needs_the_csrf_header_and_a_cookie() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, secret) = sign_in(&request, "a@brightcone.ai").await;

        let (cn, cv) = cookie(&secret);
        let no_header = request.post("/api/auth/refresh").add_header(cn, cv).await;
        assert_eq!(no_header.status_code(), 400);

        let (hn, hv) = csrf();
        let no_cookie = request.post("/api/auth/refresh").add_header(hn, hv).await;
        assert_eq!(no_cookie.status_code(), 401);

        assert_eq!(
            refresh_with(&request, "forged-secret").await.status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn refresh_is_refused_after_the_account_is_disabled() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, secret) = sign_in(&request, "a@brightcone.ai").await;
        user.into_active_model()
            .set_status(&ctx.db, "disabled")
            .await
            .unwrap();

        assert_eq!(refresh_with(&request, &secret).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logout_clears_the_cookie_and_revokes_the_session() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, secret) = sign_in(&request, "a@brightcone.ai").await;

        let (cn, cv) = cookie(&secret);
        let (hn, hv) = csrf();
        let out = request
            .post("/api/auth/logout")
            .add_header(cn, cv)
            .add_header(hn, hv)
            .await;
        assert_eq!(out.status_code(), 204);
        assert!(set_cookie_line(&out).contains("Max-Age=0"));

        assert_eq!(refresh_with(&request, &secret).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logout_is_safe_to_call_twice_or_without_a_cookie() {
    request::<App, _, _>(|request, _ctx| async move {
        let (hn, hv) = csrf();
        let out = request.post("/api/auth/logout").add_header(hn, hv).await;
        assert_eq!(out.status_code(), 204);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logout_all_signs_out_every_device() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, phone) = sign_in(&request, "a@brightcone.ai").await;
        let (_, laptop) = sign_in(&request, "a@brightcone.ai").await;

        let (cn, cv) = cookie(&phone);
        let (hn, hv) = csrf();
        request
            .post("/api/auth/logout?all=true")
            .add_header(cn, cv)
            .add_header(hn, hv)
            .await;

        assert_eq!(refresh_with(&request, &laptop).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn forgot_password_gives_the_same_answer_for_any_email_and_sends_mail_for_real_ones() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "a@brightcone.ai", Role::Employee).await;

        let real = request
            .post("/api/auth/forgot")
            .json(&json!({"email": "a@brightcone.ai"}))
            .await;
        let fake = request
            .post("/api/auth/forgot")
            .json(&json!({"email": "ghost@brightcone.ai"}))
            .await;
        assert_eq!(real.status_code(), 200);
        assert_eq!(real.text(), fake.text());

        let sent = &ctx.mailer.as_ref().expect("mailer").deliveries();
        assert_eq!(sent.count, 1);
        assert!(sent.messages[0].contains("reset-password?token="));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn reset_password_works_once_and_signs_everyone_out() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let (_, session) = sign_in(&request, "a@brightcone.ai").await;
        let token = password_resets::Model::issue(&ctx.db, user.id, 30)
            .await
            .unwrap();
        let new_password = "a brand new password 42";

        let ok = request
            .post("/api/auth/reset")
            .json(&json!({"token": token, "password": new_password}))
            .await;
        assert_eq!(ok.status_code(), 200);

        assert_eq!(
            login_with(&request, "a@brightcone.ai", new_password, false)
                .await
                .status_code(),
            200
        );
        assert_eq!(login(&request, "a@brightcone.ai").await.status_code(), 401);
        assert_eq!(refresh_with(&request, &session).await.status_code(), 401);

        let again = request
            .post("/api/auth/reset")
            .json(&json!({"token": token, "password": "yet another password 43"}))
            .await;
        assert_eq!(again.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn reset_rejects_weak_passwords_and_unknown_tokens() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let token = password_resets::Model::issue(&ctx.db, user.id, 30)
            .await
            .unwrap();

        let weak = request
            .post("/api/auth/reset")
            .json(&json!({"token": token, "password": "short"}))
            .await;
        assert_eq!(weak.status_code(), 400);
        let unknown = request
            .post("/api/auth/reset")
            .json(&json!({"token": "nope", "password": "a brand new password 42"}))
            .await;
        assert_eq!(unknown.status_code(), 400);

        // A weak password must not burn the token.
        let ok = request
            .post("/api/auth/reset")
            .json(&json!({"token": token, "password": "a brand new password 42"}))
            .await;
        assert_eq!(ok.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn an_expired_reset_link_is_refused() {
    request::<App, _, _>(|request, ctx| async move {
        let user = make_user(&ctx, "a@brightcone.ai", Role::Employee).await;
        let token = password_resets::Model::issue(&ctx.db, user.id, -1)
            .await
            .unwrap();
        let r = request
            .post("/api/auth/reset")
            .json(&json!({"token": token, "password": "a brand new password 42"}))
            .await;
        assert_eq!(r.status_code(), 400);
    })
    .await;
}
