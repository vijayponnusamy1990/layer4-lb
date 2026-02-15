#!/bin/bash
set -e

# Get script location
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/.."
CA_CERT="$PROJECT_ROOT/certs/ca.crt"

echo "Running Bandwidth Limit Test..."
echo "Target Limit: 5 MB/s"
echo "Payload: 1 MB per request (to hit limit easily)"

# We use 1MB data size (-d 1048576). 
# With 10 clients, if unrestricted, this would do hundreds of MB/s.
# With limit, it should throttle to ~5 requests per second total (5 * 1MB = 5MB).

# Kill any existing instance
pkill -f "layer4-lb" || true

# Build and Start LB in background
echo "Building and Starting Layer 4 LB..."
cargo build --release
RUST_LOG=info ./target/release/layer4-lb --config "$PROJECT_ROOT/lb_redis.yaml" &
LB_PID=$!

# Wait for startup
sleep 5

echo "Running Bandwidth Limit Test..."
echo "Target Limit: 10 MB/s"
echo "Payload: 1 MB per request (to hit limit easily)"

# We use 1MB data size (-d 1048576). 
# With 10 clients, if unrestricted, this would do hundreds of MB/s.
# With limit, it should throttle to ~10 requests per second total (10 * 1MB = 10MB).

memtier_benchmark \
    --server=127.0.0.1 \
    --port=8080 \
    --data-size=1048576 \
    --test-time=10 \
    --clients=1 \
    --threads=1 \
    --hide-histogram || true

# Cleanup
kill $LB_PID

echo "Check the 'Totals' line above."
echo "If limiting is working, 'KB/sec' should be around 10240.00 (10 MB/s)."
