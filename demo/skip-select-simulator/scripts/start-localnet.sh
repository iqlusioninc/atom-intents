#!/bin/bash
# Start Skip Select Simulator in Localnet Mode
#
# This script starts the demo server connected to local docker chains.
# Requires the localnet docker-compose to be running.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
LOCALNET_DIR="$PROJECT_DIR/../localnet"
PORT="${PORT:-8080}"

echo "=========================================="
echo "  Skip Select Simulator - Localnet Mode"
echo "=========================================="
echo ""

# Check if localnet is running
echo "Checking localnet status..."

LOCALNET_RUNNING=true

# Check local Cosmos Hub
if curl -s --max-time 2 "http://localhost:26657/status" > /dev/null 2>&1; then
    HEIGHT=$(curl -s "http://localhost:26657/status" | grep -o '"latest_block_height":"[0-9]*"' | cut -d'"' -f4)
    echo "  ✓ Local Cosmos Hub (localhub-1) - Height: $HEIGHT"
else
    echo "  ✗ Local Cosmos Hub - Not running"
    LOCALNET_RUNNING=false
fi

# Check local Osmosis
if curl -s --max-time 2 "http://localhost:26667/status" > /dev/null 2>&1; then
    HEIGHT=$(curl -s "http://localhost:26667/status" | grep -o '"latest_block_height":"[0-9]*"' | cut -d'"' -f4)
    echo "  ✓ Local Osmosis (localosmo-1) - Height: $HEIGHT"
else
    echo "  ✗ Local Osmosis - Not running"
    LOCALNET_RUNNING=false
fi

echo ""

if [ "$LOCALNET_RUNNING" = false ]; then
    echo "Localnet is not running. Starting it now..."
    echo ""

    if [ -d "$LOCALNET_DIR" ] && [ -f "$LOCALNET_DIR/docker-compose.yml" ]; then
        cd "$LOCALNET_DIR"
        docker-compose up -d

        echo ""
        echo "Waiting for chains to start..."
        sleep 10

        # Verify they're running now
        if curl -s --max-time 5 "http://localhost:26657/status" > /dev/null 2>&1; then
            echo "  ✓ Localnet is now running"
        else
            echo "  ✗ Failed to start localnet"
            echo ""
            echo "Please start localnet manually:"
            echo "  cd $LOCALNET_DIR && docker-compose up -d"
            exit 1
        fi
    else
        echo "Localnet directory not found: $LOCALNET_DIR"
        echo ""
        echo "Please ensure the localnet is set up and running:"
        echo "  cd ../localnet && docker-compose up -d"
        echo ""
        echo "Or start in simulated mode instead:"
        echo "  cargo run -- --mode simulated"
        exit 1
    fi
fi

# Build if needed
echo "Building project..."
cd "$PROJECT_DIR"
cargo build --release 2>&1 | tail -5

echo ""
echo "Starting Skip Select Simulator..."
echo "=========================================="
echo ""

# Start the server
exec cargo run --release -- \
    --mode localnet \
    --port "$PORT"
