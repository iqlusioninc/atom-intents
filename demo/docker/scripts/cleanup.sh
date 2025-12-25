#!/bin/bash
set -e

# ATOM Intents Demo - Docker Cleanup Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_DIR="$(dirname "$SCRIPT_DIR")"

echo "🧹 Cleaning up ATOM Intents Demo..."

cd "$DOCKER_DIR"

# Stop and remove containers
docker-compose --profile full --profile monitoring down -v

# Remove images
echo "🗑️  Removing images..."
docker-compose --profile full --profile monitoring down --rmi local

echo "✅ Cleanup complete."
