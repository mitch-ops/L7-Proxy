use hyper::{
    Server,
    service::{make_service_fn, service_fn},
};
use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use crate::config::Config;
use crate::{proxy::proxy_request, state::AppState};

pub async fn start_server(state: Arc<AppState>, addr: SocketAddr) {
    let make_svc = make_service_fn(move |_| {
        let state = state.clone();
        async move { Ok::<_, Infallible>(service_fn(move |req| proxy_request(req, state.clone()))) }
    });

    Server::bind(&addr).serve(make_svc).await.unwrap();
}

pub async fn start_proxy_for_test() {
    let config = Config {
        server: crate::config::ServerConfig {
            bind: "127.0.0.1:8080".to_string(),
            request_timeout_secs: 2,
            max_retries: 5,
        },
        routes: vec![crate::config::RouteConfig {
            prefix: "/".to_string(),
            upstream: vec![
                "http://127.0.0.1:9001".to_string(),
                "http://127.0.0.1:9002".to_string(),
            ],
        }],
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

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

    let state = Arc::new(AppState {
        router,
        client,
        config: Arc::new(config), // Wrap config in Arc
    });

    start_server(state, addr).await;
}
