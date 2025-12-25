#!/bin/bash
set -e

# ATOM Intents Demo - Docker Stop Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_DIR="$(dirname "$SCRIPT_DIR")"

echo "🛑 Stopping ATOM Intents Demo..."

cd "$DOCKER_DIR"

docker-compose --profile full --profile monitoring down

echo "✅ Demo stopped."
