#!/bin/bash
set -e

# ATOM Intents - Testnet Contract Deployment Script

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
SKIP_BUILD=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --chain)
            CHAIN="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --help)
            echo "Usage: $0 [--chain <chain-id>] [--skip-build]"
            echo ""
            echo "Options:"
            echo "  --chain      Chain ID to deploy to (default: theta-testnet-001)"
            echo "  --skip-build Skip contract compilation"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Set chain-specific variables
case $CHAIN in
    theta-testnet-001)
        RPC_URL="$COSMOS_TESTNET_RPC"
        GAS_PRICES="$COSMOS_TESTNET_GAS_PRICES"
        BINARY="gaiad"
        ;;
    osmo-test-5)
        RPC_URL="$OSMOSIS_TESTNET_RPC"
        GAS_PRICES="$OSMOSIS_TESTNET_GAS_PRICES"
        BINARY="osmosisd"
        ;;
    pion-1)
        RPC_URL="$NEUTRON_TESTNET_RPC"
        GAS_PRICES="$NEUTRON_TESTNET_GAS_PRICES"
        BINARY="neutrond"
        ;;
    *)
        log_error "Unknown chain: $CHAIN"
        exit 1
        ;;
esac

log_info "Deploying contracts to $CHAIN"
log_info "RPC: $RPC_URL"

# Check if mnemonic is set
if [[ -z "$DEPLOYER_MNEMONIC" ]]; then
    log_error "DEPLOYER_MNEMONIC not set in testnet.env"
    exit 1
fi

# Build contracts
if [[ "$SKIP_BUILD" != "true" ]]; then
    log_info "Building contracts..."
    cd "$PROJECT_ROOT"

    # Build settlement contract
    log_info "Building settlement contract..."
    cd contracts/settlement
    cargo build --release --target wasm32-unknown-unknown
    docker run --rm -v "$(pwd)":/code \
        --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target \
        --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
        cosmwasm/rust-optimizer:0.15.0

    # Build escrow contract
    log_info "Building escrow contract..."
    cd ../escrow
    cargo build --release --target wasm32-unknown-unknown
    docker run --rm -v "$(pwd)":/code \
        --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target \
        --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
        cosmwasm/rust-optimizer:0.15.0

    cd "$SCRIPT_DIR"
fi

# Create temp keyring
KEYRING_DIR=$(mktemp -d)
trap "rm -rf $KEYRING_DIR" EXIT

# Import deployer key
log_info "Importing deployer key..."
echo "$DEPLOYER_MNEMONIC" | $BINARY keys add deployer --recover --keyring-backend test --home "$KEYRING_DIR"
DEPLOYER_ADDRESS=$($BINARY keys show deployer -a --keyring-backend test --home "$KEYRING_DIR")
log_info "Deployer address: $DEPLOYER_ADDRESS"

# Check balance
log_info "Checking balance..."
BALANCE=$($BINARY query bank balances "$DEPLOYER_ADDRESS" --node "$RPC_URL" -o json 2>/dev/null || echo '{"balances":[]}')
log_info "Balance: $BALANCE"

# Store contracts
log_info "Storing settlement contract..."
SETTLEMENT_WASM="$PROJECT_ROOT/contracts/settlement/artifacts/settlement.wasm"
if [[ ! -f "$SETTLEMENT_WASM" ]]; then
    log_error "Settlement contract WASM not found at $SETTLEMENT_WASM"
    exit 1
fi

STORE_SETTLEMENT_TX=$($BINARY tx wasm store "$SETTLEMENT_WASM" \
    --from deployer \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices "$GAS_PRICES" \
    --chain-id "$CHAIN" \
    --node "$RPC_URL" \
    --keyring-backend test \
    --home "$KEYRING_DIR" \
    -y \
    -o json)

SETTLEMENT_TX_HASH=$(echo "$STORE_SETTLEMENT_TX" | jq -r '.txhash')
log_info "Settlement store tx: $SETTLEMENT_TX_HASH"

# Wait for tx to be included
sleep 10

