#!/bin/bash
set -e

# ATOM Intents Local Testnet - Stop Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Stopping ATOM Intents Local Testnet..."
docker-compose down

echo "Testnet stopped."
