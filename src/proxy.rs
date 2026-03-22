use crate::errors::ProxyError;
use crate::middleware::RemoteAddr;
use crate::state::AppState;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Body, Request, Response, Uri};
use rand::RngExt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{Duration, sleep, timeout};
use tracing::info;

/// Hop-by-hop headers that must not be forwarded to upstreams or back to clients.
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn strip_hop_by_hop_headers(headers: &mut hyper::HeaderMap) {
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(*name);
    }
}

async fn forward_once(
    mut req: Request<Body>,
    upstream: &str,
    final_path: &str,
    request_id: &str,
    client_ip: Option<IpAddr>,
    state: &AppState,
    request_timeout: Duration,
) -> Result<Response<Body>, ProxyError> {
    let new_uri = format!("{}{}", upstream, final_path);

    let parsed = new_uri
        .parse::<Uri>()
        .map_err(|_| ProxyError::UpstreamFailure)?;

    // Save original host before overwriting URI
    let original_host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    *req.uri_mut() = parsed;

    // --- Header normalization ---

    req.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        request_id.parse().unwrap(),
    );

    // X-Forwarded-For: append client IP
    if let Some(ip) = client_ip {
        let forwarded_for = match req.headers().get("x-forwarded-for") {
            Some(existing) => {
                let existing_str = existing.to_str().unwrap_or("");
                format!("{}, {}", existing_str, ip)
            }
            None => ip.to_string(),
        };
        req.headers_mut().insert(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_str(&forwarded_for).unwrap(),
        );
    }

    // X-Forwarded-Proto
    req.headers_mut().insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static("http"),
    );

    // X-Forwarded-Host: original Host header
    if let Some(host) = original_host {
        if let Ok(val) = HeaderValue::from_str(&host) {
            req.headers_mut()
                .insert(HeaderName::from_static("x-forwarded-host"), val);
        }
    }

    // Strip hop-by-hop headers from the outgoing request
    strip_hop_by_hop_headers(req.headers_mut());

    let upstream_call = state.client.request(req);

    match timeout(request_timeout, upstream_call).await {
        Ok(Ok(mut resp)) => {
            // Strip hop-by-hop headers from the upstream response
            strip_hop_by_hop_headers(resp.headers_mut());
            Ok(resp)
        }
        Ok(Err(_)) => Err(ProxyError::UpstreamFailure),
        Err(_) => Err(ProxyError::UpstreamTimeout),
    }
}

