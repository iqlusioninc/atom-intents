# ATOM Intent-Based Liquidity System

An intent-based trading system for Cosmos Hub that achieves:

- **2-5 second execution** (vs 6-30s traditional)
- **Near-zero solver capital** (vs $500k+ traditional market makers)
- **CEX-competitive pricing** (within 0.1-0.5% of Binance)
- **Robust IBC infrastructure** (solver-incentivized relayers)

**[Read the full specification](docs/SPECIFICATION.md)**

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                USER LAYER                                    │
│         Cosmos Wallets              Skip Go App              Integrations    │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            COORDINATION LAYER                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                           SKIP SELECT                                │   │
│  │  REST API    │  WebSocket   │   Matching   │   Auction              │   │
│  │  (Users)     │  (Solvers)   │   Engine     │   Engine               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│         │                              │                              │      │
│         ▼                              ▼                              ▼      │
│  ┌─────────────────┐          ┌─────────────────┐          ┌─────────────┐ │
│  │ Intent Matching │          │  DEX Routing    │          │ CEX Backstop│ │
│  │ Solver          │          │  Solver         │          │ Solver      │ │
│  │ Zero capital    │          │  Zero capital   │          │ ~$50k buffer│ │
│  └─────────────────┘          └─────────────────┘          └─────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            SETTLEMENT LAYER                                  │
│                          COSMOS HUB + IBC                                   │
│  Settlement Contract  │  Solver Registry  │  Escrow Contract                │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Project Structure

```
atom-intents/
├── crates/
│   ├── types/              # Core types (Intent, Asset, FillConfig, etc.)
│   ├── solver/             # Solver trait and DEX routing solver
│   ├── matching-engine/    # Order book and matching engine
│   ├── settlement/         # IBC settlement flows and two-phase commit
│   └── relayer/            # Solver-integrated relayer
├── contracts/
│   ├── escrow/             # CosmWasm escrow contract
│   └── settlement/         # CosmWasm settlement contract
└── docs/
    └── SPECIFICATION.md    # Complete technical specification
```

## Core Innovations

| Problem | Traditional Solution | Our Solution |
|---------|---------------------|--------------|
| Liquidity provision | Pre-positioned inventory ($500k+) | JIT solver execution (zero capital) |
| Cross-chain settlement | Manual multi-step bridging | Atomic IBC with wasm hooks |
| Relayer reliability | Altruistic public relayers | Solver-integrated relayers |
| Price discovery | Fragmented DEX liquidity | Intent matching + aggregation |
| Partial fills | Rare/unsupported | Native throughout |

## Liquidity Source Hierarchy

1. **Intent Matching** (zero capital) - Direct crossing of opposing user orders
2. **DEX Routing** (zero capital) - Aggregating existing Cosmos AMM pools
3. **CEX Backstop** (~$50k buffer) - Minimal inventory hedging against CEXes

## Building

```bash
cargo build
cargo test
```

## License

Apache-2.0
