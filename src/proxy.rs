use crate::config;
use crate::errors::ProxyError;
use crate::state::AppState;
use hyper::header::HeaderName;
use hyper::{Body, Request, Response, Uri};
use std::{convert::Infallible, sync::Arc};
use tokio::time::{Duration, timeout};
use tracing::{error, info};
use uuid::Uuid;

/**
 * Core proxy logic:
 * - Match route
 * - Build upstream URI
 * - Forward request
 * - Handle response
 * - Retry logic
 */

/**
 * Forward request to upstream and return response. Returns ProxyError on failure or timeout.
 * No cloning
 * takes ownership of req, so we can modify it without cloning.
 */
async fn forward_once(
    mut req: Request<Body>,
    upstream: &str,
    final_path: &str,
    request_id: &str,
    state: &AppState,
) -> Result<Response<Body>, ProxyError> {
    let new_uri = format!("{}{}", upstream, final_path);

    let parsed = new_uri
        .parse::<Uri>()
        .map_err(|_| ProxyError::UpstreamFailure)?;

    *req.uri_mut() = parsed;

    req.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        request_id.parse().unwrap(),
    );

    let upstream_call = state.client.request(req);

    match timeout(
        Duration::from_secs(state.config.server.request_timeout_secs),
        upstream_call,
    )
    .await
    {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(_)) => Err(ProxyError::UpstreamFailure),
        Err(_) => Err(ProxyError::UpstreamTimeout),
    }
}

pub async fn proxy_request(
    req: Request<Body>,
    state: Arc<AppState>,
) -> Result<Response<Body>, ProxyError> {
    let path = req.uri().path().to_string();
    let request_id = uuid::Uuid::new_v4().to_string();

    info!(
        request_id = %request_id,
        method = ?req.method(),
        path = ?req.uri().path(),
        "Proxying request"
    );

    // Match route
    let route = match state.router.match_route(&path) {
        Some(route) => route,
        None => {
            return Ok(Response::builder()
                .status(404)
                .body(Body::from("No matching route"))
                .unwrap());
        }
    };

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

    let max_retries = state.config.server.max_retries;
    let upstream_count = route.upstreams.len();

    let method = req.method().clone();
    let headers = req.headers().clone();
    let whole_body = hyper::body::to_bytes(req.into_body())
        .await
        .map_err(|_| ProxyError::UpstreamFailure)?;

    let mut last_error = None;
    let start_index = route.balancer.next_index(upstream_count);

    // If all upstreams are unhealthy, ignore health status (fallback to current behavior)
    let any_healthy = route.upstreams.iter().any(|u| state.health.is_healthy(u));

    let max_attempts = max_retries + 1;
    let mut attempts_made = 0;

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

        match forward_once(new_req, selected_upstream, &final_path, &request_id, &state).await {
            Ok(resp) => {
                if resp.status().is_server_error() {
                    state.health.record_failure(selected_upstream);
                    last_error = Some(ProxyError::UpstreamFailure);
                    continue;
                }
                state.health.record_success(selected_upstream);
                return Ok(resp);
            }

            Err(e @ ProxyError::UpstreamFailure) | Err(e @ ProxyError::UpstreamTimeout) => {
                state.health.record_failure(selected_upstream);
                last_error = Some(e);
                continue;
            }

            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap_or(ProxyError::UpstreamFailure))
}