pub async fn proxy_request(
    req: Request<Body>,
    state: Arc<AppState>,
) -> Result<Response<Body>, ProxyError> {
    let start = Instant::now();
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let method_str = method.to_string();
    let request_id = uuid::Uuid::new_v4().to_string();
    let client_ip = req.extensions().get::<RemoteAddr>().map(|r| r.0);

    // Load current config and router (snapshot for this request)
    let config = state.config.load();
    let router = state.router.load();

    info!(
        request_id = %request_id,
        method = ?req.method(),
        path = ?req.uri().path(),
        "Proxying request"
    );

    // Match route
    let route = match router.match_route(&path) {
        Some(route) => route,
        None => {
            state
                .metrics
                .requests_total
                .with_label_values(&[&method_str, "unmatched", "404"])
                .inc();
            return Ok(Response::builder()
                .status(404)
                .body(Body::from("No matching route"))
                .unwrap());
        }
    };

    let route_prefix = route.prefix.clone();

    // Build upstream URI
    let original_path = req
        .uri()
        .path_and_query()
        .map(|x| x.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    let stripped_path = original_path
        .strip_prefix(&route.prefix)
        .unwrap_or(&original_path)
        .to_string();

    let final_path = if stripped_path.is_empty() {
        "/".to_string()
    } else {
        stripped_path
    };

    let max_retries = config.server.max_retries;
    let upstream_count = route.upstreams.len();
    let request_timeout = Duration::from_secs(config.server.request_timeout_secs);

    let headers = req.headers().clone();
    let whole_body = hyper::body::to_bytes(req.into_body())
        .await
        .map_err(|_| ProxyError::UpstreamFailure)?;

    let mut last_error = None;
    let start_index = route.balancer.next_index_for_key(upstream_count, &path);

    // If all upstreams are unhealthy, ignore health status (fallback to current behavior)
    let any_healthy = route.upstreams.iter().any(|u| state.health.is_healthy(u));

    let max_attempts = max_retries + 1;
    let mut attempts_made = 0;
    let backoff_base_ms = config.server.retry_backoff_ms;

    // Record this request for retry budget tracking
    state.retry_budget.record_request();

    for offset in 0..upstream_count {
        if attempts_made >= max_attempts {
            break;
        }

        let index = (start_index + offset) % upstream_count;
        let selected_upstream = &route.upstreams[index];

        // Skip unhealthy upstreams (unless all are unhealthy)
        if any_healthy && !state.health.is_healthy(selected_upstream) {
            info!(
                request_id = %request_id,
                upstream = selected_upstream,
                "Skipping unhealthy upstream"
            );
            continue;
        }

        // For retries (not the first attempt), check budget and apply backoff
        if attempts_made > 0 {
            if !state.retry_budget.allow_retry() {
                info!(
                    request_id = %request_id,
                    "Retry budget exhausted, stopping retries"
                );
                break;
            }

            state
                .metrics
                .retries_total
                .with_label_values(&[&route_prefix])
                .inc();

            // Exponential backoff with jitter: base * 2^(attempt-1) + random jitter
            let exp_delay = backoff_base_ms.saturating_mul(1 << (attempts_made - 1));
            let jitter = rand::rng().random_range(0..=exp_delay / 2);
            let total_delay = exp_delay + jitter;

            info!(
                request_id = %request_id,
                delay_ms = total_delay,
                "Backing off before retry"
            );

            sleep(Duration::from_millis(total_delay)).await;
        }

        attempts_made += 1;

        info!(
            request_id = %request_id,
            attempt = attempts_made,
            upstream = selected_upstream,
            "Attempting upstream"
        );

        let mut new_req = Request::builder()
            .method(method.clone())
            .uri("/") // placeholder, will be replaced in forward_once
            .body(Body::from(whole_body.clone()))
            .unwrap();

        *new_req.headers_mut() = headers.clone();

        route.balancer.record_start(index);

        let result = forward_once(
            new_req,
            selected_upstream,
            &final_path,
            &request_id,
            client_ip,
            &state,
            request_timeout,
        )
        .await;

        route.balancer.record_done(index);

        match result {
            Ok(resp) => {
                if resp.status().is_server_error() {
                    state.health.record_failure(selected_upstream);
                    state
                        .metrics
                        .errors_total
                        .with_label_values(&[&route_prefix, "5xx"])
                        .inc();
                    last_error = Some(ProxyError::UpstreamFailure);
                    continue;
                }
                state.health.record_success(selected_upstream);

                let status = resp.status().as_u16().to_string();
                let elapsed = start.elapsed().as_secs_f64();
                state
                    .metrics
                    .requests_total
                    .with_label_values(&[&method_str, &route_prefix, &status])
                    .inc();
                state
                    .metrics
                    .request_duration_seconds
                    .with_label_values(&[&method_str, &route_prefix])
                    .observe(elapsed);

                return Ok(resp);
            }

            Err(e @ ProxyError::UpstreamFailure) | Err(e @ ProxyError::UpstreamTimeout) => {
                state.health.record_failure(selected_upstream);
                let error_type = match &e {
                    ProxyError::UpstreamTimeout => "timeout",
                    _ => "connection",
                };
                state
                    .metrics
                    .errors_total
                    .with_label_values(&[&route_prefix, error_type])
                    .inc();
                last_error = Some(e);
                continue;
            }

            Err(e) => return Err(e),
        }
    }

    // Record failed request metrics
    let elapsed = start.elapsed().as_secs_f64();
    let error = last_error.unwrap_or(ProxyError::UpstreamFailure);
    let status = match &error {
        ProxyError::UpstreamTimeout => "504",
        _ => "502",
    };
    state
        .metrics
        .requests_total
        .with_label_values(&[&method_str, &route_prefix, status])
        .inc();
    state
        .metrics
        .request_duration_seconds
        .with_label_values(&[&method_str, &route_prefix])
        .observe(elapsed);

    Err(error)
}
