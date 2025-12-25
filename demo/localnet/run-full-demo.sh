#!/bin/bash
set -e

# ATOM Intents - Full Demo with Local Testnet

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEMO_DIR="$(dirname "$SCRIPT_DIR")"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║        ATOM INTENTS - FULL DEMO WITH LOCAL TESTNET        ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Start local testnet
log_step "1. Starting local testnet..."
cd "$SCRIPT_DIR"
./start.sh

# Step 2: Wait for chains
log_step "2. Waiting for chains to be ready..."
sleep 10

# Step 3: Deploy contracts (if available)
if [ -f "$DEMO_DIR/../contracts/settlement/Cargo.toml" ]; then
    log_step "3. Deploying contracts..."
    ./scripts/deploy-contracts.sh
else
    log_info "Skipping contract deployment (contracts not found)"
fi

# Step 4: Start Skip Select Simulator connected to local chains
log_step "4. Starting Skip Select Simulator..."
export COSMOS_RPC_URL="http://localhost:26657"
export OSMOSIS_RPC_URL="http://localhost:26667"
export USE_LOCAL_TESTNET=true

# Check if settlement contract was deployed
if [ -f "$SCRIPT_DIR/deployment.json" ]; then
    export SETTLEMENT_CONTRACT=$(jq -r '.contracts.settlement.address' "$SCRIPT_DIR/deployment.json")
    export ESCROW_CONTRACT=$(jq -r '.contracts.escrow.address' "$SCRIPT_DIR/deployment.json")
fi

cd "$DEMO_DIR/skip-select-simulator"
cargo run &
SIMULATOR_PID=$!

# Step 5: Start Web UI
log_step "5. Starting Web UI..."
cd "$DEMO_DIR/web-ui"
npm install > /dev/null 2>&1 || true
npm run dev &
WEBUI_PID=$!

# Cleanup handler
cleanup() {
    echo ""
    log_info "Shutting down..."
    kill $SIMULATOR_PID $WEBUI_PID 2>/dev/null || true
    cd "$SCRIPT_DIR"
    ./stop.sh
}
trap cleanup EXIT

# Display info
echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║                    DEMO RUNNING                           ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""
log_info "Web UI:        http://localhost:3000"
log_info "API:           http://localhost:8080"
log_info "Cosmos Hub:    http://localhost:26657"
log_info "Osmosis:       http://localhost:26667"
echo ""
log_info "Test with real IBC:"
echo "    1. Connect wallet in Web UI"
echo "    2. Create an intent (ATOM → OSMO)"
echo "    3. Watch the auction and settlement"
echo "    4. Verify tokens arrived via IBC"
echo ""
log_info "Press Ctrl+C to stop"
echo ""

# Wait
wait
