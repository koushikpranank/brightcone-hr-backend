use axum_test::TestServer;
use brightcone_hr_backend::{app::App, authz::Role};
use loco_rs::testing::prelude::*;
use serde_json::{json, Value};
use serial_test::serial;

use super::support::*;

async fn hr_token(request: &TestServer, ctx: &loco_rs::app::AppContext) -> String {
    make_user(ctx, "records.hr@brightcone.ai", Role::HrAdmin).await;
    sign_in(request, "records.hr@brightcone.ai").await.0
}

async fn post_json(
    request: &TestServer,
    path: &str,
    token: &str,
    body: Value,
) -> axum_test::TestResponse {
    let (name, value) = bearer(token);
    request.post(path).add_header(name, value).json(&body).await
}

async fn put_json(
    request: &TestServer,
    path: &str,
    token: &str,
    body: Value,
) -> axum_test::TestResponse {
    let (name, value) = bearer(token);
    request.put(path).add_header(name, value).json(&body).await
}

fn record_data(code: &str, name: &str) -> Value {
    json!({ "code": code, "name": name, "status": "active" })
}

#[tokio::test]
#[serial]
async fn record_crud_is_hr_only_and_writes_history_transactionally() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        let employee = make_user(&ctx, "employee@brightcone.ai", Role::Employee).await;
        let employee_token = sign_in(&request, &employee.email).await.0;
        assert_eq!(
            get_as(&request, "/api/record-types", &employee_token)
                .await
                .status_code(),
            403
        );

        let created = post_json(
            &request,
            "/api/records/custom_record",
            &token,
            json!({
                "source_id": "record-100", "data": record_data("R-100", "Ada")
            }),
        )
        .await;
        assert_eq!(created.status_code(), 201, "{}", created.text());
        let record: Value = created.json();
        let id = record["id"].as_str().unwrap();
        assert_eq!(record["data"]["name"], "Ada");

        let duplicate = post_json(
            &request,
            "/api/records/custom_record",
            &token,
            json!({
                "source_id": "record-101", "data": record_data("R-100", "Augusta")
            }),
        )
        .await;
        assert_eq!(duplicate.status_code(), 409);

        let updated = put_json(
            &request,
            &format!("/api/records/id/{id}"),
            &token,
            json!({
                "data": record_data("R-100", "Grace")
            }),
        )
        .await;
        assert_eq!(updated.status_code(), 200, "{}", updated.text());
        assert_eq!(updated.json::<Value>()["data"]["name"], "Grace");

        assert_eq!(
            post_json(
                &request,
                &format!("/api/records/id/{id}/archive"),
                &token,
                json!({})
            )
            .await
            .status_code(),
            200
        );
        let active = get_as(&request, "/api/records/custom_record", &token)
            .await
            .json::<Value>();
        assert!(active["items"].as_array().unwrap().is_empty());
        assert_eq!(
            post_json(
                &request,
                &format!("/api/records/id/{id}/restore"),
                &token,
                json!({})
            )
            .await
            .status_code(),
            200
        );

        let history = get_as(&request, &format!("/api/records/id/{id}/history"), &token).await;
        assert_eq!(history.status_code(), 200);
        let history_body = history.json::<Value>();
        let actions = history_body
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["action"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actions, ["restored", "archived", "updated", "created"]);
        assert_eq!(
            get_as(&request, "/api/records/custom_record", &employee_token)
                .await
                .status_code(),
            403
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn backup_is_no_store_and_deduplicates_existing_source_records() {
    request::<App, _, _>(|request, ctx| async move {
        let token = hr_token(&request, &ctx).await;
        post_json(
            &request,
            "/api/records/custom_record",
            &token,
            json!({
                "source_id": "backup-record", "data": record_data("R-200", "Backup fixture")
            }),
        )
        .await;
        let backup = get_as(&request, "/api/backup", &token).await;
        assert_eq!(backup.status_code(), 200);
        assert_eq!(backup.headers()["cache-control"], "no-store, private");
        let document = backup.json::<Value>();
        assert_eq!(document["format_version"], 2);
        assert!(!document["history"].as_array().unwrap().is_empty());

        let imported = post_json(&request, "/api/backup/import", &token, document).await;
        assert_eq!(imported.status_code(), 200, "{}", imported.text());
        assert_eq!(imported.json::<Value>()["imported"], 0);
        assert_eq!(imported.json::<Value>()["duplicates_skipped"], 1);
    })
    .await;
}
