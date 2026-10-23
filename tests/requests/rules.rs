use brightcone_hr_backend::{app::App, authz::Role};
use loco_rs::testing::prelude::*;
use serde_json::{json, Value};
use serial_test::serial;
use uuid::Uuid;

use super::support::*;

async fn post_json(
    request: &axum_test::TestServer,
    path: &str,
    token: &str,
    body: Value,
) -> axum_test::TestResponse {
    let (name, value) = bearer(token);
    request.post(path).add_header(name, value).json(&body).await
}

async fn hr_token(request: &axum_test::TestServer, ctx: &loco_rs::app::AppContext) -> String {
    make_user(ctx, "rules.hr@brightcone.ai", Role::HrAdmin).await;
    sign_in(request, "rules.hr@brightcone.ai").await.0
}

#[tokio::test]
#[serial]
async fn rules_are_hr_managed_and_dispatch_user_scoped_notifications() {
    request::<App, _, _>(|request, ctx| async move {
        let hr = hr_token(&request, &ctx).await;
        make_user(&ctx, "rules.employee@brightcone.ai", Role::Employee).await;
        let employee_token = sign_in(&request, "rules.employee@brightcone.ai").await.0;
        let forbidden = get_as(&request, "/api/rules", &employee_token).await;
        assert_eq!(forbidden.status_code(), 403);

        let (name, value) = bearer(&hr);
        let rules = request.get("/api/rules").add_header(name, value).await;
        assert_eq!(rules.status_code(), 200);
        assert_eq!(rules.json::<Value>().as_array().unwrap().len(), 9);

        let (name, value) = bearer(&hr);
        let enabled = request
            .put("/api/rules/employee.created")
            .add_header(name, value)
            .json(&json!({"enabled": true}))
            .await;
        assert_eq!(enabled.status_code(), 200);

        let event_id = Uuid::new_v4();
        brightcone_hr_backend::database::sql::execute(
            &ctx.db,
            "INSERT INTO hr_rule_events(event_id,action_key,payload) VALUES($1,'employee.created',$2)",
            vec![event_id.into(), json!({ "record_id": Uuid::new_v4(), "record_type": "employee", "actor_user_id": 0 }).into()],
        ).await.expect("insert pending rule event");
        brightcone_hr_backend::rule_engine::process_event(&ctx, event_id).await.expect("process rule event");

        let notices = get_as(&request, "/api/notifications", &hr).await;
        assert_eq!(notices.status_code(), 200);
        let body = notices.json::<Value>();
        assert_eq!(body["unread_count"], 1);
        assert_eq!(body["items"][0]["rule_key"], "employee.created");
        let notice_id = body["items"][0]["id"].as_str().unwrap();

        let metrics = get_as(&request, "/api/rules/metrics", &hr).await;
        assert_eq!(metrics.status_code(), 200);
        assert_eq!(metrics.json::<Value>()["complete"], 1);

        assert_eq!(
            get_as(&request, "/api/notifications", &employee_token)
                .await
                .json::<Value>()["unread_count"],
            0
        );
        assert_eq!(
            post_json(
                &request,
                &format!("/api/notifications/{notice_id}/read"),
                &employee_token,
                json!({})
            )
            .await
            .status_code(),
            404
        );

        let marked = post_json(
            &request,
            &format!("/api/notifications/{notice_id}/read"),
            &hr,
            json!({}),
        )
        .await;
        assert_eq!(marked.status_code(), 200);
        assert!(marked.json::<Value>()["read_at"].is_string());
    })
    .await;
}
