#!/bin/bash
# Start Skip Select Simulator in Testnet Mode
#
# This script starts the demo server connected to real Cosmos testnets.
# It verifies connectivity before starting and provides helpful error messages.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CONFIG_FILE="${1:-$PROJECT_DIR/config/testnet.toml}"
PORT="${PORT:-8080}"

echo "=========================================="
echo "  Skip Select Simulator - Testnet Mode"
echo "=========================================="
echo ""

# Check if config file exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo "Error: Config file not found: $CONFIG_FILE"
    echo ""
    echo "Usage: $0 [config-file]"
    echo ""
    echo "Example:"
    echo "  $0                              # Use default config/testnet.toml"
    echo "  $0 /path/to/custom-config.toml  # Use custom config"
    echo ""
    exit 1
fi

echo "Configuration: $CONFIG_FILE"
echo "Port: $PORT"
echo ""

# Parse and display contract addresses from config
if command -v grep &> /dev/null && command -v sed &> /dev/null; then
    SETTLEMENT=$(grep 'settlement_address' "$CONFIG_FILE" | head -1 | sed 's/.*= *"\([^"]*\)".*/\1/')
    ESCROW=$(grep 'escrow_address' "$CONFIG_FILE" | head -1 | sed 's/.*= *"\([^"]*\)".*/\1/')

    echo "Contracts:"
    echo "  Settlement: $SETTLEMENT"
    echo "  Escrow: $ESCROW"
    echo ""
fi

# Check network connectivity to testnet RPCs
echo "Checking network connectivity..."

# Test Cosmos Hub testnet
if curl -s --max-time 5 "https://rpc.sentry-01.theta-testnet.polypore.xyz/status" > /dev/null 2>&1; then
    echo "  ✓ Cosmos Hub testnet (theta-testnet-001) - Connected"
else
    echo "  ✗ Cosmos Hub testnet - Not reachable"
    echo ""
    echo "Warning: Could not connect to Cosmos Hub testnet."
    echo "The demo will still start but may have limited functionality."
fi

# Test Osmosis testnet
if curl -s --max-time 5 "https://rpc.testnet.osmosis.zone/status" > /dev/null 2>&1; then
    echo "  ✓ Osmosis testnet (osmo-test-5) - Connected"
else
    echo "  ✗ Osmosis testnet - Not reachable"
fi

echo ""

# Check if contracts are deployed (placeholder check)
if [ "$SETTLEMENT" = "cosmos14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s4hmalr" ]; then
    echo "Note: Using placeholder contract addresses."
    echo "      For real testnet execution, deploy contracts first:"
    echo "      cd ../testnet && ./deploy-contracts.sh"
    echo ""
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
    --mode testnet \
    --config "$CONFIG_FILE" \
    --port "$PORT"
