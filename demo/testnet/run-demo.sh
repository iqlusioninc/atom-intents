#!/bin/bash
set -e

# ATOM Intents - Testnet Demo Runner

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Load environment
if [[ -f "$SCRIPT_DIR/testnet.env" ]]; then
    source "$SCRIPT_DIR/testnet.env"
else
    log_error "testnet.env not found. Copy testnet.env.example to testnet.env and configure."
    exit 1
fi

# Parse arguments
CHAIN="theta-testnet-001"
MODE="local"

while [[ $# -gt 0 ]]; do
    case $1 in
        --chain)
            CHAIN="$2"
            shift 2
            ;;
        --mode)
            MODE="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--chain <chain-id>] [--mode <local|docker>]"
            echo ""
            echo "Options:"
            echo "  --chain  Chain ID for testnet (default: theta-testnet-001)"
            echo "  --mode   Run mode: local or docker (default: local)"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Verify contracts are deployed
if [[ -z "$SETTLEMENT_CONTRACT_ADDRESS" ]] || [[ -z "$ESCROW_CONTRACT_ADDRESS" ]]; then
    log_error "Contracts not deployed. Run deploy-contracts.sh first."
    exit 1
fi

log_info "Starting ATOM Intents Demo"
log_info "  Chain: $CHAIN"
log_info "  Mode: $MODE"
log_info "  Settlement: $SETTLEMENT_CONTRACT_ADDRESS"
log_info "  Escrow: $ESCROW_CONTRACT_ADDRESS"

# Set testnet-specific environment
export TESTNET_MODE=true
export TESTNET_CHAIN_ID="$CHAIN"
export SETTLEMENT_CONTRACT="$SETTLEMENT_CONTRACT_ADDRESS"
export ESCROW_CONTRACT="$ESCROW_CONTRACT_ADDRESS"

case $CHAIN in
    theta-testnet-001)
        export RPC_URL="$COSMOS_TESTNET_RPC"
        export GAS_PRICES="$COSMOS_TESTNET_GAS_PRICES"
        ;;
    osmo-test-5)
        export RPC_URL="$OSMOSIS_TESTNET_RPC"
        export GAS_PRICES="$OSMOSIS_TESTNET_GAS_PRICES"
        ;;
    pion-1)
        export RPC_URL="$NEUTRON_TESTNET_RPC"
        export GAS_PRICES="$NEUTRON_TESTNET_GAS_PRICES"
        ;;
esac

if [[ "$MODE" == "docker" ]]; then
    log_info "Starting with Docker..."
    cd "$PROJECT_ROOT/demo/docker"

    # Export additional env vars for docker-compose
    export TESTNET_RPC_URL="$RPC_URL"

    docker-compose -f docker-compose.yml up -d
else
    log_info "Starting local development server..."

    # Start Skip Select Simulator
    cd "$PROJECT_ROOT/demo/skip-select-simulator"
    cargo run &
    SKIP_SELECT_PID=$!

    # Start Web UI
    cd "$PROJECT_ROOT/demo/web-ui"
    npm run dev &
    WEB_UI_PID=$!

    # Trap to cleanup on exit
    trap "kill $SKIP_SELECT_PID $WEB_UI_PID 2>/dev/null" EXIT

    log_info "Demo running!"
    log_info "  Web UI: http://localhost:3000"
    log_info "  API: http://localhost:8080"
    log_info ""
    log_info "Press Ctrl+C to stop"

    wait
fi
