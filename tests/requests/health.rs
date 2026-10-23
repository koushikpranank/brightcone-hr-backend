use brightcone_hr_backend::app::App;
use loco_rs::testing::prelude::*;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn health_reports_ok_when_database_is_up() {
    request::<App, _, _>(|request, _ctx| async move {
        let response = request.get("/api/health").await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["database"], "ok");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn health_returns_security_headers() {
    request::<App, _, _>(|request, _ctx| async move {
        let response = request.get("/api/health").await;

        // `frame-ancestors 'none'` is a CSP directive, not a standalone header.
        assert!(response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
        assert_eq!(
            response.headers()["strict-transport-security"],
            "max-age=31536000; includeSubDomains"
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn openapi_is_served_outside_production() {
    request::<App, _, _>(|request, _ctx| async move {
        let response = request.get("/api/openapi.json").await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["paths"]["/api/health"].is_object());
    })
    .await;
}
