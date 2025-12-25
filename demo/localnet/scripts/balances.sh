#!/bin/bash

# Query balances on both chains

echo "Querying balances..."
echo ""

if [ -n "$1" ]; then
    # Query specific address
    ADDRESS=$1
    echo "Address: $ADDRESS"
    echo ""

    if [[ $ADDRESS == cosmos1* ]]; then
        echo "Cosmos Hub Balance:"
        curl -s "http://localhost:1317/cosmos/bank/v1beta1/balances/$ADDRESS" | jq '.balances'
    fi

    if [[ $ADDRESS == osmo1* ]]; then
        echo "Osmosis Balance:"
        curl -s "http://localhost:1318/cosmos/bank/v1beta1/balances/$ADDRESS" | jq '.balances'
    fi
else
    # Query all test accounts
    echo "Test Account Balances"
    echo "====================="
    echo ""

    # Get addresses from containers
    ALICE_COSMOS=$(docker exec atom-intents-hub gaiad keys show alice -a --keyring-backend test 2>/dev/null)
    BOB_COSMOS=$(docker exec atom-intents-hub gaiad keys show bob -a --keyring-backend test 2>/dev/null)
    ALICE_OSMO=$(docker exec atom-intents-osmosis osmosisd keys show alice -a --keyring-backend test 2>/dev/null)
    BOB_OSMO=$(docker exec atom-intents-osmosis osmosisd keys show bob -a --keyring-backend test 2>/dev/null)

    echo "Alice (Cosmos Hub): $ALICE_COSMOS"
    if [ -n "$ALICE_COSMOS" ]; then
        curl -s "http://localhost:1317/cosmos/bank/v1beta1/balances/$ALICE_COSMOS" | jq -r '.balances[] | "  \(.amount) \(.denom)"'
    fi
    echo ""

    echo "Alice (Osmosis): $ALICE_OSMO"
    if [ -n "$ALICE_OSMO" ]; then
        curl -s "http://localhost:1318/cosmos/bank/v1beta1/balances/$ALICE_OSMO" | jq -r '.balances[] | "  \(.amount) \(.denom)"'
    fi
    echo ""

    echo "Bob (Cosmos Hub): $BOB_COSMOS"
    if [ -n "$BOB_COSMOS" ]; then
        curl -s "http://localhost:1317/cosmos/bank/v1beta1/balances/$BOB_COSMOS" | jq -r '.balances[] | "  \(.amount) \(.denom)"'
    fi
    echo ""

    echo "Bob (Osmosis): $BOB_OSMO"
    if [ -n "$BOB_OSMO" ]; then
        curl -s "http://localhost:1318/cosmos/bank/v1beta1/balances/$BOB_OSMO" | jq -r '.balances[] | "  \(.amount) \(.denom)"'
    fi
fi
