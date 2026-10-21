//! HTTP-level tests for the AppService endpoints (ADR 0019), which need no database.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use nuck_server::api;
use nuck_server::config::Config;
use nuck_server::matrix::SubstrateClient;
use nuck_server::state::AppState;
use tower::ServiceExt;

fn app(hs_token: Option<&str>, appservice_user: Option<&str>) -> Router {
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        server_name: "test.example.org".to_owned(),
        database_url: None,
        tuwunel_url: "http://localhost:8008".to_owned(),
        tuwunel_admin_token: None,
        appservice_hs_token: hs_token.map(ToOwned::to_owned),
        appservice_user: appservice_user.map(ToOwned::to_owned),
        appservice_as_token: None,
        bot_user: None,
        bot_token: None,
        appservice_url: None,
        appservice_registration_path: None,
        appservice_id: "nuck-projector".to_owned(),
        reconcile_interval_secs: 300,
        reconcile_on_startup: false,
        message_backfill_limit: None,
        registration_mode: "token".to_owned(),
        registration_token: None,
        admin_users: Vec::new(),
    };
    let substrate = SubstrateClient::from_config(&config);
    api::router(Arc::new(AppState { config: Arc::new(config), projection: None, substrate }))
}

fn transaction_request(token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("PUT")
        .uri("/_matrix/app/v1/transactions/txn1")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(r#"{"events":[]}"#)).unwrap()
}

#[tokio::test]
async fn transaction_without_a_projection_store_is_503() {
    let response = app(None, None)
        .oneshot(transaction_request(None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn transaction_with_a_wrong_hs_token_is_401() {
    let response = app(Some("secret"), None)
        .oneshot(transaction_request(Some("wrong")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn transaction_without_a_token_when_one_is_required_is_401() {
    let response = app(Some("secret"), None)
        .oneshot(transaction_request(None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn user_query_claims_only_the_configured_projector() {
    let router = app(None, Some("@nuck-projector:test.example.org"));

    let claimed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/_matrix/app/v1/users/@nuck-projector:test.example.org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claimed.status(), StatusCode::OK);

    let other = router
        .oneshot(
            Request::builder()
                .uri("/_matrix/app/v1/users/@someone:test.example.org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn room_query_never_claims() {
    let response = app(None, None)
        .oneshot(
            Request::builder()
                .uri("/_matrix/app/v1/rooms/%23somewhere:test.example.org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
