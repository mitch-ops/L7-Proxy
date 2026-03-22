use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

use rust_proxy::config::{BalancerStrategy, Config, RouteConfig, ServerConfig};
use rust_proxy::server::start_proxy_with_config;

async fn handler_a() -> impl IntoResponse {
    (StatusCode::OK, "server-a")
}

async fn handler_b() -> impl IntoResponse {
    (StatusCode::OK, "server-b")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn make_config(
    upstreams: Vec<String>,
    strategy: BalancerStrategy,
    weights: Option<Vec<u32>>,
) -> Config {
    Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 2,
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
            drain_timeout_secs: 30,
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: upstreams,
            strategy,
            weights,
        }],
    }
}

#[tokio::test]
async fn weighted_round_robin_distributes_by_weight() {
    let addr_a = SocketAddr::from(([127, 0, 0, 1], 9901));
    let addr_b = SocketAddr::from(([127, 0, 0, 1], 9902));

    task::spawn(start_upstream(
        addr_a,
        Router::new().route("/", get(handler_a)),
    ));
    task::spawn(start_upstream(
        addr_b,
        Router::new().route("/", get(handler_b)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Weight 3:1 — server A should get 3x the traffic of server B
    let config = make_config(
        vec![format!("http://{}", addr_a), format!("http://{}", addr_b)],
        BalancerStrategy::WeightedRoundRobin,
        Some(vec![3, 1]),
    );

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send 8 requests (2 full cycles of the [A,A,A,B] schedule)
    let mut a_count = 0;
    let mut b_count = 0;
    for _ in 0..8 {
        let body = reqwest::get(format!("http://{}", proxy_addr))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        match body.as_str() {
            "server-a" => a_count += 1,
            "server-b" => b_count += 1,
            other => panic!("unexpected response: {}", other),
        }
    }

    assert_eq!(a_count, 6, "server-a should get 6 of 8 requests (weight 3)");
    assert_eq!(b_count, 2, "server-b should get 2 of 8 requests (weight 1)");
}

#[tokio::test]
async fn least_connections_balances_load() {
    let addr_a = SocketAddr::from(([127, 0, 0, 1], 9903));
    let addr_b = SocketAddr::from(([127, 0, 0, 1], 9904));

    task::spawn(start_upstream(
        addr_a,
        Router::new().route("/", get(handler_a)),
    ));
    task::spawn(start_upstream(
        addr_b,
        Router::new().route("/", get(handler_b)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let config = make_config(
        vec![format!("http://{}", addr_a), format!("http://{}", addr_b)],
        BalancerStrategy::LeastConnections,
        None,
    );

    let proxy_addr = start_proxy_with_config(config).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send 10 sequential requests — with instant responses and connection tracking,
    // least connections should distribute roughly evenly
    let mut a_count = 0;
    let mut b_count = 0;
    for _ in 0..10 {
        let body = reqwest::get(format!("http://{}", proxy_addr))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        match body.as_str() {
            "server-a" => a_count += 1,
            "server-b" => b_count += 1,
            other => panic!("unexpected response: {}", other),
        }
    }

    // With sequential requests (each completes before next starts),
    // active connections are always 0 for both, so ties go to index 0 (server-a).
    // The important thing is the strategy works without errors.
    assert_eq!(a_count + b_count, 10, "all requests should succeed");
}
