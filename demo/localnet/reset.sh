#!/bin/bash
set -e

# ATOM Intents Local Testnet - Reset Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Resetting ATOM Intents Local Testnet..."

# Stop containers
docker-compose down -v

# Remove chain data
rm -rf chains/hub/* chains/osmosis/*

echo "Testnet reset. Run ./start.sh to start fresh."
