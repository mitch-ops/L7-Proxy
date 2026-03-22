use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower::{Layer, Service};
use tracing::info;

use crate::proxy::proxy_request;
use crate::rate_limiter::RateLimiter;
use crate::state::AppState;

// --- ProxyService ---

/// Tower Service that wraps the core proxy_request handler.
/// Converts ProxyError results into HTTP responses so the error type is Infallible.
#[derive(Clone)]
pub struct ProxyService {
    state: Arc<AppState>,
}

impl ProxyService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

impl Service<Request<Body>> for ProxyService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        Box::pin(async move {
            match proxy_request(req, state).await {
                Ok(resp) => Ok(resp),
                Err(e) => Ok(e.into_response()),
            }
        })
    }
}

// --- LoggingLayer ---

/// Tower Layer that wraps a service with request/response logging.
/// Logs method, path, status code, and duration for every request.
#[derive(Clone)]
pub struct LoggingLayer;

impl<S> Layer<S> for LoggingLayer {
    type Service = LoggingMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoggingMiddleware { inner }
    }
}

#[derive(Clone)]
pub struct LoggingMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for LoggingMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let start = Instant::now();
        let fut = self.inner.call(req);

        Box::pin(async move {
            let resp = fut.await?;
            let elapsed = start.elapsed();

            info!(
                method = %method,
                path = %path,
                status = resp.status().as_u16(),
                duration_ms = elapsed.as_millis() as u64,
                "Request completed"
            );

            Ok(resp)
        })
    }
}

// --- RequestTimeoutLayer ---

/// Tower Layer that enforces an overall request timeout.
/// This is distinct from the per-upstream timeout in forward_once —
/// it caps the entire request lifecycle including all retries and backoff.
/// Returns 504 Gateway Timeout if exceeded.
#[derive(Clone)]
pub struct RequestTimeoutLayer {
    timeout: Duration,
}

impl RequestTimeoutLayer {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl<S> Layer<S> for RequestTimeoutLayer {
    type Service = RequestTimeoutMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestTimeoutMiddleware {
            inner,
            timeout: self.timeout,
        }
    }
}

#[derive(Clone)]
pub struct RequestTimeoutMiddleware<S> {
    inner: S,
    timeout: Duration,
}

impl<S> Service<Request<Body>> for RequestTimeoutMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let timeout_duration = self.timeout;
        let fut = self.inner.call(req);

        Box::pin(async move {
            match tokio::time::timeout(timeout_duration, fut).await {
                Ok(result) => result,
                Err(_) => Ok(Response::builder()
                    .status(StatusCode::GATEWAY_TIMEOUT)
                    .body(Body::from("Request timed out"))
                    .unwrap()),
            }
        })
    }
}

// --- InjectAddrService ---

/// Injects the client's remote IP as a request extension.
/// Created per-connection in start_server from the AddrStream.
#[derive(Clone)]
pub struct InjectAddrService<S> {
    pub inner: S,
    pub remote_ip: IpAddr,
}

/// Request extension carrying the client's remote IP address.
#[derive(Clone, Copy)]
pub struct RemoteAddr(pub IpAddr);

impl<S> Service<Request<Body>> for InjectAddrService<S>
where
    S: Service<Request<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        req.extensions_mut().insert(RemoteAddr(self.remote_ip));
        self.inner.call(req)
    }
}

// --- RateLimitLayer ---

/// Tower Layer that enforces per-IP rate limiting using a token bucket.
/// Returns 429 Too Many Requests with a Retry-After header when exceeded.
#[derive(Clone)]
pub struct RateLimitLayer {
    rate_limiter: Arc<RateLimiter>,
}

impl RateLimitLayer {
    pub fn new(rate_limiter: Arc<RateLimiter>) -> Self {
        Self { rate_limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    rate_limiter: Arc<RateLimiter>,
}

impl<S> Service<Request<Body>> for RateLimitMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if let Some(remote) = req.extensions().get::<RemoteAddr>() {
            if !self.rate_limiter.check(remote.0) {
                return Box::pin(async {
                    Ok(Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header("Retry-After", "1")
                        .body(Body::from("Rate limit exceeded"))
                        .unwrap())
                });
            }
        }

        let fut = self.inner.call(req);
        Box::pin(fut)
    }
}
