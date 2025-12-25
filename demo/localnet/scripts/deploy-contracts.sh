#!/bin/bash
set -e

# Deploy ATOM Intents Contracts to Local Testnet

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$(dirname "$SCRIPT_DIR")")")"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_info "Building contracts..."

# Build contracts using CosmWasm optimizer
cd "$PROJECT_ROOT/contracts/settlement"
if [ ! -f "artifacts/settlement.wasm" ]; then
    docker run --rm -v "$(pwd)":/code \
        --mount type=volume,source="settlement_cache",target=/code/target \
        --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
        cosmwasm/rust-optimizer:0.15.0
fi

cd "$PROJECT_ROOT/contracts/escrow"
if [ ! -f "artifacts/escrow.wasm" ]; then
    docker run --rm -v "$(pwd)":/code \
        --mount type=volume,source="escrow_cache",target=/code/target \
        --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
        cosmwasm/rust-optimizer:0.15.0
fi

log_info "Uploading contracts to local hub..."

# Copy WASM files to container
docker cp "$PROJECT_ROOT/contracts/settlement/artifacts/settlement.wasm" atom-intents-hub:/tmp/
docker cp "$PROJECT_ROOT/contracts/escrow/artifacts/escrow.wasm" atom-intents-hub:/tmp/

# Store settlement contract
log_info "Storing settlement contract..."
SETTLEMENT_STORE_TX=$(docker exec atom-intents-hub gaiad tx wasm store /tmp/settlement.wasm \
    --from validator \
    --chain-id localhub-1 \
    --keyring-backend test \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices 0.025uatom \
    -y \
    --output json)

sleep 5

SETTLEMENT_CODE_ID=$(docker exec atom-intents-hub gaiad query wasm list-code --output json | jq -r '.code_infos[-1].code_id')
log_info "Settlement contract code ID: $SETTLEMENT_CODE_ID"

# Store escrow contract
log_info "Storing escrow contract..."
docker exec atom-intents-hub gaiad tx wasm store /tmp/escrow.wasm \
    --from validator \
    --chain-id localhub-1 \
    --keyring-backend test \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices 0.025uatom \
    -y

sleep 5

ESCROW_CODE_ID=$(docker exec atom-intents-hub gaiad query wasm list-code --output json | jq -r '.code_infos[-1].code_id')
log_info "Escrow contract code ID: $ESCROW_CODE_ID"

# Get validator address
VALIDATOR_ADDR=$(docker exec atom-intents-hub gaiad keys show validator -a --keyring-backend test)

# Instantiate settlement contract
log_info "Instantiating settlement contract..."
SETTLEMENT_INIT="{\"admin\":\"$VALIDATOR_ADDR\",\"min_solver_stake\":\"1000000\",\"settlement_timeout\":300,\"max_price_deviation_bps\":500}"

docker exec atom-intents-hub gaiad tx wasm instantiate $SETTLEMENT_CODE_ID "$SETTLEMENT_INIT" \
    --from validator \
    --label "atom-intents-settlement" \
    --admin $VALIDATOR_ADDR \
    --chain-id localhub-1 \
    --keyring-backend test \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices 0.025uatom \
    -y

sleep 5

SETTLEMENT_CONTRACT=$(docker exec atom-intents-hub gaiad query wasm list-contract-by-code $SETTLEMENT_CODE_ID --output json | jq -r '.contracts[-1]')
log_info "Settlement contract address: $SETTLEMENT_CONTRACT"

# Instantiate escrow contract
log_info "Instantiating escrow contract..."
ESCROW_INIT="{\"admin\":\"$VALIDATOR_ADDR\",\"settlement_contract\":\"$SETTLEMENT_CONTRACT\",\"escrow_timeout\":300}"

docker exec atom-intents-hub gaiad tx wasm instantiate $ESCROW_CODE_ID "$ESCROW_INIT" \
    --from validator \
    --label "atom-intents-escrow" \
    --admin $VALIDATOR_ADDR \
    --chain-id localhub-1 \
    --keyring-backend test \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices 0.025uatom \
    -y

sleep 5

ESCROW_CONTRACT=$(docker exec atom-intents-hub gaiad query wasm list-contract-by-code $ESCROW_CODE_ID --output json | jq -r '.contracts[-1]')
log_info "Escrow contract address: $ESCROW_CONTRACT"

# Save deployment info
cat > "$SCRIPT_DIR/../deployment.json" << EOF
{
  "chain_id": "localhub-1",
  "contracts": {
    "settlement": {
      "code_id": $SETTLEMENT_CODE_ID,
      "address": "$SETTLEMENT_CONTRACT"
    },
    "escrow": {
      "code_id": $ESCROW_CODE_ID,
      "address": "$ESCROW_CONTRACT"
    }
  }
}
EOF

echo ""
log_info "Contracts deployed successfully!"
echo ""
echo "Settlement Contract: $SETTLEMENT_CONTRACT"
echo "Escrow Contract: $ESCROW_CONTRACT"
echo ""
echo "Deployment info saved to deployment.json"
