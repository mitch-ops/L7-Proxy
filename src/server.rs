use hyper::server::conn::AddrStream;
use hyper::service::make_service_fn;
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tracing::info;

use arc_swap::ArcSwap;

use crate::config::Config;
use crate::middleware::{
    InjectAddrService, LoggingLayer, ProxyService, RateLimitLayer, RequestTimeoutLayer,
};
use crate::rate_limiter::RateLimiter;
use crate::state::AppState;

pub async fn start_server(
    state: Arc<AppState>,
    listener: tokio::net::TcpListener,
    shutdown_signal: impl Future<Output = ()> + Send + 'static,
) {
    let config = state.config.load();
    let overall_timeout = Duration::from_secs(config.server.overall_timeout_secs);
    let drain_timeout = Duration::from_secs(config.server.drain_timeout_secs);

    let make_svc = make_service_fn(move |conn: &AddrStream| {
        let remote_ip = conn.remote_addr().ip();
        let state = state.clone();
        async move {
            let rate_limiter = state.rate_limiter.clone();
            let svc = ServiceBuilder::new()
                .layer(LoggingLayer)
                .layer(RateLimitLayer::new(rate_limiter))
                .layer(RequestTimeoutLayer::new(overall_timeout))
                .service(ProxyService::new(state));
            Ok::<_, Infallible>(InjectAddrService {
                inner: svc,
                remote_ip,
            })
        }
    });

    // Share the shutdown signal between graceful shutdown and drain timeout
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        shutdown_signal.await;
        let _ = shutdown_tx.send(true);
    });

    let mut graceful_rx = shutdown_rx.clone();
    let server = hyper::Server::from_tcp(listener.into_std().unwrap())
        .unwrap()
        .serve(make_svc)
        .with_graceful_shutdown(async move {
            graceful_rx.changed().await.ok();
            info!("Shutdown signal received, draining connections...");
        });

    // Race the server against the drain timeout.
    // The drain timeout only starts counting after the shutdown signal fires.
    let mut drain_rx = shutdown_rx;
    tokio::select! {
        result = server => {
            match result {
                Ok(_) => info!("Server shut down gracefully"),
                Err(e) => tracing::error!("Server error: {}", e),
            }
        }
        _ = async move {
            drain_rx.changed().await.ok();
            tokio::time::sleep(drain_timeout).await;
        } => {
            info!("Drain timeout exceeded ({}s), forcing shutdown", drain_timeout.as_secs());
        }
    }
}

pub async fn start_metrics_server(state: Arc<AppState>, listener: tokio::net::TcpListener) {
    let make_svc = make_service_fn(move |_| {
        let state = state.clone();
        async move {
            Ok::<_, Infallible>(hyper::service::service_fn(move |_req| {
                let state = state.clone();
                async move {
                    let body = state.metrics.render();
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(200)
                            .header("content-type", "text/plain; version=0.0.4")
                            .body(hyper::Body::from(body))
                            .unwrap(),
                    )
                }
            }))
        }
    });

    hyper::Server::from_tcp(listener.into_std().unwrap())
        .unwrap()
        .serve(make_svc)
        .await
        .unwrap();
}

pub async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM");

        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await.expect("failed to listen for ctrl+c");
}

pub async fn start_proxy_for_test() -> std::net::SocketAddr {
    let config = Config {
        server: crate::config::ServerConfig {
            bind: "127.0.0.1:8080".to_string(),
            request_timeout_secs: 2,
            max_retries: 5,
            failure_threshold: 3,
            health_cooldown_secs: 30,
            health_check: None,
            retry_backoff_ms: 10,
            retry_budget_percent: 20.0,
            retry_budget_window_secs: 10,
            metrics_bind: None,
            overall_timeout_secs: 30,
            rate_limit: None,
            drain_timeout_secs: 30,
        },
        routes: vec![crate::config::RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![
                "http://127.0.0.1:9001".to_string(),
                "http://127.0.0.1:9002".to_string(),
            ],
        }],
    };

    start_proxy_with_config(config).await
}

async fn build_proxy(config: Config) -> (Arc<AppState>, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();

    let routes = config
        .routes
        .iter()
        .map(|r| crate::router::Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: crate::balancer::RoundRobin::new(),
        })
        .collect();

    let router = Arc::new(crate::router::Router::new(routes));
    let client = hyper::Client::new();

    let health = Arc::new(crate::health::HealthTracker::new(
        config.server.failure_threshold,
        std::time::Duration::from_secs(config.server.health_cooldown_secs),
    ));

    let retry_budget = Arc::new(crate::retry_budget::RetryBudget::new(
        config.server.retry_budget_percent,
        config.server.retry_budget_window_secs,
    ));

    let metrics = Arc::new(crate::metrics::Metrics::new());

    let rate_limiter = Arc::new(match &config.server.rate_limit {
        Some(rl) => RateLimiter::new(rl.requests_per_second, rl.burst_size),
        None => RateLimiter::disabled(),
    });

    let state = Arc::new(AppState {
        router: ArcSwap::new(router),
        client,
        config: ArcSwap::new(Arc::new(config)),
        health,
        retry_budget,
        metrics,
        rate_limiter,
    });

    crate::health_check::spawn_active_health_checker(state.clone());

    (state, listener)
}

pub async fn start_proxy_with_config(config: Config) -> std::net::SocketAddr {
    let (state, listener) = build_proxy(config).await;
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        start_server(state, listener, std::future::pending::<()>()).await;
    });

    addr
}

pub async fn start_proxy_with_shutdown(
    config: Config,
) -> (std::net::SocketAddr, tokio::sync::oneshot::Sender<()>) {
    let (state, listener) = build_proxy(config).await;
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        start_server(state, listener, async {
            shutdown_rx.await.ok();
        })
        .await;
    });

    (addr, shutdown_tx)
}
