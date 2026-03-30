# rust-proxy

A production-style Layer 7 reverse proxy built in Rust. Implements the core functionality found in systems like NGINX, Envoy, and Cloudflare's edge proxies: routing, load balancing, retries, health checking, rate limiting, metrics, and graceful lifecycle management.

Built with [tokio](https://tokio.rs/), [hyper](https://hyper.rs/), and [tower](https://github.com/tower-rs/tower).

## Features

- **Prefix-based routing** with longest-match selection and path rewriting
- **Load balancing** — round robin, least connections, weighted round robin, consistent hashing
- **Retries** with exponential backoff, jitter, and a sliding-window retry budget
- **Health checking** — passive (failure tracking) and active (background probes)
- **Rate limiting** — per-IP token bucket with automatic stale bucket cleanup
- **Prometheus metrics** — request count, latency histogram, retries, errors on a separate metrics server
- **Graceful shutdown** — SIGINT/SIGTERM handling, in-flight request draining with configurable timeout
- **Hot config reload** — SIGHUP triggers lock-free swap of routes and config via `arc-swap`
- **Header normalization** — `X-Forwarded-For/Proto/Host`, hop-by-hop header stripping, `X-Request-Id` injection
- **TLS upstream support** — proxy can connect to HTTPS backends
- **Tower middleware stack** — logging, rate limiting, and overall request timeout composed via `ServiceBuilder`

## Architecture

```
Client Request
      |
      v
 +-----------+
 | TCP Accept |  (hyper server)
 +-----------+
      |
      v
 +-------------------+
 | InjectAddrService  |  Extract client IP from connection
 +-------------------+
      |
      v
 +-------------------+
 | LoggingMiddleware   |  Log method, path, status, duration
 +-------------------+
      |
      v
 +-------------------+
 | RateLimitMiddleware |  Per-IP token bucket (429 if exceeded)
 +-------------------+
      |
      v
 +-------------------+
 | TimeoutMiddleware   |  Overall request timeout (504 if exceeded)
 +-------------------+
      |
      v
 +-------------------+
 | ProxyService        |  Core proxy logic:
 |                     |    1. Match route (longest prefix)
 |                     |    2. Select upstream (load balancer)
 |                     |    3. Forward request
 |                     |    4. Retry on failure/5xx/timeout
 +-------------------+
      |
      v
   Upstream
```

### Request lifecycle

1. Incoming request arrives at the hyper server
2. Tower middleware stack runs: logging, rate limiting, overall timeout
3. Router matches the request path to a route by longest prefix
4. The matched prefix is stripped from the path before forwarding
5. Load balancer selects an upstream from the route's pool
6. Request body is buffered (required for retries) and forwarded to the upstream
7. On failure, timeout, or 5xx response: retry with a different upstream (subject to retry budget and backoff)
8. Upstream response is returned to the client with hop-by-hop headers stripped

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)
- OpenSSL development headers (`libssl-dev` on Debian/Ubuntu, `openssl` via Homebrew on macOS)

### Build

```bash
cargo build --release
```

This produces two binaries:
- `target/release/rust-proxy` — the proxy itself
- `target/release/mock-upstream` — a test backend for load testing

### Configure

Create a `config.yaml` in the working directory:

```yaml
server:
  bind: "127.0.0.1:8080"
  metrics_bind: "127.0.0.1:9090"
  request_timeout_secs: 5
  max_retries: 2

routes:
  - prefix: "/api"
    upstream:
      - "http://127.0.0.1:3000"
      - "http://127.0.0.1:3001"
    strategy: round_robin
```

### Run

```bash
# From the directory containing config.yaml
cargo run --release

# Or directly
./target/release/rust-proxy
```

The proxy reads `config.yaml` from the current working directory on startup. There are no CLI arguments — all configuration is via the YAML file.

### Verify

```bash
# Send a request through the proxy
curl http://127.0.0.1:8080/api/health

# Check Prometheus metrics
curl http://127.0.0.1:9090/metrics
```

## Configuration Reference

