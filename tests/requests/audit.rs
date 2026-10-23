use brightcone_hr_backend::{
    app::App,
    audit::{self, Entry},
    authz::Role,
    models::_entities::audit_logs,
    request_meta::RequestMeta,
};
use loco_rs::testing::prelude::*;
use sea_orm::{ConnectionTrait, EntityTrait};
use serde_json::Value;
use serial_test::serial;

use super::support::*;

#[tokio::test]
#[serial]
async fn sign_in_events_are_recorded_without_secrets() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "hr@brightcone.ai", Role::HrAdmin).await;
        login_with(
            &request,
            "hr@brightcone.ai",
            "this is a wrong password",
            false,
        )
        .await;
        login_with(
            &request,
            "ghost@brightcone.ai",
            "another wrong password",
            false,
        )
        .await;
        let (token, refresh) = sign_in(&request, "hr@brightcone.ai").await;

        let log = get_as(&request, "/api/audit?action_prefix=auth.", &token).await;
        assert_eq!(log.status_code(), 200);
        let text = log.text();
        let actions: Vec<String> = log.json::<Value>()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["action"].as_str().unwrap().to_owned())
            .collect();
        assert!(actions.contains(&"auth.login".to_owned()));
        assert_eq!(
            actions.iter().filter(|a| *a == "auth.login_failed").count(),
            2
        );

        for secret in [
            PASSWORD,
            "this is a wrong password",
            "another wrong password",
            &token,
            &refresh,
        ] {
            assert!(!text.contains(secret), "audit output leaked a secret");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn the_log_pages_by_cursor_newest_first() {
    request::<App, _, _>(|request, ctx| async move {
        make_user(&ctx, "hr@brightcone.ai", Role::HrAdmin).await;
        let (token, _) = sign_in(&request, "hr@brightcone.ai").await;
        for n in 0..5 {
            audit::record(
                &ctx.db,
                None,
                &RequestMeta::default(),
                Entry::new("test.event").summary(format!("event {n}")),
            )
            .await
            .unwrap();
        }

        let first: Value = get_as(&request, "/api/audit?action_prefix=test.&limit=2", &token)
            .await
            .json();
        assert_eq!(first["items"].as_array().unwrap().len(), 2);
        assert_eq!(first["items"][0]["summary"], "event 4");
        let cursor = first["next_before_id"].as_i64().expect("more pages");

        let second: Value = get_as(
            &request,
            &format!("/api/audit?action_prefix=test.&limit=2&before_id={cursor}"),
            &token,
        )
        .await
        .json();
        assert_eq!(second["items"][0]["summary"], "event 2");

        let last: Value = get_as(&request, "/api/audit?action_prefix=test.&limit=50", &token)
            .await
            .json();
        assert_eq!(last["items"].as_array().unwrap().len(), 5);
        assert!(last["next_before_id"].is_null());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn audit_rows_cannot_be_changed_or_deleted_even_by_the_table_owner() {
    request::<App, _, _>(|_request, ctx| async move {
        audit::record(
            &ctx.db,
            None,
            &RequestMeta::default(),
            Entry::new("test.event"),
        )
        .await
        .unwrap();

        let update = ctx
            .db
            .execute_unprepared("UPDATE audit_logs SET action = 'tampered'")
            .await;
        assert!(update.is_err(), "UPDATE must be blocked by the trigger");
        let delete = ctx.db.execute_unprepared("DELETE FROM audit_logs").await;
        assert!(delete.is_err(), "DELETE must be blocked by the trigger");
        let truncate = ctx.db.execute_unprepared("TRUNCATE audit_logs").await;
        assert!(truncate.is_err(), "TRUNCATE must be blocked by the trigger");

        let rows = audit_logs::Entity::find().all(&ctx.db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, "test.event");
    })
    .await;
}
