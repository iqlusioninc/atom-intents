#!/bin/bash
set -e

# ATOM Intents Demo - Docker Start Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_DIR="$(dirname "$SCRIPT_DIR")"

echo "🚀 Starting ATOM Intents Demo..."

# Parse arguments
PROFILE=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --full)
            PROFILE="--profile full"
            shift
            ;;
        --monitoring)
            PROFILE="--profile monitoring"
            shift
            ;;
        --all)
            PROFILE="--profile full --profile monitoring"
            shift
            ;;
        *)
            shift
            ;;
    esac
done

cd "$DOCKER_DIR"

# Build and start containers
echo "📦 Building containers..."
docker-compose build

echo "🔧 Starting services..."
docker-compose up -d $PROFILE

echo ""
echo "✅ ATOM Intents Demo is running!"
echo ""
echo "📊 Access points:"
echo "   Web UI:        http://localhost:3000"
echo "   API:           http://localhost:8080"
echo "   Health Check:  http://localhost:8080/health"
echo ""

if [[ $PROFILE == *"monitoring"* ]]; then
    echo "📈 Monitoring:"
    echo "   Prometheus:    http://localhost:9092"
    echo "   Grafana:       http://localhost:3001 (admin/admin)"
    echo ""
fi

if [[ $PROFILE == *"full"* ]]; then
    echo "⛓️  Mock Chains:"
    echo "   Cosmos Hub:    http://localhost:26657"
    echo "   Osmosis:       http://localhost:26658"
    echo ""
fi

echo "📝 Useful commands:"
echo "   View logs:     docker-compose logs -f"
echo "   Stop demo:     ./scripts/stop.sh"
echo "   Clean up:      ./scripts/cleanup.sh"
