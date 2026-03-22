use rust_proxy::config::{Config, RateLimitConfig, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_config;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

async fn ok_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[tokio::test]
async fn rate_limit_returns_429_when_exceeded() {
    let upstream_addr = SocketAddr::from(([127, 0, 0, 1], 9601));

    task::spawn(start_upstream(
        upstream_addr,
        Router::new().route("/", get(ok_handler)),
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
            drain_timeout_secs: 30,
            rate_limit: Some(RateLimitConfig {
                requests_per_second: 2.0,
                burst_size: 2,
            }),
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![format!("http://{}", upstream_addr)],
            strategy: Default::default(),
            weights: None,
        }],
    };

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // First 2 requests should succeed (burst_size = 2)
    let resp1 = reqwest::get(format!("http://{}", proxy_addr)).await.unwrap();
    assert_eq!(resp1.status(), 200, "first request should succeed");

    let resp2 = reqwest::get(format!("http://{}", proxy_addr)).await.unwrap();
    assert_eq!(resp2.status(), 200, "second request should succeed");

    // Third request immediately should be rate limited
    let resp3 = reqwest::get(format!("http://{}", proxy_addr)).await.unwrap();
    assert_eq!(resp3.status(), 429, "third request should be rate limited");

    // Verify Retry-After header
    let retry_after = resp3.headers().get("Retry-After").unwrap();
    assert_eq!(retry_after, "1");

    // Wait for tokens to refill (at 2/sec, 0.5s gives 1 token)
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Should succeed again after refill
    let resp4 = reqwest::get(format!("http://{}", proxy_addr)).await.unwrap();
    assert_eq!(resp4.status(), 200, "request should succeed after rate limit recovery");
}
