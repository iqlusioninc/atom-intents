#!/bin/bash
set -e

# ATOM Intents Local Testnet - Start Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

cd "$SCRIPT_DIR"

echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║           ATOM INTENTS LOCAL TESTNET                      ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

# Create directories
mkdir -p chains/hub chains/osmosis hermes/keys

# Add relayer keys for Hermes
log_step "Setting up relayer keys..."
cat > hermes/keys/relayer.json << 'EOF'
{
  "name": "relayer",
  "type": "local",
  "address": "",
  "pubkey": "",
  "mnemonic": "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon actual"
}
EOF

# Start the chains
log_step "Starting local chains..."
docker-compose up -d cosmoshub osmosis

# Wait for chains to be ready
log_info "Waiting for Cosmos Hub to be ready..."
for i in {1..60}; do
    if curl -s http://localhost:26657/status > /dev/null 2>&1; then
        log_info "Cosmos Hub is ready!"
        break
    fi
    sleep 2
done

log_info "Waiting for Osmosis to be ready..."
for i in {1..60}; do
    if curl -s http://localhost:26667/status > /dev/null 2>&1; then
        log_info "Osmosis is ready!"
        break
    fi
    sleep 2
done

# Start the relayer
log_step "Starting IBC relayer..."
docker-compose up -d hermes

# Wait for channel creation
log_info "Waiting for IBC channel creation..."
sleep 15

# Display status
echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║                   TESTNET READY                           ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

log_info "Cosmos Hub (localhub-1):"
echo "    RPC:  http://localhost:26657"
echo "    REST: http://localhost:1317"
echo "    gRPC: localhost:9090"
echo ""

log_info "Osmosis (localosmo-1):"
echo "    RPC:  http://localhost:26667"
echo "    REST: http://localhost:1318"
echo "    gRPC: localhost:9091"
echo ""

log_info "Test Accounts (same mnemonic prefix):"
echo "    alice   - 10,000 ATOM / 100,000 OSMO"
echo "    bob     - 10,000 ATOM / 100,000 OSMO"
echo "    solver1 - 50,000 ATOM / 500,000 OSMO"
echo "    solver2 - 50,000 ATOM / 500,000 OSMO"
echo ""

log_info "Commands:"
echo "    View logs:    docker-compose logs -f"
echo "    Stop:         ./stop.sh"
echo "    Reset:        ./reset.sh"
echo ""

# Check chain status
log_step "Chain Status:"
HUB_HEIGHT=$(curl -s http://localhost:26657/status | jq -r '.result.sync_info.latest_block_height' 2>/dev/null || echo "N/A")
OSMO_HEIGHT=$(curl -s http://localhost:26667/status | jq -r '.result.sync_info.latest_block_height' 2>/dev/null || echo "N/A")
echo "    Cosmos Hub: Block $HUB_HEIGHT"
echo "    Osmosis:    Block $OSMO_HEIGHT"
echo ""
