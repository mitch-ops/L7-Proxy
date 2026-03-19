use rust_proxy::config::{Config, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_config;

use axum::response::IntoResponse;
use axum::http::StatusCode;
use axum::{Router, routing::get};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::task;

static FAIL_UPSTREAM_HITS: AtomicUsize = AtomicUsize::new(0);

async fn fail_handler() -> impl IntoResponse {
    FAIL_UPSTREAM_HITS.fetch_add(1, Ordering::SeqCst);
    (StatusCode::INTERNAL_SERVER_ERROR, "fail")
}

async fn success_handler() -> impl IntoResponse {
    (StatusCode::OK, "success")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[tokio::test]
async fn unhealthy_upstream_is_skipped() {
    // Reset counter
    FAIL_UPSTREAM_HITS.store(0, Ordering::SeqCst);

    let fail_addr = SocketAddr::from(([127, 0, 0, 1], 9101));
    let ok_addr = SocketAddr::from(([127, 0, 0, 1], 9102));

    task::spawn(start_upstream(
        fail_addr,
        Router::new().route("/", get(fail_handler)),
    ));
    task::spawn(start_upstream(
        ok_addr,
        Router::new().route("/", get(success_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 2,
            max_retries: 5,
            failure_threshold: 1, // Mark unhealthy after 1 failure
            health_cooldown_secs: 60,
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![
                format!("http://{}", fail_addr),
                format!("http://{}", ok_addr),
            ],
        }],
    };

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // First request: hits fail upstream, retries to success upstream
    // This marks the fail upstream as unhealthy (threshold=1)
    let resp = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let hits_after_first = FAIL_UPSTREAM_HITS.load(Ordering::SeqCst);
    assert_eq!(hits_after_first, 1, "fail upstream should be hit once on first request");

    // Second request: should skip the unhealthy upstream entirely
    let resp = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let hits_after_second = FAIL_UPSTREAM_HITS.load(Ordering::SeqCst);
    assert_eq!(
        hits_after_second, 1,
        "fail upstream should NOT be hit on second request (skipped as unhealthy)"
    );
}
