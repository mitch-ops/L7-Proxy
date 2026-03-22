use std::sync::Arc;

use tracing::{error, info};

use crate::balancer::create_balancer;
use crate::config::Config;
use crate::router::{Route, Router};
use crate::state::AppState;

/// Apply a new config to the running proxy, swapping routes and config atomically.
pub fn apply_config(state: &AppState, config: Config) {
    let routes = config
        .routes
        .iter()
        .map(|r| Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: create_balancer(&r.strategy, r.upstream.len(), r.weights.as_deref()),
        })
        .collect();

    let router = Arc::new(Router::new(routes));
    let config = Arc::new(config);

    state.router.store(router);
    state.config.store(config);

    info!("Configuration applied");
}

/// Read config from disk and apply it.
pub fn reload_config(state: &AppState, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_str = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&config_str)?;
    apply_config(state, config);
    Ok(())
}

/// Spawn a background task that listens for SIGHUP and reloads config.
pub fn spawn_config_reloader(state: Arc<AppState>, config_path: String) {
    #[cfg(unix)]
    {
        tokio::spawn(async move {
            let mut sighup = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::hangup(),
            )
            .expect("failed to listen for SIGHUP");

            loop {
                sighup.recv().await;
                info!(path = %config_path, "SIGHUP received, reloading configuration");
                match reload_config(&state, &config_path) {
                    Ok(()) => {}
                    Err(e) => error!(error = %e, "Failed to reload configuration"),
                }
            }
        });
    }
}
