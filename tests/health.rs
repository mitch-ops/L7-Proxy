use rust_proxy::config::{Config, HealthCheckConfig, RouteConfig, ServerConfig};
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
            health_check: None,
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

// --- Active health check test ---

static ACTIVE_FAIL_HITS: AtomicUsize = AtomicUsize::new(0);

async fn health_fail_handler() -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "unhealthy")
}

async fn health_ok_handler() -> impl IntoResponse {
    (StatusCode::OK, "healthy")
}

async fn root_ok_handler() -> impl IntoResponse {
    ACTIVE_FAIL_HITS.fetch_add(1, Ordering::SeqCst);
    (StatusCode::OK, "ok-from-a")
}

#[tokio::test]
async fn active_health_check_marks_upstream_unhealthy() {
    ACTIVE_FAIL_HITS.store(0, Ordering::SeqCst);

    let addr_a = SocketAddr::from(([127, 0, 0, 1], 9201));
    let addr_b = SocketAddr::from(([127, 0, 0, 1], 9202));

    // Upstream A: 200 on /, but 500 on /health
    task::spawn(start_upstream(
        addr_a,
        Router::new()
            .route("/", get(root_ok_handler))
            .route("/health", get(health_fail_handler)),
    ));

    // Upstream B: 200 on / and /health
    task::spawn(start_upstream(
        addr_b,
        Router::new()
            .route("/", get(success_handler))
            .route("/health", get(health_ok_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 2,
            max_retries: 5,
            failure_threshold: 1,
            health_cooldown_secs: 60,
            health_check: Some(HealthCheckConfig {
                path: "/health".to_string(),
                interval_secs: 1,
                timeout_secs: 1,
            }),
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

    // Wait for 2+ health check cycles to mark upstream A as unhealthy
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Send several requests — they should all go to upstream B (skipping A)
    for _ in 0..3 {
        let resp = reqwest::get(format!("http://{}", proxy_addr))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    let a_hits = ACTIVE_FAIL_HITS.load(Ordering::SeqCst);
    assert_eq!(
        a_hits, 0,
        "upstream A should not receive requests after being marked unhealthy by active checks"
    );
}