SETTLEMENT_CODE_ID=$($BINARY query tx "$SETTLEMENT_TX_HASH" --node "$RPC_URL" -o json | \
    jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')
log_info "Settlement code ID: $SETTLEMENT_CODE_ID"

# Store escrow contract
log_info "Storing escrow contract..."
ESCROW_WASM="$PROJECT_ROOT/contracts/escrow/artifacts/escrow.wasm"
if [[ ! -f "$ESCROW_WASM" ]]; then
    log_error "Escrow contract WASM not found at $ESCROW_WASM"
    exit 1
fi

STORE_ESCROW_TX=$($BINARY tx wasm store "$ESCROW_WASM" \
    --from deployer \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices "$GAS_PRICES" \
    --chain-id "$CHAIN" \
    --node "$RPC_URL" \
    --keyring-backend test \
    --home "$KEYRING_DIR" \
    -y \
    -o json)

ESCROW_TX_HASH=$(echo "$STORE_ESCROW_TX" | jq -r '.txhash')
log_info "Escrow store tx: $ESCROW_TX_HASH"

# Wait for tx
sleep 10

ESCROW_CODE_ID=$($BINARY query tx "$ESCROW_TX_HASH" --node "$RPC_URL" -o json | \
    jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')
log_info "Escrow code ID: $ESCROW_CODE_ID"

# Instantiate settlement contract
log_info "Instantiating settlement contract..."
SETTLEMENT_INIT_MSG=$(cat <<EOF
{
  "admin": "$DEPLOYER_ADDRESS",
  "min_solver_stake": "1000000",
  "settlement_timeout": 300,
  "max_price_deviation_bps": 500
}
EOF
)

INSTANTIATE_SETTLEMENT_TX=$($BINARY tx wasm instantiate "$SETTLEMENT_CODE_ID" "$SETTLEMENT_INIT_MSG" \
    --from deployer \
    --label "atom-intents-settlement" \
    --admin "$DEPLOYER_ADDRESS" \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices "$GAS_PRICES" \
    --chain-id "$CHAIN" \
    --node "$RPC_URL" \
    --keyring-backend test \
    --home "$KEYRING_DIR" \
    -y \
    -o json)

SETTLEMENT_INST_TX_HASH=$(echo "$INSTANTIATE_SETTLEMENT_TX" | jq -r '.txhash')
log_info "Settlement instantiate tx: $SETTLEMENT_INST_TX_HASH"

sleep 10

SETTLEMENT_CONTRACT_ADDRESS=$($BINARY query tx "$SETTLEMENT_INST_TX_HASH" --node "$RPC_URL" -o json | \
    jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')
log_info "Settlement contract address: $SETTLEMENT_CONTRACT_ADDRESS"

# Instantiate escrow contract
log_info "Instantiating escrow contract..."
ESCROW_INIT_MSG=$(cat <<EOF
{
  "admin": "$DEPLOYER_ADDRESS",
  "settlement_contract": "$SETTLEMENT_CONTRACT_ADDRESS",
  "escrow_timeout": 300
}
EOF
)

INSTANTIATE_ESCROW_TX=$($BINARY tx wasm instantiate "$ESCROW_CODE_ID" "$ESCROW_INIT_MSG" \
    --from deployer \
    --label "atom-intents-escrow" \
    --admin "$DEPLOYER_ADDRESS" \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices "$GAS_PRICES" \
    --chain-id "$CHAIN" \
    --node "$RPC_URL" \
    --keyring-backend test \
    --home "$KEYRING_DIR" \
    -y \
    -o json)

ESCROW_INST_TX_HASH=$(echo "$INSTANTIATE_ESCROW_TX" | jq -r '.txhash')
log_info "Escrow instantiate tx: $ESCROW_INST_TX_HASH"

sleep 10

ESCROW_CONTRACT_ADDRESS=$($BINARY query tx "$ESCROW_INST_TX_HASH" --node "$RPC_URL" -o json | \
    jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')
log_info "Escrow contract address: $ESCROW_CONTRACT_ADDRESS"

# Save deployment info
DEPLOYMENT_FILE="$SCRIPT_DIR/deployments/${CHAIN}_$(date +%Y%m%d_%H%M%S).json"
mkdir -p "$SCRIPT_DIR/deployments"

cat > "$DEPLOYMENT_FILE" <<EOF
{
  "chain_id": "$CHAIN",
  "rpc_url": "$RPC_URL",
  "deployer": "$DEPLOYER_ADDRESS",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "contracts": {
    "settlement": {
      "code_id": $SETTLEMENT_CODE_ID,
      "address": "$SETTLEMENT_CONTRACT_ADDRESS"
    },
    "escrow": {
      "code_id": $ESCROW_CODE_ID,
      "address": "$ESCROW_CONTRACT_ADDRESS"
    }
  }
}
EOF

echo ""
log_info "=========================================="
log_info "Deployment Complete!"
log_info "=========================================="
echo ""
log_info "Chain: $CHAIN"
log_info "Settlement Contract: $SETTLEMENT_CONTRACT_ADDRESS"
log_info "Escrow Contract: $ESCROW_CONTRACT_ADDRESS"
log_info "Deployment saved to: $DEPLOYMENT_FILE"
echo ""
log_info "Update testnet.env with:"
log_info "  SETTLEMENT_CONTRACT_ADDRESS=$SETTLEMENT_CONTRACT_ADDRESS"
log_info "  ESCROW_CONTRACT_ADDRESS=$ESCROW_CONTRACT_ADDRESS"
