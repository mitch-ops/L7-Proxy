use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::config::HealthCheckConfig;
use crate::state::AppState;

pub fn spawn_active_health_checker(state: Arc<AppState>) {
    let config = state.config.load();
    let hc_config = match &config.server.health_check {
        Some(cfg) => cfg.clone(),
        None => return,
    };

    tokio::spawn(async move {
        run_health_check_loop(state, hc_config).await;
    });
}

async fn run_health_check_loop(state: Arc<AppState>, config: HealthCheckConfig) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .expect("failed to build health check client");

    let interval = Duration::from_secs(config.interval_secs);

    // Collect all unique upstreams across all routes
    let upstreams: Vec<String> = {
        let cfg = state.config.load();
        let mut set = HashSet::new();
        for route_cfg in &cfg.routes {
            for upstream in &route_cfg.upstream {
                set.insert(upstream.clone());
            }
        }
        set.into_iter().collect()
    };

    info!(
        upstreams = ?upstreams,
        path = %config.path,
        interval_secs = config.interval_secs,
        "Starting active health checks"
    );

    loop {
        tokio::time::sleep(interval).await;

        for upstream in &upstreams {
            let url = format!("{}{}", upstream, config.path);

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    state.health.record_success(upstream);
                }
                Ok(resp) => {
                    warn!(
                        upstream = %upstream,
                        status = %resp.status(),
                        "Health check failed (bad status)"
                    );
                    state.health.record_failure(upstream);
                }
                Err(e) => {
                    warn!(
                        upstream = %upstream,
                        error = %e,
                        "Health check failed (connection error)"
                    );
                    state.health.record_failure(upstream);
                }
            }
        }
    }
}
