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

    for attempt in 0..=max_retries.min(upstream_count - 1) {
        let index = route.balancer.next_index(upstream_count);
        let selected_upstream = &route.upstreams[index];

        info!(
            request_id = %request_id,
            attempt = attempt,
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
            Ok(resp) => return Ok(resp),

            Err(e @ ProxyError::UpstreamFailure) | Err(e @ ProxyError::UpstreamTimeout) => {
                last_error = Some(e);
                continue;
            }

            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap_or(ProxyError::UpstreamFailure))
}