```yaml
server:
  bind: "127.0.0.1:8080"           # Listen address for proxy traffic
  metrics_bind: "127.0.0.1:9090"   # Listen address for Prometheus metrics (optional)
  request_timeout_secs: 5          # Timeout for each individual upstream request
  overall_timeout_secs: 30         # Timeout for the entire request lifecycle including retries
  max_retries: 2                   # Number of retry attempts on failure/5xx/timeout
  drain_timeout_secs: 30           # Max time to wait for in-flight requests during shutdown

  # Passive health checking
  failure_threshold: 3             # Consecutive failures before marking upstream unhealthy
  health_cooldown_secs: 30         # Seconds before retrying an unhealthy upstream

  # Retry tuning
  retry_backoff_ms: 100            # Base delay for exponential backoff (delay = base * 2^attempt)
  retry_budget_percent: 20.0       # Max retries as a percentage of total requests in the window
  retry_budget_window_secs: 10     # Sliding window duration for retry budget tracking

  # Connection pool
  pool_idle_timeout_secs: 90       # Idle connection timeout
  pool_max_idle_per_host: 32       # Max idle connections per upstream host

  # Active health checking (optional — omit section to disable)
  health_check:
    path: "/health"                # Endpoint to probe on each upstream
    interval_secs: 10              # Probe frequency
    timeout_secs: 3                # Probe timeout

  # Rate limiting (optional — omit section to disable)
  rate_limit:
    requests_per_second: 100.0     # Token bucket refill rate per IP
    burst_size: 200                # Token bucket capacity per IP

routes:
  - prefix: "/api"                 # Route prefix (longest match wins)
    upstream:                      # Backend servers
      - "http://backend-1:8080"
      - "http://backend-2:8080"
      - "http://backend-3:8080"
    strategy: round_robin          # Load balancing strategy (see below)
    weights: [5, 3, 2]            # Weights for weighted_round_robin (optional)
```

### Load balancing strategies

| Strategy | Config value | Behavior |
|----------|-------------|----------|
| Round Robin | `round_robin` | Cycles through upstreams sequentially via atomic counter |
| Least Connections | `least_connections` | Routes to the upstream with the fewest active connections (atomic counters) |
| Weighted Round Robin | `weighted_round_robin` | Pre-computes a schedule from `weights` (e.g., `[3,1]` expands to `[A,A,A,B]`) |
| Consistent Hash | `consistent_hash` | Hashes the request path onto a ring with 150 virtual nodes per upstream. Same path always routes to the same backend |

### Hot reload

Send SIGHUP to the process to reload `config.yaml` without downtime:

```bash
kill -HUP $(pgrep rust-proxy)
```

The proxy atomically swaps the router and config using `arc-swap`. In-flight requests continue using the previous config; new requests pick up the updated routes.

## Running Tests

Integration tests spin up real HTTP servers on localhost — no mocks for the core request path.

```bash
# Run all tests
cargo test

# Run a specific test file
cargo test --test health
cargo test --test load_balancing
cargo test --test retry

# With stdout/stderr output
cargo test -- --nocapture
```

### Test suite

| Test file | What it covers |
|-----------|---------------|
| `retry.rs` | Retry on 5xx, failover to healthy upstream |
| `health.rs` | Passive failure tracking, active health check probes |
| `retry_budget.rs` | Sliding window retry budget enforcement |
| `metrics.rs` | Prometheus counter/histogram recording |
| `middleware.rs` | Overall timeout (504) via Tower middleware |
| `rate_limit.rs` | Token bucket 429 responses and Retry-After header |
| `graceful_shutdown.rs` | In-flight request completion during shutdown |
| `config_reload.rs` | Hot swap of upstreams via `apply_config` |
| `load_balancing.rs` | Weighted round robin distribution, least connections, consistent hashing |

## Performance Testing

