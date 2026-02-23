use rust_proxy::server::start_proxy_for_test;

use axum::{Router, routing::get};
use axum::response::IntoResponse;
use axum::http::{StatusCode, request};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

async fn fail_handler() -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "fail")
}

async fn success_handler() -> impl IntoResponse {
    (StatusCode::OK, "success")
}

async fn healthy_upstream() {
    let app = Router::new().route("/", get(success_handler));
    let addr = SocketAddr::from(([127, 0, 0, 1], 9002));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn failing_upstream() {
    let app = Router::new().route("/", get(fail_handler));
    let addr = SocketAddr::from(([127, 0, 0, 1], 9001));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[tokio::test]
async fn proxy_retries_and_succeeds() {
    task::spawn(failing_upstream());
    task::spawn(healthy_upstream());

    tokio::time::sleep(Duration::from_millis(200)).await;

    let proxy_addr = start_proxy_for_test().await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let response = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    assert_eq!(body, "success");
}