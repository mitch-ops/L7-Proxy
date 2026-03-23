FROM rust:1.85-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies: copy manifests and build with a dummy source
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    echo 'fn main() {}' > src/main.rs && \
    mkdir -p src/bin && \
    echo 'fn main() {}' > src/bin/mock_upstream.rs && \
    cargo build --release --bin rust-proxy 2>/dev/null || true && \
    rm -rf src

# Copy real source and build
COPY src/ src/
COPY benches/ benches/
RUN touch src/main.rs && cargo build --release --bin rust-proxy

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r proxy && useradd -r -g proxy -d /app proxy

WORKDIR /app

COPY --from=builder /build/target/release/rust-proxy /app/rust-proxy

RUN chown -R proxy:proxy /app
USER proxy

EXPOSE 8080 9090
STOPSIGNAL SIGTERM

ENTRYPOINT ["/app/rust-proxy"]