### Micro-benchmarks (Criterion)

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark suite
cargo bench --bench router_bench
cargo bench --bench balancer_bench
```

HTML reports are generated in `target/criterion/`.

**Router benchmarks** (`router_bench`):
- `few_routes` — 5 routes, match a middle route
- `many_routes` — 50 routes, match route #45
- `no_match` — 10 routes, no matching prefix
- `deep_prefix` — 4 nested prefixes, match deepest

**Balancer benchmarks** (`balancer_bench`):
- `round_robin` — atomic counter increment (pool sizes 2, 5, 10)
- `least_connections` — min-search across pool (pool sizes 2, 5, 10)
- `weighted_round_robin` — schedule lookup (uniform, skewed, large weight sets)
- `consistent_hash` — ring binary search (pool sizes 2, 5, 10)

### Load testing

The project includes a shell-based load test script that runs five scenarios against the proxy with mock upstream servers.

**Prerequisites**: install either [wrk](https://github.com/wg/wrk) or [hey](https://github.com/rakyll/hey):

```bash
# macOS
brew install wrk
# or
brew install hey
```

**Run the load tests:**

```bash
./scripts/load_test.sh
```

The script:
1. Builds release binaries (`rust-proxy` and `mock-upstream`)
2. For each scenario: starts mock upstreams, writes a test config, starts the proxy, runs the load test, then tears everything down
3. Backs up and restores your `config.yaml`

**Scenarios:**

| # | Name | Setup | What it tests |
|---|------|-------|--------------|
| 1 | Baseline | 1 upstream, round robin | Single-backend throughput |
| 2 | Multi RR | 3 upstreams, round robin | Load distribution across backends |
| 3 | Weighted RR | 3 upstreams, weights 5:3:2 | Weighted distribution accuracy |
| 4 | Least Connections | 3 upstreams (1 slow at 50ms) | Adaptive routing under uneven latency |
| 5 | Consistent Hash | 3 upstreams, varied paths | Hash distribution and affinity |

**Mock upstream options:**

```bash
# Start a mock backend with custom behavior
cargo run --release --bin mock-upstream -- \
  --port 10001 \
  --delay-ms 50 \
  --body-size 1024 \
  --fail-rate 0.1
```

## Docker

### Build

```bash
docker build -t rust-proxy:latest .
```

The Dockerfile uses a multi-stage build: dependencies are cached in a separate layer so rebuilds after source changes are fast. The runtime image is `debian:bookworm-slim` and runs as a non-root `proxy` user.

### Run

```bash
docker run -p 8080:8080 -p 9090:9090 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  rust-proxy:latest
```

The container exposes port 8080 (proxy) and 9090 (metrics). It uses `SIGTERM` as the stop signal, which triggers graceful shutdown with in-flight request draining.

## Kubernetes

Manifests are in the `k8s/` directory.

### Deploy

```bash
kubectl apply -f k8s/
```

This creates three resources:

- **ConfigMap** (`rust-proxy-config`) — contains the proxy configuration. Edit `k8s/configmap.yaml` to set your upstream addresses, health check settings, and rate limits.
- **Deployment** (`rust-proxy`) — runs the proxy with liveness/readiness probes, resource limits, non-root security context, and a read-only root filesystem. `terminationGracePeriodSeconds` is set to 35 to give the proxy's 30-second drain timeout room to complete.
- **Service** (`rust-proxy`) — ClusterIP service exposing port 8080 (proxy) and 9090 (metrics). Includes Prometheus scrape annotations.

### Useful commands

```bash
# Check pod status
kubectl get pods -l app=rust-proxy

# View logs
kubectl logs -l app=rust-proxy -f

# Port-forward for local access
kubectl port-forward svc/rust-proxy 8080:8080

# Update config (edit configmap, then restart pods to pick up changes)
kubectl edit configmap rust-proxy-config
kubectl rollout restart deployment/rust-proxy

# Remove
kubectl delete -f k8s/
```

### Prometheus integration

The Service includes annotations for automatic Prometheus scraping:

```yaml
annotations:
  prometheus.io/scrape: "true"
  prometheus.io/port: "9090"
  prometheus.io/path: "/metrics"
