use rust_proxy::config::{Config, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_shutdown;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

async fn slow_handler() -> impl IntoResponse {
    tokio::time::sleep(Duration::from_secs(1)).await;
    (StatusCode::OK, "slow but done")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// After a shutdown signal, in-flight requests should complete before the server exits.
#[tokio::test]
async fn graceful_shutdown_completes_in_flight_request() {
    let upstream_addr = SocketAddr::from(([127, 0, 0, 1], 9701));

    task::spawn(start_upstream(
        upstream_addr,
        Router::new().route("/", get(slow_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 5,
            max_retries: 0,
            failure_threshold: 100,
            health_cooldown_secs: 60,
            health_check: None,
            retry_backoff_ms: 10,
            retry_budget_percent: 100.0,
            retry_budget_window_secs: 10,
            metrics_bind: None,
            overall_timeout_secs: 30,
            rate_limit: None,
            drain_timeout_secs: 5, // 5s drain — plenty for the 1s upstream
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![format!("http://{}", upstream_addr)],
        }],
    };

    let (proxy_addr, shutdown_tx) = start_proxy_with_shutdown(config).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start an in-flight request (upstream takes 1 second)
    let proxy_url = format!("http://{}", proxy_addr);
    let req_handle = tokio::spawn(async move {
        reqwest::get(&proxy_url).await
    });

    // Wait for request to be in-flight, then trigger shutdown
    tokio::time::sleep(Duration::from_millis(200)).await;
    shutdown_tx.send(()).unwrap();

    // The in-flight request should still complete successfully
    let resp = req_handle.await.unwrap().unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert_eq!(body, "slow but done");
}
