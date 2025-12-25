# ATOM Intents Demo System

A complete end-to-end simulation, demonstration, and testnet system for the ATOM Intent-Based Liquidity System. This demo can be launched on Google Cloud or run locally.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           WEB INTERFACE                                  │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │  Intent Creator  │  Live Auction View  │  Solver Dashboard         │  │
│  │  Trade History   │  Price Charts       │  Settlement Monitor       │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ REST/WebSocket
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    SKIP SELECT SIMULATOR                                 │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────────┐    │
│  │  REST API  │  │  WebSocket │  │  Matching  │  │  Mock Oracle   │    │
│  │  /intents  │  │  /ws       │  │  Engine    │  │  Price Feeds   │    │
│  └────────────┘  └────────────┘  └────────────┘  └────────────────┘    │
│                           │                                              │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────────┐    │
│  │  Batch     │  │  Quote     │  │  Settlement │  │  Analytics    │    │
│  │  Auction   │  │  Aggregator│  │  Simulator  │  │  Engine       │    │
│  └────────────┘  └────────────┘  └────────────┘  └────────────────┘    │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │
        ┌────────────────────────┼────────────────────────┐
        │                        │                        │
        ▼                        ▼                        ▼
┌───────────────┐      ┌───────────────┐      ┌───────────────┐
│  Mock Solver  │      │  Mock Solver  │      │  Mock Solver  │
│  (DEX Router) │      │  (Intent)     │      │  (CEX)        │
└───────────────┘      └───────────────┘      └───────────────┘
        │                        │                        │
        └────────────────────────┼────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    SIMULATED BLOCKCHAIN LAYER                            │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────────┐    │
│  │  Mock Hub  │  │  Mock      │  │  Mock IBC  │  │  Settlement    │    │
│  │  Chain     │  │  Osmosis   │  │  Relayer   │  │  Contract      │    │
│  └────────────┘  └────────────┘  └────────────┘  └────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. Skip Select Simulator (`skip-select-simulator/`)
A simplified Rust implementation of the Skip Select coordination layer:
- REST API for submitting intents
- WebSocket for real-time updates
- Batch auction engine
- Mock oracle price feeds
- Settlement simulation

### 2. Web Interface (`web-ui/`)
React-based dashboard with:
- Intent creation wizard
- Live auction visualization
- Solver quote comparison
- Settlement tracking
- Analytics dashboard

### 3. Docker Infrastructure (`docker/`)
Complete containerized environment:
- Docker Compose for local development
- Mock blockchain nodes
- Pre-configured solver instances

### 4. Google Cloud Deployment (`gcloud/`)
Production-ready GCP infrastructure:
- Terraform modules for GKE
- Cloud Run for stateless services
- Cloud SQL for persistence
- Load balancing and SSL

### 5. Simulation Framework (`simulation/`)
Tools for testing and demonstration:
- Intent generators
- Solver simulators
- Market condition simulators
- Performance benchmarks

### 6. Testnet Integration (`testnet/`)
Scripts for deploying to real testnets:
- Cosmos Hub testnet (theta)
- Osmosis testnet
- Neutron testnet

## Quick Start

### Local Development
```bash
# Build and run everything locally
cd demo
docker-compose up

# Access the web UI
open http://localhost:3000

# Access the API
curl http://localhost:8080/api/v1/health
```

### Google Cloud Deployment
```bash
# Set up GCP project
cd demo/gcloud
./setup.sh --project=your-gcp-project

# Deploy infrastructure
terraform init
terraform apply

# Deploy services
./deploy.sh
```

### Testnet Mode
```bash
# Deploy to Cosmos testnet
cd demo/testnet
./deploy-testnet.sh --chain=theta-testnet-001
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `SKIP_SELECT_PORT` | API server port | 8080 |
| `WEBSOCKET_PORT` | WebSocket server port | 8081 |
| `AUCTION_INTERVAL_MS` | Batch auction interval | 500 |
| `MOCK_LATENCY_MS` | Simulated network latency | 100 |
| `ENABLE_ANALYTICS` | Enable analytics collection | true |
| `GCP_PROJECT_ID` | Google Cloud project ID | - |
| `GCP_REGION` | Deployment region | us-central1 |

## API Reference

### REST Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/intents` | POST | Submit a new intent |
| `/api/v1/intents/{id}` | GET | Get intent status |
| `/api/v1/auctions/current` | GET | Get current auction state |
| `/api/v1/auctions/{id}/quotes` | GET | Get solver quotes |
| `/api/v1/settlements/{id}` | GET | Get settlement status |
| `/api/v1/prices` | GET | Get current price feeds |
| `/api/v1/solvers` | GET | List active solvers |

### WebSocket Events

| Event | Direction | Description |
|-------|-----------|-------------|
| `intent.submitted` | server→client | New intent received |
| `auction.started` | server→client | Batch auction started |
| `quote.received` | server→client | New solver quote |
| `auction.completed` | server→client | Auction completed |
| `settlement.update` | server→client | Settlement status change |

## Demo Scenarios

### Scenario 1: Simple Swap
Demonstrates a basic ATOM→OSMO swap with DEX routing.

### Scenario 2: Intent Matching
Shows two users with opposing intents being matched directly.

### Scenario 3: Multi-Hop Settlement
Demonstrates cross-chain settlement via IBC.

### Scenario 4: CEX Backstop
Shows fallback to CEX liquidity when DEX is insufficient.

### Scenario 5: Auction Competition
Multiple solvers competing for the best execution price.

## License

Apache 2.0 - See [LICENSE](../LICENSE)
