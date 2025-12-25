#!/bin/bash
set -e

# ATOM Intents - Testnet Faucet Request Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

# Parse arguments
CHAIN=""
ADDRESS=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --chain)
            CHAIN="$2"
            shift 2
            ;;
        --address)
            ADDRESS="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 --chain <chain-id> --address <address>"
            echo ""
            echo "Supported chains:"
            echo "  theta-testnet-001  Cosmos Hub Testnet"
            echo "  osmo-test-5        Osmosis Testnet"
            echo "  pion-1             Neutron Testnet"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$CHAIN" ]] || [[ -z "$ADDRESS" ]]; then
    log_error "Both --chain and --address are required"
    exit 1
fi

log_info "Requesting testnet tokens"
log_info "  Chain: $CHAIN"
log_info "  Address: $ADDRESS"

case $CHAIN in
    theta-testnet-001)
        FAUCET_URL="https://faucet.polypore.xyz"
        log_info "Using Cosmos Hub Theta Testnet faucet"
        log_warn "Visit: $FAUCET_URL"
        log_warn "Manual request required for Cosmos Hub testnet"
        ;;
    osmo-test-5)
        FAUCET_URL="https://faucet.testnet.osmosis.zone"
        log_info "Requesting from Osmosis testnet faucet..."
        curl -X POST "$FAUCET_URL" \
            -H "Content-Type: application/json" \
            -d "{\"address\": \"$ADDRESS\"}" \
            2>/dev/null && log_info "Request sent!" || log_warn "Faucet request failed, try web interface: $FAUCET_URL"
        ;;
    pion-1)
        FAUCET_URL="https://faucet.pion-1.ntrn.tech"
        log_info "Requesting from Neutron testnet faucet..."
        curl -X POST "$FAUCET_URL/credit" \
            -H "Content-Type: application/json" \
            -d "{\"address\": \"$ADDRESS\", \"denom\": \"untrn\"}" \
            2>/dev/null && log_info "Request sent!" || log_warn "Faucet request failed, try web interface: $FAUCET_URL"
        ;;
    *)
        log_error "Unknown chain: $CHAIN"
        exit 1
        ;;
esac

echo ""
log_info "Note: Faucet requests may take a few minutes to process."
log_info "Check your balance after a few minutes."
