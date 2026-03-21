use hyper::service::make_service_fn;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;

use crate::config::Config;
use crate::middleware::{LoggingLayer, ProxyService, RequestTimeoutLayer};
use crate::state::AppState;

pub async fn start_server(state: Arc<AppState>, listener: tokio::net::TcpListener) {
    let overall_timeout =
        Duration::from_secs(state.config.server.overall_timeout_secs);

    let make_svc = make_service_fn(move |_| {
        let state = state.clone();
        async move {
            let svc = ServiceBuilder::new()
                .layer(LoggingLayer)
                .layer(RequestTimeoutLayer::new(overall_timeout))
                .service(ProxyService::new(state));
            Ok::<_, Infallible>(svc)
        }
    });

    hyper::Server::from_tcp(listener.into_std().unwrap())
        .unwrap()
        .serve(make_svc)
        .await
        .unwrap();
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

pub async fn start_proxy_with_config(config: Config) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();

    let addr = listener.local_addr().unwrap();

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

    let state = Arc::new(AppState {
        router,
        client,
        config: Arc::new(config),
        health,
        retry_budget,
        metrics,
    });

    crate::health_check::spawn_active_health_checker(state.clone());

    tokio::spawn(async move {
        start_server(state, listener).await;
    });

    addr
}
