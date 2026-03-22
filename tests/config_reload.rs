use arc_swap::ArcSwap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task;

use rust_proxy::balancer::create_balancer;
use rust_proxy::config::{Config, RouteConfig, ServerConfig};
use rust_proxy::config_reloader::apply_config;
use rust_proxy::health::HealthTracker;
use rust_proxy::metrics::Metrics;
use rust_proxy::rate_limiter::RateLimiter;
use rust_proxy::retry_budget::RetryBudget;
use rust_proxy::router::{Route, Router as ProxyRouter};
use rust_proxy::server::start_server;
use rust_proxy::state::AppState;

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

fn make_config(upstreams: Vec<String>) -> Config {
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
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 32,
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: upstreams,
            strategy: Default::default(),
            weights: None,
        }],
    }
}

fn build_state(config: Config) -> Arc<AppState> {
    let routes = config
        .routes
        .iter()
        .map(|r| Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: create_balancer(&r.strategy, r.upstream.len(), r.weights.as_deref()),
        })
        .collect();

    let router = Arc::new(ProxyRouter::new(routes));
    let https = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build(https);
    let health = Arc::new(HealthTracker::new(
        config.server.failure_threshold,
        Duration::from_secs(config.server.health_cooldown_secs),
    ));
    let retry_budget = Arc::new(RetryBudget::new(
        config.server.retry_budget_percent,
        config.server.retry_budget_window_secs,
    ));
    let metrics = Arc::new(Metrics::new());
    let rate_limiter = Arc::new(RateLimiter::disabled());

    Arc::new(AppState {
        router: ArcSwap::new(router),
        client,
        config: ArcSwap::new(Arc::new(config)),
        health,
        retry_budget,
        metrics,
        rate_limiter,
    })
}

#[tokio::test]
async fn config_reload_swaps_upstreams() {
    let addr_a = SocketAddr::from(([127, 0, 0, 1], 9801));
    let addr_b = SocketAddr::from(([127, 0, 0, 1], 9802));

    task::spawn(start_upstream(
        addr_a,
        Router::new().route("/", get(handler_a)),
    ));
    task::spawn(start_upstream(
        addr_b,
        Router::new().route("/", get(handler_b)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start proxy pointing only at server A
    let config = make_config(vec![format!("http://{}", addr_a)]);
    let state = build_state(config);

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();

    let server_state = state.clone();
    tokio::spawn(async move {
        start_server(server_state, proxy_listener, std::future::pending::<()>()).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify requests go to server A
    let body = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "server-a");

    // Reload config to point at server B instead
    let new_config = make_config(vec![format!("http://{}", addr_b)]);
    apply_config(&state, new_config);

    // Verify requests now go to server B
    let body = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "server-b");
}
