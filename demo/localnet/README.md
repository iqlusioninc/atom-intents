# ATOM Intents Local Testnet

A local multi-chain testnet environment for realistic demonstration of the ATOM Intents system.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           LOCAL TESTNET                                  │
│                                                                          │
│  ┌──────────────┐         ┌──────────────┐         ┌──────────────┐    │
│  │  Cosmos Hub  │◄───────►│   Hermes     │◄───────►│   Osmosis    │    │
│  │  (gaiad)     │   IBC   │   Relayer    │   IBC   │  (osmosisd)  │    │
│  │              │         │              │         │              │    │
│  │  Chain:      │         │              │         │  Chain:      │    │
│  │  localhub-1  │         │              │         │  localosmo-1 │    │
│  │              │         │              │         │              │    │
│  │  RPC: 26657  │         │              │         │  RPC: 26667  │    │
│  │  gRPC: 9090  │         │              │         │  gRPC: 9091  │    │
│  │  REST: 1317  │         │              │         │  REST: 1318  │    │
│  └──────────────┘         └──────────────┘         └──────────────┘    │
│         │                                                   │           │
│         │                 ┌──────────────┐                 │           │
│         └────────────────►│  Settlement  │◄────────────────┘           │
│                           │  Contract    │                              │
│                           └──────────────┘                              │
└─────────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        SKIP SELECT SIMULATOR                             │
│                    (Connected to real local chains)                      │
└─────────────────────────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Start the local testnet
./start.sh

# This will:
# 1. Start Cosmos Hub local node
# 2. Start Osmosis local node
# 3. Configure IBC channels
# 4. Start Hermes relayer
# 5. Fund test accounts
# 6. Deploy settlement contracts
```

## Test Accounts

Pre-funded accounts for testing:

| Name | Address | Hub Balance | Osmosis Balance |
|------|---------|-------------|-----------------|
| alice | cosmos1... | 10,000 ATOM | 100,000 OSMO |
| bob | cosmos1... | 10,000 ATOM | 100,000 OSMO |
| solver1 | cosmos1... | 50,000 ATOM | 500,000 OSMO |
| solver2 | cosmos1... | 50,000 ATOM | 500,000 OSMO |

## IBC Channels

| Source | Destination | Channel |
|--------|-------------|---------|
| localhub-1 | localosmo-1 | channel-0 |
| localosmo-1 | localhub-1 | channel-0 |

## Endpoints

### Cosmos Hub (localhub-1)
- RPC: http://localhost:26657
- REST: http://localhost:1317
- gRPC: localhost:9090

### Osmosis (localosmo-1)
- RPC: http://localhost:26667
- REST: http://localhost:1318
- gRPC: localhost:9091

## Commands

```bash
# Check Hub status
./scripts/hub-status.sh

# Check Osmosis status
./scripts/osmo-status.sh

# Send IBC transfer
./scripts/ibc-transfer.sh <from> <to> <amount> <denom>

# Query balances
./scripts/balances.sh <address>

# Stop all nodes
./stop.sh

# Reset to genesis
./reset.sh
```

## Development

### Adding a new chain

1. Add chain config to `chains/`
2. Update `docker-compose.yml`
3. Add IBC channel config to `hermes/config.toml`
4. Update start script

### Modifying genesis

Edit the genesis files in `genesis/` and run `./reset.sh`.
