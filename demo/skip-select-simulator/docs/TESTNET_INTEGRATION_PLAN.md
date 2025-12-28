# Testnet Integration Plan

## Overview

This document describes the testnet integration for the Skip Select Simulator demo, enabling real smart contract execution on Cosmos testnets instead of pure in-memory simulation.

## Problem Statement

The original demo system was a complete in-memory simulation:
- Generated fake transaction IDs
- No real blockchain calls
- No actual smart contract interaction
- Useful for UI demos but didn't exercise the real contracts

## Solution Architecture

### Backend Abstraction

We introduced an `ExecutionBackend` trait that abstracts the execution layer:

```
┌─────────────────────────────────────────────────────────────┐
│                     Demo Frontend (UI)                       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Settlement Processor                     │
│                  (src/settlement.rs)                        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   ExecutionBackend Trait                     │
│                    (src/backend/mod.rs)                     │
└─────────────────────────────────────────────────────────────┘
                    ╱                    ╲
                   ╱                      ╲
                  ▼                        ▼
┌─────────────────────────┐    ┌─────────────────────────────┐
│   SimulatedBackend      │    │      TestnetBackend         │
│ (In-memory, fake txs)   │    │ (Real chain interaction)    │
└─────────────────────────┘    └─────────────────────────────┘
                                           │
                    ┌──────────────────────┼──────────────────────┐
                    │                      │                      │
                    ▼                      ▼                      ▼
            ┌─────────────┐      ┌──────────────┐      ┌─────────────────┐
            │ RPC Client  │      │ gRPC Client  │      │ Wallet/Signer   │
            │ (JSON-RPC)  │      │ (tonic)      │      │ (secp256k1)     │
            └─────────────┘      └──────────────┘      └─────────────────┘
```

### Components Implemented

#### 1. Wallet & Signing (`src/backend/wallet.rs`)
- **CosmosWallet**: secp256k1 key management using k256
- Address derivation: SHA256 → RIPEMD160 → bech32
- Cross-chain address support (same key, different prefixes)
- Environment variable loading (`COSMOS_PRIVATE_KEY`)

#### 2. Transaction Builder (`src/backend/tx_builder.rs`)
- Cosmos SDK protobuf message encoding
- `MsgExecuteContract` construction for CosmWasm
- Proper SignDoc creation and signing
- Per-chain gas configuration

#### 3. Chain Clients (`src/backend/testnet.rs`, `src/backend/grpc_client.rs`)

**RPC Client (SimpleChainClient)**:
- Tendermint JSON-RPC for tx broadcast
- REST API for account queries
- Transaction confirmation polling

**gRPC Client (CosmosGrpcClient)**:
- Type-safe queries via cosmos-sdk-proto
- Account info, balances, block height
- Contract state queries
- Transaction broadcast and confirmation

#### 4. Configuration (`src/backend/config.rs`)
- TOML-based chain configuration
- Multi-chain support with IBC channel config
- Contract address management

#### 5. Settlement Flow Updates
- Intent data (user address, denoms) wired through to settlement
- Real contract calls when wallet is configured
- Fallback to simulation when no wallet available

## File Structure

```
demo/skip-select-simulator/
├── src/
│   ├── backend/
│   │   ├── mod.rs           # ExecutionBackend trait, BackendError, events
│   │   ├── config.rs        # TestnetConfig, ChainConfig
│   │   ├── simulated.rs     # SimulatedBackend (in-memory)
│   │   ├── testnet.rs       # TestnetBackend (real chains)
│   │   ├── grpc_client.rs   # CosmosGrpcClient (gRPC queries)
│   │   ├── tx_builder.rs    # Transaction building & signing
│   │   └── wallet.rs        # Key management & signing
│   ├── models.rs            # Settlement struct with intent fields
│   ├── auction.rs           # Populates settlement with intent data
│   └── settlement.rs        # Settlement processor with backend support
├── config/
│   └── testnet.toml         # Testnet chain configuration
├── scripts/
│   ├── generate-wallet.sh   # Shell script for wallet generation
│   ├── start-testnet.sh     # Start in testnet mode
│   └── start-localnet.sh    # Start in localnet mode
└── examples/
    └── generate_wallet.rs   # Rust wallet generator
```

## Usage

### 1. Generate a Wallet

```bash
# Using Rust example
cargo run --example generate_wallet

# Using shell script
./scripts/generate-wallet.sh
```

This creates:
- `.env.local` - Environment file with private key
- `wallet.key` - Raw private key file

### 2. Fund the Wallet