```

If you're running Prometheus with the standard Kubernetes service discovery, metrics will be picked up automatically.

### Metrics exposed

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `proxy_requests_total` | Counter | method, route, status | Total requests processed |
| `proxy_request_duration_seconds` | Histogram | method, route | Request latency distribution |
| `proxy_retries_total` | Counter | route | Number of retry attempts |
| `proxy_errors_total` | Counter | route, error_type | Errors by type (5xx, timeout, connection) |

## Design Decisions

### Why hyper 0.14 instead of a higher-level framework?

The proxy needs precise control over connection pooling, request buffering, timeouts, and header manipulation. hyper gives us direct access to the HTTP layer without opinions about routing or middleware. Tower is used for the middleware stack, but the core proxy logic (retries, health-aware upstream selection) is kept in `proxy.rs` because it's tightly coupled and doesn't map cleanly to Tower's retry middleware.

### Why `arc-swap` for config reload?

Config reload needs to be lock-free on the request path. `arc-swap` gives us atomic pointer swaps — each request loads a snapshot of the current config/router with no contention. The SIGHUP handler swaps in a new `Arc<Config>` and `Arc<Router>`, and in-flight requests using the old config continue undisturbed.

### Why a retry budget instead of just max retries?

A per-request retry count (`max_retries`) limits individual requests, but under sustained failure it can still amplify load: if every request retries 2 times, the failing upstream sees 3x its normal traffic. The retry budget tracks retries as a percentage of total requests over a sliding window (default 20% over 10s). When the budget is exhausted, retries are suppressed system-wide, preventing cascading failures.

### Why two timeout layers?

`request_timeout_secs` is the per-upstream-attempt timeout — how long to wait for a single backend to respond. `overall_timeout_secs` is a Tower middleware that caps the entire request lifecycle including all retry attempts. Without it, a request with 3 retries and a 5-second timeout could take up to 15+ seconds (with backoff). The overall timeout provides a hard upper bound for client-facing latency.

### Why passive AND active health checks?

Passive health checking (tracking failures from real traffic) is reactive — it only detects a problem after requests have already failed. Active health checking (background probes) detects problems proactively, even for upstreams that aren't currently receiving traffic. Together they provide fast detection and recovery: active checks catch problems early, and passive checks provide an immediate circuit breaker on the request path.

### Why consistent hashing uses virtual nodes?

A naive hash ring with one node per upstream leads to uneven distribution. With 150 virtual nodes per upstream, the ring has many more points, so hash space is divided more uniformly. Binary search on the sorted ring keeps lookup at O(log n) regardless of the number of virtual nodes.

### Why no HTTP/2 server-side?

In real-world deployments, HTTP/2 termination is typically handled by an edge load balancer (CDN, cloud LB) that sits in front of the reverse proxy. The proxy itself communicates with backends over HTTP/1.1, which is simpler and avoids the complexity of HTTP/2 multiplexing in the proxy layer. The proxy does support HTTPS for upstream connections.

### Why buffer the request body?

Retries require re-sending the request body. Since hyper's request body is a stream that can only be consumed once, the proxy buffers it into bytes before the first attempt. This trades memory for retry capability — a reasonable tradeoff for an L7 proxy where request bodies are typically small.

## Project Structure

```
src/
  main.rs              Entrypoint — config loading, server startup, signal handling
  server.rs            Hyper server setup, metrics server, graceful shutdown
  proxy.rs             Core request handling — routing, forwarding, retries, header normalization
  router.rs            Prefix-based route matching (longest match)
  balancer.rs          LoadBalancer trait + 4 implementations (RR, LC, WRR, consistent hash)
  middleware.rs        Tower layers: logging, rate limiting, timeout, client IP injection
  config.rs            YAML config structures and defaults
  config_reloader.rs   SIGHUP listener, lock-free config swap via arc-swap
  state.rs             AppState — shared state across all request handlers
  health.rs            Passive health tracker (failure counting, cooldown recovery)
  health_check.rs      Active health check background task
  metrics.rs           Prometheus metric definitions and recording
  rate_limiter.rs      Per-IP token bucket rate limiter
  retry_budget.rs      Sliding window retry budget
  errors.rs            ProxyError enum and HTTP response conversion
  bin/
    mock_upstream.rs   Configurable mock backend for testing and benchmarks

tests/                 Integration tests (real HTTP servers, no mocks)
benches/               Criterion micro-benchmarks (router, all balancer strategies)
k8s/                   Kubernetes manifests (Deployment, Service, ConfigMap)
scripts/               Load testing script and scenario configs
```
