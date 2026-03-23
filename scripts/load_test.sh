#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG_DIR="$SCRIPT_DIR/configs"
PROXY_BIN="$PROJECT_DIR/target/release/rust-proxy"
UPSTREAM_BIN="$PROJECT_DIR/target/release/mock-upstream"

# Test parameters
DURATION="10s"
CONCURRENCY=100
THREADS=4

PIDS=()
ORIGINAL_CONFIG_SAVED=false

cleanup() {
    echo ""
    echo "Cleaning up..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    PIDS=()
    # Restore original config.yaml if we backed it up
    if [ "$ORIGINAL_CONFIG_SAVED" = true ] && [ -f "$PROJECT_DIR/config.yaml.bak" ]; then
        mv "$PROJECT_DIR/config.yaml.bak" "$PROJECT_DIR/config.yaml"
    elif [ "$ORIGINAL_CONFIG_SAVED" = false ]; then
        rm -f "$PROJECT_DIR/config.yaml"
    fi
}

trap cleanup EXIT

# --- Tool detection ---
TOOL=""
if command -v wrk &>/dev/null; then
    TOOL="wrk"
elif command -v hey &>/dev/null; then
    TOOL="hey"
else
    echo "ERROR: Neither 'wrk' nor 'hey' found."
    echo "Install one of them:"
    echo "  brew install wrk"
    echo "  brew install hey"
    exit 1
fi
echo "Using load test tool: $TOOL"

# --- Build ---
echo ""
echo "Building release binaries..."
cargo build --release --bin rust-proxy --bin mock-upstream 2>&1 | tail -1

# --- Helpers ---

start_upstream() {
    local port=$1
    shift
    "$UPSTREAM_BIN" --port "$port" "$@" &
    PIDS+=($!)
}

start_proxy() {
    local config_file=$1
    # Back up existing config.yaml on first call
    if [ "$ORIGINAL_CONFIG_SAVED" = false ] && [ -f "$PROJECT_DIR/config.yaml" ]; then
        cp "$PROJECT_DIR/config.yaml" "$PROJECT_DIR/config.yaml.bak"
        ORIGINAL_CONFIG_SAVED=true
    fi
    # Copy test config to config.yaml (proxy reads from cwd)
    cp "$config_file" "$PROJECT_DIR/config.yaml"
    (cd "$PROJECT_DIR" && "$PROXY_BIN" 2>/dev/null || true) &
    PIDS+=($!)
}

wait_for_ready() {
    local url=$1
    local max_wait=5
    local elapsed=0
    while ! curl -sf "$url" >/dev/null 2>&1; do
        sleep 0.2
        elapsed=$(echo "$elapsed + 0.2" | bc)
        if (( $(echo "$elapsed >= $max_wait" | bc -l) )); then
            echo "  WARNING: Server at $url not ready after ${max_wait}s"
            return 1
        fi
    done
    return 0
}

run_load_test() {
    local url=$1
    local extra_args=${2:-}

    if [ "$TOOL" = "wrk" ]; then
        wrk -t"$THREADS" -c"$CONCURRENCY" -d"$DURATION" $extra_args "$url"
    else
        hey -z "$DURATION" -c "$CONCURRENCY" "$url"
    fi
}

kill_scenario() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    PIDS=()
    sleep 0.5
}

separator() {
    echo ""
    echo "================================================================"
    echo ""
}

# ================================================================
# SCENARIOS
# ================================================================

separator
echo "SCENARIO 1: Baseline (single upstream, round robin)"
echo "----------------------------------------------------------------"
start_upstream 10001
start_proxy "$CONFIG_DIR/single_upstream.yaml"
sleep 1
wait_for_ready "http://127.0.0.1:10001/health"
wait_for_ready "http://127.0.0.1:8080/api/test" || true
echo ""
run_load_test "http://127.0.0.1:8080/api/test"
kill_scenario

separator
echo "SCENARIO 2: Multi-upstream round robin (3 upstreams)"
echo "----------------------------------------------------------------"
start_upstream 10001
start_upstream 10002
start_upstream 10003
start_proxy "$CONFIG_DIR/multi_upstream_rr.yaml"
sleep 1
wait_for_ready "http://127.0.0.1:10001/health"
wait_for_ready "http://127.0.0.1:8080/api/test" || true
echo ""
run_load_test "http://127.0.0.1:8080/api/test"
kill_scenario

separator
echo "SCENARIO 3: Weighted round robin (3 upstreams, weights 5:3:2)"
echo "----------------------------------------------------------------"
start_upstream 10001
start_upstream 10002
start_upstream 10003
start_proxy "$CONFIG_DIR/multi_upstream_wrr.yaml"
sleep 1
wait_for_ready "http://127.0.0.1:10001/health"
wait_for_ready "http://127.0.0.1:8080/api/test" || true
echo ""
run_load_test "http://127.0.0.1:8080/api/test"
kill_scenario

separator
echo "SCENARIO 4: Least connections (1 slow upstream at 50ms delay)"
echo "----------------------------------------------------------------"
start_upstream 10001
start_upstream 10002
start_upstream 10003 --delay-ms 50
start_proxy "$CONFIG_DIR/multi_upstream_lc.yaml"
sleep 1
wait_for_ready "http://127.0.0.1:10001/health"
wait_for_ready "http://127.0.0.1:8080/api/test" || true
echo ""
run_load_test "http://127.0.0.1:8080/api/test"
kill_scenario

separator
echo "SCENARIO 5: Consistent hash (3 upstreams, varied paths)"
echo "----------------------------------------------------------------"
start_upstream 10001
start_upstream 10002
start_upstream 10003
start_proxy "$CONFIG_DIR/multi_upstream_ch.yaml"
sleep 1
wait_for_ready "http://127.0.0.1:10001/health"
wait_for_ready "http://127.0.0.1:8080/api/item/1" || true
echo ""
if [ "$TOOL" = "wrk" ]; then
    run_load_test "http://127.0.0.1:8080/api/item/1" "-s $SCRIPT_DIR/varied_paths.lua"
else
    # hey doesn't support varied paths natively, use a fixed path
    run_load_test "http://127.0.0.1:8080/api/item/42"
fi
kill_scenario

separator
echo "All scenarios complete."