Use testnet faucets:
- Cosmos Hub: https://faucet.testnet.cosmos.network/
- Osmosis: https://faucet.testnet.osmosis.zone/
- Neutron: https://t.me/+SyhWrlnwfCw2NGM6

### 3. Run in Testnet Mode

```bash
# Load wallet and start
source .env.local
cargo run -- --mode testnet --config config/testnet.toml

# Or directly
COSMOS_PRIVATE_KEY=$(cat wallet.key) cargo run -- --mode testnet
```

### 4. Run in Simulated Mode (default)

```bash
cargo run -- --mode simulated
```

## Execution Modes

| Mode | Wallet Required | Chain Connection | Use Case |
|------|-----------------|------------------|----------|
| `simulated` | No | None | UI development, demos |
| `testnet` | Optional* | Real testnets | Contract testing |
| `localnet` | Optional* | Local nodes | Development |

*Without wallet, falls back to simulation with real chain height queries

## Configuration

### testnet.toml

```toml
primary_chain = "theta-testnet-001"
settlement_timeout_secs = 600

[contracts]
settlement_address = "cosmos14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s4hmalr"
escrow_address = "cosmos1nc5tatafv6eyq7llkr2gv50ff9e22mnf70qgjlv737ktmt4eswrqvp52rq"

[chains.theta-testnet-001]
chain_id = "theta-testnet-001"
rpc_url = "https://rpc.sentry-01.theta-testnet.polypore.xyz:443"
grpc_url = "https://grpc.sentry-01.theta-testnet.polypore.xyz:443"
gas_price = "0.025"
fee_denom = "uatom"
```

## Data Flow

### Intent → Settlement → Contract

```
1. User submits intent
   └── Intent { user_address, input: Asset, output: OutputSpec }

2. Auction matches intent with solver
   └── Creates Settlement with intent data

3. Settlement struct now includes:
   ├── user_address (from intent)
   ├── input_chain_id, input_denom (from intent.input)
   └── output_chain_id, output_denom (from intent.output)

4. TestnetBackend.execute_settlement()
   ├── Uses settlement.input_denom (not hardcoded)
   ├── Uses settlement.user_address
   └── Builds real contract message with intent data
```

## Remaining Work

### Completed ✓
- [x] ExecutionBackend trait abstraction
- [x] Wallet/key management (secp256k1)
- [x] Transaction builder (Cosmos SDK protobuf)
- [x] Account queries (REST + gRPC)
- [x] Transaction broadcast (RPC + gRPC)
- [x] Transaction confirmation polling
- [x] Intent data wired through settlement
- [x] CLI mode switching
- [x] Configuration system
- [x] Wallet generation tooling

### Future Enhancements
- [ ] IBC transfer execution (requires relayer)
- [ ] Endpoint fallback/failover
- [ ] Transaction simulation before broadcast
- [ ] Gas estimation
- [ ] Multi-signature support
- [ ] Hardware wallet support

## Security Considerations

1. **Private Keys**: Never commit `.env.local` or `wallet.key` files
2. **Testnet Only**: Current implementation is for testnets only
3. **No Mainnet**: Do not use generated keys on mainnet
4. **Rate Limits**: Public endpoints may have rate limits

## Chain Endpoints

### Current (Public Testnets)

| Chain | RPC | gRPC |
|-------|-----|------|
| Cosmos Hub Testnet | rpc.sentry-01.theta-testnet.polypore.xyz | grpc.sentry-01.theta-testnet.polypore.xyz |
| Osmosis Testnet | rpc.testnet.osmosis.zone | grpc.testnet.osmosis.zone |
| Neutron Testnet | rpc-palvus.pion-1.ntrn.tech | grpc-palvus.pion-1.ntrn.tech |

### Production Options

1. **Public Endpoints**: Free but may have rate limits
2. **Self-hosted Nodes**: Full control, requires infrastructure
3. **Node Providers**: QuickNode, NodeReal, etc.
4. **Chain-specific**: Polypore (Cosmos), Notional

## Testing

```bash
# Run all tests
cargo test

# Test wallet generation
cargo run --example generate_wallet

# Test with testnet (requires funded wallet)
source .env.local
cargo run -- --mode testnet
```

## Dependencies Added

```toml
k256 = { version = "0.13", features = ["ecdsa", "sha256"] }
bech32 = "0.11"
ripemd = "0.1"
prost = "0.12"
cosmos-sdk-proto = { version = "0.21", features = ["cosmwasm", "grpc"] }
tonic = { version = "0.11", features = ["tls", "tls-roots"] }
```
