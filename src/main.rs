mod balancer;
mod config;
mod errors;
mod health;
mod health_check;
mod metrics;
mod proxy;
mod router;
mod state;
mod retry_budget;
mod server;

use balancer::RoundRobin;
use config::Config;
use hyper::client::HttpConnector;
use hyper::{Body, Client};
use router::{Route, Router};
use std::fs;
use std::sync::Arc;
use tracing::info;
use crate::server::{start_server, start_metrics_server};

use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config_str = fs::read_to_string("config.yaml")?;
    let config: Config = serde_yaml::from_str(&config_str)?;
    let config = Arc::new(config);

    let addr: std::net::SocketAddr = config.server.bind.parse()?;

    let routes = config
        .routes
        .iter()
        .map(|r| Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: RoundRobin::new(),
        })
        .collect::<Vec<_>>();

    let router = Arc::new(Router::new(routes));

    info!("Starting HTTP server on {}", addr);

    let client: Client<HttpConnector, Body> = Client::new();

    let health = Arc::new(health::HealthTracker::new(
        config.server.failure_threshold,
        std::time::Duration::from_secs(config.server.health_cooldown_secs),
    ));

    let retry_budget = Arc::new(retry_budget::RetryBudget::new(
        config.server.retry_budget_percent,
        config.server.retry_budget_window_secs,
    ));

    let metrics = Arc::new(metrics::Metrics::new());

    let state = Arc::new(AppState { router, client, config, health, retry_budget, metrics });

    let listener = tokio::net::TcpListener::bind(addr).await?;

    health_check::spawn_active_health_checker(state.clone());

    // Start metrics server if configured
    if let Some(ref metrics_bind) = state.config.server.metrics_bind {
        let metrics_addr: std::net::SocketAddr = metrics_bind.parse()?;
        let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
        info!("Starting metrics server on {}", metrics_addr);
        let metrics_state = state.clone();
        tokio::spawn(async move {
            start_metrics_server(metrics_state, metrics_listener).await;
        });
    }

    start_server(state, listener).await;

    Ok(())
}
