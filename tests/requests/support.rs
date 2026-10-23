//! Helpers shared by the request tests.
#![allow(dead_code)]
use axum::http::{header, HeaderName, HeaderValue};
use axum_test::{TestResponse, TestServer};
use brightcone_hr_backend::{
    authz::Role,
    models::users::{self, NewUser},
};
use loco_rs::prelude::*;
use serde_json::{json, Value};

pub const PASSWORD: &str = "correct horse battery staple 9";

/// Creates an active account directly in the database.
pub async fn make_user(ctx: &AppContext, email: &str, role: Role) -> users::Model {
    users::Model::create_with_password(
        &ctx.db,
        &NewUser {
            email: email.to_owned(),
            name: "Test Person".to_owned(),
            password: PASSWORD.to_owned(),
            role,
            must_change_password: false,
        },
    )
    .await
    .expect("create user")
}

pub fn csrf() -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static("x-requested-with"),
        HeaderValue::from_static("brightcone"),
    )
}

pub fn bearer(token: &str) -> (HeaderName, HeaderValue) {
    (
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
    )
}

pub fn cookie(secret: &str) -> (HeaderName, HeaderValue) {
    (
        header::COOKIE,
        HeaderValue::from_str(&format!("brightcone_rt={secret}")).expect("header"),
    )
}

pub async fn login_with(
    request: &TestServer,
    email: &str,
    password: &str,
    remember: bool,
) -> TestResponse {
    request
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": password, "remember": remember }))
        .await
}

pub async fn login(request: &TestServer, email: &str) -> TestResponse {
    login_with(request, email, PASSWORD, false).await
}

/// The full `Set-Cookie` line of a response.
pub fn set_cookie_line(response: &TestResponse) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|v| {
            v.to_str()
                .ok()
                .filter(|s| s.starts_with("brightcone_rt="))
                .map(str::to_owned)
        })
        .expect("response has a brightcone_rt cookie")
}

/// Just the secret from the refresh cookie.
pub fn refresh_secret(response: &TestResponse) -> String {
    set_cookie_line(response)
        .split(';')
        .next()
        .and_then(|kv| kv.split_once('='))
        .map(|(_, v)| v.to_owned())
        .expect("cookie value")
}

pub fn access_token(response: &TestResponse) -> String {
    response.json::<Value>()["access_token"]
        .as_str()
        .expect("access_token")
        .to_owned()
}

/// Signs in and returns `(access_token, refresh_secret)`.
pub async fn sign_in(request: &TestServer, email: &str) -> (String, String) {
    let response = login(request, email).await;
    assert_eq!(
        response.status_code(),
        200,
        "login failed: {}",
        response.text()
    );
    (access_token(&response), refresh_secret(&response))
}

pub async fn refresh_with(request: &TestServer, secret: &str) -> TestResponse {
    let (cn, cv) = cookie(secret);
    let (hn, hv) = csrf();
    request
        .post("/api/auth/refresh")
        .add_header(cn, cv)
        .add_header(hn, hv)
        .await
}

pub async fn get_as(request: &TestServer, path: &str, token: &str) -> TestResponse {
    let (n, v) = bearer(token);
    request.get(path).add_header(n, v).await
}
