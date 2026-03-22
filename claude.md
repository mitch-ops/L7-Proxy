# Rust L7 Reverse Proxy (Systems Project)

## Overview

This project is a production-style Layer 7 reverse proxy built in Rust using:

- tokio (async runtime)
- hyper (HTTP server/client)
- serde (config)
- tracing (observability)

The goal is to simulate real-world infrastructure systems (similar to Cloudflare / Envoy / NGINX), not a toy project.

---

## Current Architecture

### Request Flow

1. Incoming HTTP request received by hyper server
2. `proxy_request` handler executes:
   - Generate request ID
   - Match route via router
   - Strip prefix from path
   - Select upstream via load balancer
   - Buffer request body (for retries)
   - Forward request to upstream
3. Retry loop:
   - Retries on:
     - connection failure
     - timeout
     - HTTP 5xx responses
   - Tries different upstreams deterministically
4. Return upstream response to client

---

## Core Components

### 1. Server (`server.rs`)
- Starts Hyper server
- Accepts `TcpListener` (supports ephemeral ports for tests)
- Spawns request handlers

### 2. Proxy (`proxy.rs`)
- Core request handling logic
- Handles:
  - routing
  - retries
  - request transformation
  - upstream forwarding

### 3. Router (`router.rs`)
- Prefix-based routing
- Matches incoming request path → route

### 4. Balancer (`balancer.rs`)
- Round-robin load balancing
- Deterministic retry ordering

### 5. State (`state.rs`)
Shared application state:
- router
- hyper client (connection pooling)
- config

### 6. Config (`config.rs`)
YAML-based configuration:
- server settings (timeouts, retries)
- routes (prefix + upstreams)

---

## Features Implemented ✅

### Core Proxy
- [x] HTTP server (hyper)
- [x] Request forwarding
- [x] Path rewriting (prefix stripping)
- [x] Header injection (`x-request-id`)

### Routing
- [x] Prefix-based routing
- [x] Multiple routes

### Load Balancing
- [x] Round-robin across upstreams

### Retries
- [x] Retry on:
  - connection failure
  - timeout
  - HTTP 5xx
- [x] Deterministic upstream rotation
- [x] Request body buffering (required for retries)
- [x] Exponential backoff with jitter
- [x] Retry budget (sliding window, configurable %)

### Observability
- [x] Structured logging with tracing
- [x] Request ID propagation
- [x] Prometheus metrics (request count, latency histogram, retries, errors)
- [x] Separate metrics server (configurable port)

### Config
- [x] YAML config parsing
- [x] Runtime-configured routes + upstreams

### Testing
- [x] Integration test with real upstream servers
- [x] Ephemeral port binding (no hardcoded ports)
- [x] End-to-end retry validation

---

## Current Limitations

- No health checking (dead upstreams still used) — ✅ Implemented
- No backoff strategy (retries are immediate) — ✅ Implemented
- No connection-level tuning
- No metrics (Prometheus, etc.) — ✅ Implemented
- No rate limiting — ✅ Implemented
- No middleware abstraction (tower not used yet) — ✅ Implemented
- No graceful shutdown — ✅ Implemented
- No TLS / HTTPS support
- No HTTP/2 support

---

## Roadmap (Next Features)

### 1. Retry Improvements ✅
- [x] Exponential backoff
- [x] Jitter
- [x] Retry budgets

---

### 2. Passive Health Checking
- Track failures per upstream
- Temporarily mark upstreams as unhealthy
- Skip unhealthy upstreams

---

### 3. Active Health Checks
- Background task periodically probes upstreams
- Reinstate healthy upstreams

---

### 4. Observability (Production-Level) ✅
- [x] Metrics (Prometheus)
  - request count (by method, route, status)
  - latency histogram
  - error rate (by route, error type)
  - retry count (by route)
- [x] Separate metrics server (configurable port)
- [ ] Structured spans (tracing)
- [ ] Request timing

---

### 5. Middleware Layer (Tower) ✅
- [x] Introduce `tower::Service` (`ProxyService`)
- [x] Logging middleware (method, path, status, duration)
- [x] Overall request timeout middleware (distinct from per-upstream timeout)
- [x] `ServiceBuilder` composition in server
- Retry logic remains in `proxy.rs` (tightly coupled with health/budget)

---

### 6. Rate Limiting ✅
- [x] Token bucket algorithm (per-IP)
- [x] Configurable requests_per_second and burst_size
- [x] Tower middleware (returns 429 + Retry-After header)
- [x] Client IP extracted from connection (AddrStream)
- [x] Periodic stale bucket cleanup

---

### 7. Configuration Reloading
- Hot reload config without restart

---

### 8. Graceful Shutdown ✅
- [x] SIGINT + SIGTERM signal handling
- [x] Drain in-flight requests via `with_graceful_shutdown`
- [x] Configurable drain timeout (force-stop if exceeded)
- [x] `start_proxy_with_shutdown` test helper

---

### 9. Advanced Load Balancing
- Least connections
- Weighted round robin
- Consistent hashing

---

### 10. HTTP Features
- Connection reuse tuning
- HTTP/2 support
- Header normalization

---

## Design Principles

- No unnecessary abstractions early
- Build incrementally, test each step
- Prefer explicit control over magic
- Match real-world proxy behavior
- Optimize for correctness before performance

---

## Testing Strategy

- Integration tests using real HTTP servers
- No mocks for core request flow
- Use ephemeral ports (port 0)
- Validate full request lifecycle

---

## Notes for AI Assistants (Claude Code)

- Do NOT rewrite architecture without reason
- Preserve:
  - retry semantics
  - request buffering logic
  - deterministic load balancing
- Prefer small, incremental changes
- Always keep code compiling
- Add tests for new features
- Avoid introducing heavy frameworks prematurely

---

## Goal

This project is meant to demonstrate:

- Systems-level thinking
- Networking fundamentals
- Production-grade backend design
- Rust async + ownership mastery

Target audience: infrastructure / platform engineering roles (e.g., Cloudflare, etc.)
