#!/bin/bash
set -e

# IBC Transfer Script

if [ "$#" -lt 4 ]; then
    echo "Usage: $0 <source-chain> <from-key> <to-address> <amount>"
    echo ""
    echo "Examples:"
    echo "  $0 hub alice osmo1... 1000000uatom"
    echo "  $0 osmosis bob cosmos1... 1000000uosmo"
    exit 1
fi

SOURCE=$1
FROM=$2
TO=$3
AMOUNT=$4

case $SOURCE in
    hub|cosmoshub)
        CHAIN_ID="localhub-1"
        BINARY="gaiad"
        CONTAINER="atom-intents-hub"
        CHANNEL="channel-0"
        ;;
    osmosis|osmo)
        CHAIN_ID="localosmo-1"
        BINARY="osmosisd"
        CONTAINER="atom-intents-osmosis"
        CHANNEL="channel-0"
        ;;
    *)
        echo "Unknown chain: $SOURCE"
        exit 1
        ;;
esac

echo "Sending IBC transfer..."
echo "  From: $FROM on $CHAIN_ID"
echo "  To: $TO"
echo "  Amount: $AMOUNT"
echo "  Channel: $CHANNEL"

docker exec $CONTAINER $BINARY tx ibc-transfer transfer transfer $CHANNEL $TO $AMOUNT \
    --from $FROM \
    --chain-id $CHAIN_ID \
    --keyring-backend test \
    --gas auto \
    --gas-adjustment 1.3 \
    --gas-prices 0.025uatom \
    -y

echo "Transfer submitted. Check destination balance in a few seconds."
