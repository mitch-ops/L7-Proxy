use rust_proxy::config::{Config, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_config;

use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

async fn slow_handler() -> impl IntoResponse {
    // Sleep longer than the overall timeout
    tokio::time::sleep(Duration::from_secs(5)).await;
    (axum::http::StatusCode::OK, "slow response")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// When the overall request timeout (Tower middleware) fires,
/// the client should get a 504 even if the per-upstream timeout is longer.
#[tokio::test]
async fn overall_timeout_returns_504() {
    let upstream_addr = SocketAddr::from(([127, 0, 0, 1], 9501));

    task::spawn(start_upstream(
        upstream_addr,
        Router::new().route("/", get(slow_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 10, // per-upstream: generous
            max_retries: 0,
            failure_threshold: 100,
            health_cooldown_secs: 60,
            health_check: None,
            retry_backoff_ms: 10,
            retry_budget_percent: 100.0,
            retry_budget_window_secs: 10,
            metrics_bind: None,
            overall_timeout_secs: 1, // overall: strict — should fire first
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![format!("http://{}", upstream_addr)],
        }],
    };

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let start = std::time::Instant::now();
    let resp = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    // Should get 504 from the overall timeout middleware, not from per-upstream timeout
    assert_eq!(resp.status(), 504);

    // Should complete in ~1s (overall timeout), not ~5s (upstream sleep) or ~10s (per-upstream timeout)
    assert!(
        elapsed.as_secs() < 3,
        "overall timeout should fire at ~1s, but took {:?}",
        elapsed
    );
}
