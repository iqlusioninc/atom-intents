# CLAUDE.md - ATOM Intents Project Guide

## Project Overview

ATOM Intents is an intent-based trading system for Cosmos Hub enabling fast cross-chain trading with zero solver capital requirements for DEX routing. It achieves 2-5 second execution (vs 6-30s traditional) and CEX-competitive pricing.

**Repository:** https://github.com/iqlusioninc/atom-intents
**License:** Apache 2.0

## Quick Reference

### Build Commands

```bash
# Build entire workspace
cargo build

# Build with release optimizations
cargo build --release

# Check without building (faster)
cargo check

# Build CosmWasm contracts
cargo build --release -p atom-intents-settlement-contract
cargo build --release -p atom-intents-escrow
```

### Test Commands

```bash
# Run all tests
cargo test

# Test specific crate
cargo test -p atom-intents-types
cargo test -p atom-intents-solver
cargo test -p atom-intents-settlement
cargo test -p atom-intents-matching-engine
cargo test -p atom-intents-orchestrator

# Run with output
cargo test -- --nocapture
```

### Code Quality

```bash
cargo fmt        # Format code
cargo clippy     # Lint
```

### Web UI (demo/web-ui/)

```bash
cd demo/web-ui
npm install
npm run dev         # Dev server
npm run build       # Production build
npm run typecheck   # Type checking
```

### Docker / Local Testnet

```bash
cd demo/localnet && docker-compose up    # Local testnet
cd demo/docker && docker-compose up      # Docker demo
```

## Architecture

```
User Layer (Keplr, wallets, dApps)
         ↓
Coordination Layer (Go Fast Simulator - intent ordering, auction, routing)
         ↓
Settlement Layer (IBC, escrow contract, two-phase commit)
```

### Key Crates

| Crate | Purpose |
|-------|---------|
| `crates/types/` | Core domain types (intents, fills, solutions) |
| `crates/solver/` | Solver framework and implementations (DEX, CEX, matching) |
| `crates/matching-engine/` | Order book and batch auction matching |
| `crates/settlement/` | IBC settlement with two-phase commit |
| `crates/relayer/` | Solver-integrated IBC packet relaying |
| `crates/orchestrator/` | Main service orchestration and execution |
| `crates/metrics/` | Prometheus monitoring |
| `crates/config/` | Configuration management |

### Smart Contracts

| Contract | Location | Purpose |
|----------|----------|---------|
| Escrow | `contracts/escrow/` | Locks user funds, verifies delivery via IBC acks |
| Settlement | `contracts/settlement/` | Two-phase commit state machine, fund release |

## Code Conventions

### Rust

- **Error handling:** Use `thiserror` for custom errors, `anyhow` for context
- **Async:** `tokio` runtime, `async-trait` for trait async methods
- **Types:** Strong typing with newtype patterns (e.g., `Amount`, `Price`)
- **Serialization:** `serde` with JSON, `#[cw_serde]` for contracts
- **Module layout:** `lib.rs` exports public API, `error.rs` for errors

### CosmWasm Contracts

- Entry points in `contract.rs`
- State via `cw-storage-plus`
- Always validate inputs before state changes
- Use `#[cw_serde]` for message types

### Testing

- Unit tests in same file with `#[cfg(test)]`
- Integration tests in `tests/` directory
- Contract tests in `contracts/*/tests/`

## Key Documentation

- `docs/SPECIFICATION.md` - Complete technical spec (80+ sections)
- `docs/OPERATIONS_GUIDE.md` - Deployment and operations
- `docs/GO_FAST_RELATIONSHIP.md` - Skip Protocol integration
- `docs/ESCROW_SETTLEMENT_FLOW.md` - Two-phase settlement mechanics
- `docs/plans/` - Implementation plans for upcoming features

## Important Patterns

### Two-Phase Commit Settlement

1. Phase 1: Both parties lock funds in escrow
2. Phase 2: Delivery verification via IBC ack → fund release
3. Timeout: Automatic refund

### Solver Competition

- Multiple solvers quoted in parallel
- Best solution selected by auction
- Reputation system tracks performance

### Three-Tier Liquidity Sourcing

1. Intent matching (zero capital) - P2P crossing
2. DEX routing (zero capital) - Osmosis/Astroport execution
3. CEX backstop (~$50k buffer) - Fallback only

## Environment & Deployment

- **Local:** Docker Compose in `demo/localnet/`
- **Testnet:** Cosmos Hub theta, Osmosis, Neutron
- **GCP:** Kubernetes configs in `demo/gcp/`

### Config Files

- `.env` files for local config (not versioned)
- Hermes relayer configs in `demo/localnet/hermes/`
- Terraform in `demo/gcp/terraform/`

## Current Development

Active work on:
- IBC Eureka integration (Ethereum support)
- NEAR Intents cross-ecosystem extension
- Solver bond redesign with per-settlement locking
