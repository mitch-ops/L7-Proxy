use rust_proxy::config::{Config, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_config;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::task;

static TOTAL_HITS: AtomicUsize = AtomicUsize::new(0);

async fn fail_handler() -> impl IntoResponse {
    TOTAL_HITS.fetch_add(1, Ordering::SeqCst);
    (StatusCode::INTERNAL_SERVER_ERROR, "fail")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// When retry budget is exhausted, the proxy should stop retrying early.
/// We set budget to 0% so no retries are allowed at all — each request gets exactly 1 attempt.
#[tokio::test]
async fn retry_budget_limits_retries() {
    TOTAL_HITS.store(0, Ordering::SeqCst);

    let addr_a = SocketAddr::from(([127, 0, 0, 1], 9301));
    let addr_b = SocketAddr::from(([127, 0, 0, 1], 9302));

    task::spawn(start_upstream(
        addr_a,
        Router::new().route("/", get(fail_handler)),
    ));
    task::spawn(start_upstream(
        addr_b,
        Router::new().route("/", get(fail_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 2,
            max_retries: 5,
            failure_threshold: 100, // high threshold so health checks don't interfere
            health_cooldown_secs: 60,
            health_check: None,
            retry_backoff_ms: 10,
            retry_budget_percent: 0.0, // no retries allowed
            retry_budget_window_secs: 10,
            metrics_bind: None,
            overall_timeout_secs: 30,
            rate_limit: None,
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![
                format!("http://{}", addr_a),
                format!("http://{}", addr_b),
            ],
        }],
    };

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Send a request — with 0% budget, it should only attempt 1 upstream (no retries)
    let resp = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();

    // Should get a 502 (upstream failure, no retries allowed)
    assert_eq!(resp.status(), 502);

    let hits = TOTAL_HITS.load(Ordering::SeqCst);
    assert_eq!(hits, 1, "with 0% retry budget, only 1 attempt should be made (no retries)");
}
