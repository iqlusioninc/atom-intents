# Cosmos Hub Intent-Based Liquidity System

## Complete Technical Specification 

---

# Executive Summary

This document specifies an intent-based trading system for Cosmos Hub that achieves:

- **2-5 second execution** (vs 6-30s traditional)
- **Near-zero solver capital** (vs $500k+ traditional market makers)
- **CEX-competitive pricing** (within 0.1-0.5% of Binance)
- **Robust IBC infrastructure** (solver-incentivized relayers)

## Core Innovations

| Problem | Traditional Solution | Our Solution |
|---------|---------------------|--------------|
| Liquidity provision | Pre-positioned inventory ($500k+) | JIT solver execution (zero capital) |
| Cross-chain settlement | Manual multi-step bridging | Atomic IBC with wasm hooks |
| Relayer reliability | Altruistic public relayers | Solver-integrated relayers |
| Price discovery | Fragmented DEX liquidity | Intent matching + aggregation |
| Partial fills | Rare/unsupported | Native throughout |

## Key Metrics

| Metric | Traditional DEX | This System |
|--------|-----------------|-------------|
| User latency | 6-30 seconds | 2-5 seconds |
| Solver capital | $500k+ | ~$50k buffer |
| Price vs CEX | -1 to -3% | -0.1 to -0.5% |
| Partial fills | Rare | Native |
| IBC reliability | Best-effort | Economically guaranteed |

---

# Table of Contents

## Part I: Architecture
1. [System Overview](#1-system-overview)
2. [Design Principles](#2-design-principles)

## Part II: Intent Layer
3. [Intent Specification](#3-intent-specification)
4. [Partial Fill Support](#4-partial-fill-support)

## Part III: Coordination Layer
5. [Skip Select Integration](#5-skip-select-integration)
6. [Matching Engine](#6-matching-engine)
7. [Auction Mechanism](#7-auction-mechanism)

## Part IV: Solver Layer
8. [Solver Framework](#8-solver-framework)
9. [DEX Routing Solver](#9-dex-routing-solver)
10. [CEX Backstop Solver](#10-cex-backstop-solver)
11. [Solution Aggregation](#11-solution-aggregation)

## Part V: Settlement Layer
12. [IBC Settlement Flows](#12-ibc-settlement-flows)
13. [Packet Forward Middleware](#13-packet-forward-middleware)
14. [IBC Hooks & Wasm Execution](#14-ibc-hooks--wasm-execution)
15. [Timeout & Recovery](#15-timeout--recovery)

## Part VI: Relayer Economics
16. [The Relayer Problem](#16-the-relayer-problem)
17. [Solver-Integrated Relayers](#17-solver-integrated-relayers)
18. [Two-Phase Commit Settlement](#18-two-phase-commit-settlement)
19. [Relayer as Profit Center](#19-relayer-as-profit-center)

## Part VII: Security & Economics
20. [Security Model](#20-security-model)
21. [Economic Analysis](#21-economic-analysis)
22. [Failure Modes & Recovery](#22-failure-modes--recovery)

## Part VIII: Extensions
23. [Cross-Ecosystem Module (NEAR)](#23-cross-ecosystem-module-near)

## Part IX: Implementation
24. [Implementation Roadmap](#24-implementation-roadmap)

---

# Part I: Architecture

---

# 1. System Overview

## 1.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                USER LAYER                                        │
│                                                                                  │
│         Cosmos Wallets              Skip Go App              Integrations        │
│         (Keplr, Leap)               (Web, Mobile)            (dApps)            │
│              │                           │                        │              │
└──────────────┴───────────────────────────┴────────────────────────┴──────────────┘
                                           │
                                           ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            COORDINATION LAYER                                    │
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │                           SKIP SELECT                                    │   │
│  │                                                                          │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  │   │
│  │  │  REST API    │  │  WebSocket   │  │   Matching   │  │   Auction   │  │   │
│  │  │  (Users)     │  │  (Solvers)   │  │   Engine     │  │   Engine    │  │   │
│  │  └──────────────┘  └──────────────┘  └──────────────┘  └─────────────┘  │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                        │                                         │
│         ┌──────────────────────────────┼──────────────────────────────┐         │
│         │                              │                              │         │
│         ▼                              ▼                              ▼         │
│  ┌─────────────────┐          ┌─────────────────┐          ┌─────────────────┐ │
│  │ Intent Matching │          │  DEX Routing    │          │  CEX Backstop   │ │
│  │ Solver          │          │  Solver         │          │  Solver         │ │
│  │                 │          │                 │          │                 │ │
│  │ ┌─────────────┐ │          │ ┌─────────────┐ │          │ ┌─────────────┐ │ │
│  │ │  Relayer    │ │          │ │  Relayer    │ │          │ │  Relayer    │ │ │
│  │ └─────────────┘ │          │ └─────────────┘ │          │ └─────────────┘ │ │
│  │                 │          │                 │          │                 │ │
│  │ Zero capital    │          │ Zero capital    │          │ ~$50k buffer    │ │
│  └─────────────────┘          └─────────────────┘          └─────────────────┘ │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
                                           │
                                           ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            SETTLEMENT LAYER                                      │
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │                          COSMOS HUB                                      │   │
│  │                                                                          │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐         │   │
│  │  │  Settlement     │  │  Solver         │  │  Escrow         │         │   │
│  │  │  Contract       │  │  Registry       │  │  Contract       │         │   │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────┘         │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                           │                                      │
│              ┌────────────────────────────┼────────────────────────────┐        │
│              │                            │                            │        │
│              ▼                            ▼                            ▼        │
│  ┌───────────────────┐       ┌───────────────────┐       ┌───────────────────┐ │
│  │      Noble        │       │     Osmosis       │       │     Neutron       │ │
│  │      (USDC)       │       │     (DEX)         │       │     (DeFi)        │ │
│  └───────────────────┘       └───────────────────┘       └───────────────────┘ │
│                                                                                  │
│                    IBC / Packet Forward Middleware / Wasm Hooks                 │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## 1.2 Execution Flow Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         INTENT LIFECYCLE (2-5 seconds)                           │
└─────────────────────────────────────────────────────────────────────────────────┘

T+0ms       User signs intent
               │
               ▼
T+50ms      Skip Select receives
               │
               ├──────────────────────┐
               │                      │
               ▼                      ▼
T+100ms     Matching Engine       Solver Network
            checks book           receives intent
               │                      │
               ▼                      ▼
T+200ms     Cross with            Solvers submit
            opposing intents      competitive quotes
               │                      │
               └──────────┬───────────┘
                          │
                          ▼
T+500ms     Auction selects best combination
               │
               ▼
T+1000ms    Settlement bundle built
               │
               ▼
T+2000ms    On-chain execution
               │
               ├── Same chain: Bank transfer
               │
               ├── Adjacent chain: Direct IBC
               │
               └── Multi-hop: PFM + Wasm hooks
                          │
                          ▼
T+3000ms    Solver relayer prioritizes packets
               │
               ▼
T+5000ms    User receives funds ✓
```

## 1.3 Liquidity Source Hierarchy

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         LIQUIDITY SOURCE PRIORITY                                │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  PRIORITY 1: Intent Matching (Zero Capital)                                     │
│  ────────────────────────────────────────────                                   │
│  Cross opposing user intents directly                                           │
│  • Best prices (no AMM slippage, no solver spread)                              │
│  • Instant execution (single block)                                             │
│  • Limited by two-sided flow                                                    │
│  • Expected: 20-40% of volume                                                   │
│                                                                                  │
│  PRIORITY 2: DEX Routing (Zero Capital)                                         │
│  ──────────────────────────────────────────                                     │
│  Route through existing Cosmos AMM pools                                        │
│  • Osmosis, Astroport, Neutron pools                                            │
│  • Aggregated via Skip Go for best route                                        │
│  • Subject to AMM slippage on large orders                                      │
│  • Expected: 40-60% of volume                                                   │
│                                                                                  │
│  PRIORITY 3: CEX Backstop (Minimal Capital)                                     │
│  ─────────────────────────────────────────────                                  │
│  Hedge against centralized exchanges                                            │
│  • ~$50k inventory buffer per solver                                            │
│  • Always available at a price                                                  │
│  • Marginal price setter for large orders                                       │
│  • Expected: 10-20% of volume                                                   │
│                                                                                  │
│  EXTENSION: Cross-Ecosystem [See Section 23]                                    │
│  ───────────────────────────────────────────────                                │
│  Access liquidity in other ecosystems (NEAR, etc.)                              │
│  • Requires bridge integration (Omni Bridge)                                    │
│  • Higher latency (30-120s)                                                     │
│  • Enables new trading pairs (ATOM/NEAR)                                        │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

# 2. Design Principles

## 2.1 Core Principles

| Principle | Implementation | Why It Matters |
|-----------|----------------|----------------|
| **Minimal Trust** | On-chain settlement with slashing | Users don't trust solvers |
| **Minimal Capital** | JIT execution, intent matching | Lower barriers, more competition |
| **Fast UX** | Off-chain coordination + solver relayers | Competitive with CEX |
| **Price Competition** | Open solver network, batch auctions | Best execution for users |
| **Partial Fills** | Native support throughout | Better fill rates |
| **Aligned Incentives** | Solver-relayers protect their own capital | Robust infrastructure |

## 2.2 Trust Spectrum

```
FULLY TRUSTLESS                                                    FULLY TRUSTED
      │                                                                  │
      ▼                                                                  ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                                                                  │
│  On-Chain          Settlement        Skip Select       Solver          CEX      │
│  Fallback          Contracts         Coordination      Execution       APIs     │
│                                                                                  │
│     ◄──────────────────────────────────────────────────────────────────────►    │
│                                                                                  │
│  Slowest                                                              Fastest   │
│  (15-30s)                                                             (50ms)    │
│                                                                                  │
│  Most secure                                                      Most efficient│
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘

DEFAULT PATH: Skip Select (semi-trusted) → Settlement Contract (trustless)
FALLBACK:     Direct on-chain submission (fully trustless, slower)
```

---

# Part II: Intent Layer

---

# 3. Intent Specification

## 3.1 Core Intent Structure

```rust
/// A user's expression of desired trade outcome
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Intent {
    // ═══════════════════════════════════════════════════════════════════════════
    // IDENTIFICATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Unique identifier (hash of contents + nonce)
    pub id: String,
    
    /// Protocol version for compatibility
    pub version: String,  // "1.0"
    
    /// Nonce for replay protection
    pub nonce: u64,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // USER IDENTITY
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// User's address on source chain
    pub user: String,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // TRADE SPECIFICATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// What the user is offering
    pub input: Asset,
    
    /// What the user wants
    pub output: OutputSpec,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // EXECUTION CONFIGURATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Partial fill settings
    pub fill_config: FillConfig,
    
    /// Execution constraints
    pub constraints: ExecutionConstraints,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // AUTHENTICATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Signature over canonical intent hash
    pub signature: Binary,
    
    /// Public key for verification
    pub public_key: Binary,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // METADATA
    // ═══════════════════════════════════════════════════════════════════════════
    
    pub created_at: u64,
    pub expires_at: u64,
}

/// Asset specification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    /// Source chain (e.g., "cosmoshub-4")
    pub chain_id: String,
    
    /// Token denomination
    pub denom: String,
    
    /// Amount in base units (e.g., uatom)
    pub amount: Uint128,
}

/// Output specification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputSpec {
    /// Destination chain
    pub chain_id: String,
    
    /// Desired token denomination
    pub denom: String,
    
    /// Minimum acceptable amount
    pub min_amount: Uint128,
    
    /// Limit price (output per unit input)
    pub limit_price: Decimal,
    
    /// Recipient address on destination chain
    pub recipient: String,
}
```

## 3.2 Execution Constraints

```rust
/// Constraints on how intent can be executed
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionConstraints {
    /// Absolute deadline (Unix timestamp)
    pub deadline: u64,
    
    /// Maximum IBC hops allowed
    pub max_hops: Option<u32>,
    
    /// Venues to exclude
    pub excluded_venues: Vec<String>,
    
    /// Maximum solver fee (basis points)
    pub max_solver_fee_bps: Option<u32>,
    
    /// Allow cross-ecosystem execution (NEAR, etc.)
    pub allow_cross_ecosystem: bool,
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self {
            deadline: current_timestamp() + 60,     // 60 seconds
            max_hops: Some(3),
            excluded_venues: vec![],
            max_solver_fee_bps: Some(50),           // 0.5% max
            allow_cross_ecosystem: false,           // Cosmos-only by default
        }
    }
}
```

---

# 4. Partial Fill Support

## 4.1 Fill Configuration

```rust
/// Configuration for partial fills
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillConfig {
    /// Allow partial fills?
    pub allow_partial: bool,
    
    /// Minimum fill amount (absolute)
    pub min_fill_amount: Uint128,
    
    /// Minimum fill percentage (0.0 - 1.0)
    pub min_fill_pct: Decimal,
    
    /// Time window to aggregate fills (ms)
    pub aggregation_window_ms: u64,
    
    /// Fill strategy
    pub strategy: FillStrategy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FillStrategy {
    /// Accept any fills meeting price
    Eager,
    
    /// Full fill or nothing
    AllOrNothing,
    
    /// Require minimum %, then accept any additional
    MinimumThenEager { min_pct: Decimal },
    
    /// Let solver optimize
    SolverDiscretion,
}

impl Default for FillConfig {
    fn default() -> Self {
        Self {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: Decimal::from_str("0.1").unwrap(),  // 10%
            aggregation_window_ms: 5000,                       // 5 seconds
            strategy: FillStrategy::Eager,
        }
    }
}
```

## 4.2 Intent Lifecycle

```
                                          ┌──────────────────┐
                                          │                  │
                          ┌──────────────►│    Cancelled     │
                          │               │                  │
                          │ cancel        └──────────────────┘
                          │ (no fills)
                          │
┌──────────┐  submit  ┌───┴──────────┐   partial    ┌───────────────────┐
│          │─────────►│              │─────fill────►│                   │
│  Created │          │   Pending    │              │  PartiallyFilled  │◄──┐
│          │          │              │◄─────────────│                   │───┘
└──────────┘          └──────────────┘    more      └───────────────────┘
                             │            fills             │
                             │                              │
                             │ full fill                    │ finalize
                             │                              │ (user/timeout)
                             ▼                              ▼
                      ┌──────────────┐             ┌───────────────────┐
                      │              │             │                   │
                      │    Filled    │             │     Finalized     │
                      │   (100%)     │             │ (partial accepted)│
                      │              │             │                   │
                      └──────────────┘             └───────────────────┘
                             │                              │
                             └──────────────┬───────────────┘
                                            │
                                            ▼
                                   ┌───────────────────┐
                                   │                   │
                                   │      Settled      │
                                   │                   │
                                   └───────────────────┘
```

---

# Part III: Coordination Layer

---

# 5. Skip Select Integration

## 5.1 Overview

Skip Select provides off-chain coordination with on-chain settlement guarantees:

| Feature | Benefit |
|---------|---------|
| Low latency (50ms) | Competitive with CEX |
| Intent streaming | Real-time solver feed |
| Matching engine | Zero-capital P2P crossing |
| Batch auctions | Fair price discovery |
| On-chain fallback | Trustless alternative |

## 5.2 API Overview

```yaml
# User API
POST   /v1/intents              # Submit intent
GET    /v1/intents/{id}         # Get status
POST   /v1/intents/{id}/cancel  # Cancel (if no fills)
POST   /v1/intents/{id}/finalize # Accept partial fill

# Solver API
WS     /v1/solver/stream        # Real-time intent feed
POST   /v1/solutions            # Submit solution
POST   /v1/solvers/register     # Register as solver
GET    /v1/solvers/{id}/stats   # Performance metrics

# Market Data
GET    /v1/pairs                # Available pairs
GET    /v1/pairs/{pair}/book    # Order book snapshot
GET    /v1/pairs/{pair}/price   # Current price
```

## 5.3 Solver WebSocket Protocol

```yaml
# Connect
WS wss://api.skip.money/v1/solver/stream

# Subscribe
→ { "type": "subscribe", "pairs": ["ATOM/USDC", "OSMO/USDC"] }

# New intent notification
← {
    "type": "new_intent",
    "intent": { ... },
    "book_state": { "best_bid": "10.42", "best_ask": "10.48" },
    "oracle_price": "10.45"
  }

# Submit solution
→ {
    "type": "solution",
    "intent_id": "int_abc123",
    "fill": {
      "input_amount": "10000000000",
      "output_amount": "104500000000",
      "price": "10.45"
    },
    "execution_plan": { "type": "dex_route", "steps": [...] },
    "valid_for_ms": 5000
  }

# Result
← { "type": "solution_result", "status": "accepted", "position": 1 }
```

---

# 6. Matching Engine

## 6.1 Order Book Structure

```rust
/// Central limit order book for a trading pair
pub struct OrderBook {
    pub pair: TradingPair,
    
    /// Buy orders (bids) - price descending
    pub bids: BTreeMap<PriceLevel, VecDeque<BookEntry>>,
    
    /// Sell orders (asks) - price ascending
    pub asks: BTreeMap<PriceLevel, VecDeque<BookEntry>>,
    
    /// Sequence for time priority
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct BookEntry {
    pub intent_id: String,
    pub user: String,
    pub side: Side,
    pub original_amount: Uint128,
    pub remaining_amount: Uint128,
    pub limit_price: Decimal,
    pub fill_config: FillConfig,
    pub timestamp: Timestamp,
    pub sequence: u64,
}
```

## 6.2 Matching Algorithm

```rust
impl OrderBook {
    /// Process incoming intent
    pub fn process_intent(&mut self, intent: &Intent) -> MatchResult {
        let side = self.determine_side(intent);
        let mut remaining = intent.input.amount;
        let mut fills = Vec::new();
        
        // Get opposite side of book
        let opposite = match side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };
        
        // Walk the book at each price level
        for (price_level, entries) in opposite.iter_mut() {
            // Check if prices cross
            if !self.prices_cross(intent.output.limit_price, price_level.0, side) {
                break;
            }
            
            // Match against entries at this level (FIFO)
            for entry in entries.iter_mut() {
                if remaining.is_zero() {
                    break;
                }
                
                let match_amount = std::cmp::min(remaining, entry.remaining_amount);
                
                if !match_amount.is_zero() {
                    // Execute at maker's price (price improvement for taker)
                    fills.push(Fill {
                        input_amount: match_amount,
                        output_amount: match_amount * price_level.0,
                        price: price_level.0,
                        source: FillSource::IntentMatch {
                            counterparty: entry.intent_id.clone(),
                        },
                    });
                    
                    remaining -= match_amount;
                    entry.remaining_amount -= match_amount;
                }
            }
        }
        
        // Add remainder to book if partial allowed
        if !remaining.is_zero() && intent.fill_config.allow_partial {
            self.add_to_book(intent, remaining, side);
        }
        
        MatchResult { fills, remaining }
    }
}
```

---

# 7. Auction Mechanism

## 7.1 Batch Uniform Clearing

For epochs with multiple intents:

```rust
pub struct BatchAuction {
    pub epoch_id: u64,
    pub pair: TradingPair,
    pub intents: Vec<Intent>,
    pub quotes: Vec<SolverQuote>,
    pub oracle_price: Decimal,
}

impl BatchAuction {
    pub fn clear(&self) -> AuctionResult {
        // 1. Separate by side
        let (buys, sells): (Vec<_>, Vec<_>) = self.intents.iter()
            .partition(|i| self.is_buy(i));
        
        // 2. Cross internal orders first (no solver needed)
        let (internal_fills, remaining_buy, remaining_sell) = 
            self.cross_internal(&buys, &sells);
        
        // 3. Route net flow to solvers
        let solver_fills = if remaining_buy > remaining_sell {
            self.fill_from_solver_asks(remaining_buy - remaining_sell)
        } else {
            self.fill_from_solver_bids(remaining_sell - remaining_buy)
        };
        
        // 4. Uniform clearing price
        let clearing_price = self.calculate_clearing_price(&internal_fills, &solver_fills);
        
        AuctionResult {
            epoch_id: self.epoch_id,
            clearing_price,
            internal_fills,
            solver_fills,
        }
    }
}
```

---

# Part IV: Solver Layer

---

# 8. Solver Framework

## 8.1 Solver Interface

```rust
#[async_trait]
pub trait Solver: Send + Sync {
    fn id(&self) -> &str;
    fn supported_pairs(&self) -> &[TradingPair];
    fn capabilities(&self) -> &SolverCapabilities;
    
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError>;
    async fn capacity(&self, pair: &TradingPair) -> SolverCapacity;
}

#[derive(Clone, Debug)]
pub struct SolverCapabilities {
    pub dex_routing: bool,
    pub intent_matching: bool,
    pub cex_backstop: bool,
    pub cross_ecosystem: bool,
    pub max_fill_size_usd: u64,
}

#[derive(Clone, Debug)]
pub struct Solution {
    pub solver_id: String,
    pub intent_id: String,
    pub fill: ProposedFill,
    pub execution: ExecutionPlan,
    pub valid_until: Timestamp,
    pub bond: Uint128,
}

#[derive(Clone, Debug)]
pub enum ExecutionPlan {
    DexRoute { steps: Vec<DexSwapStep> },
    InventoryFill { source_chain: String },
    CexHedge { exchange: String },
    CrossEcosystem { bridge: String, target: String },
}
```

---

# 9. DEX Routing Solver

**Capital Required: Zero**

```rust
pub struct DexRoutingSolver {
    id: String,
    osmosis: OsmosisClient,
    astroport: AstroportClient,
    skip_go: SkipGoClient,
    relayer: Arc<SolverRelayer>,  // Integrated relayer
}

#[async_trait]
impl Solver for DexRoutingSolver {
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError> {
        // Query all DEXs concurrently
        let (osmosis, astroport, aggregated) = futures::join!(
            self.query_osmosis(intent, ctx.remaining),
            self.query_astroport(intent, ctx.remaining),
            self.skip_go.get_route(intent, ctx.remaining),
        );
        
        // Find best route
        let best = [osmosis, astroport, aggregated]
            .into_iter()
            .filter_map(|q| q.ok())
            .filter(|q| q.output >= intent.output.min_amount)
            .max_by_key(|q| q.output)
            .ok_or(SolveError::NoViableRoute)?;
        
        // Calculate fee (10% of surplus over user's limit)
        let user_min = ctx.remaining * intent.output.limit_price;
        let surplus = best.output.saturating_sub(user_min);
        let solver_fee = surplus * Decimal::from_str("0.10")?;
        
        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: ctx.remaining,
                output_amount: best.output - solver_fee,
                price: Decimal::from_ratio(best.output - solver_fee, ctx.remaining),
            },
            execution: ExecutionPlan::DexRoute { steps: best.route },
            valid_until: current_timestamp() + Duration::from_secs(5),
            bond: self.calculate_bond(ctx.remaining),
        })
    }
}
```

---

# 10. CEX Backstop Solver

**Capital Required: ~$50k buffer**

```rust
pub struct CexBackstopSolver {
    id: String,
    binance: BinanceClient,
    inventory: Arc<RwLock<InventoryBuffer>>,
    hedger: HedgingService,
    relayer: Arc<SolverRelayer>,  // Integrated relayer
}

#[async_trait]
impl Solver for CexBackstopSolver {
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError> {
        // Get CEX price
        let cex_price = self.binance.get_mid_price(&intent.pair()).await?;
        
        // Apply spread
        let our_price = self.apply_spread(cex_price, intent.side());
        
        // Check inventory
        let inventory = self.inventory.read().await;
        let available = inventory.available(&intent.output.chain_id, &intent.output.denom);
        let fill_amount = std::cmp::min(ctx.remaining, available);
        
        // Queue hedge
        self.hedger.queue(HedgeOrder {
            pair: intent.pair().to_symbol(),
            side: intent.side().opposite(),
            amount: fill_amount,
        }).await?;
        
        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: fill_amount,
                output_amount: fill_amount * our_price,
                price: our_price,
            },
            execution: ExecutionPlan::CexHedge { exchange: "binance".to_string() },
            valid_until: current_timestamp() + Duration::from_secs(3),
            bond: self.calculate_bond(fill_amount),
        })
    }
}
```

---

# 11. Solution Aggregation

```rust
pub struct SolutionAggregator {
    solvers: Vec<Arc<dyn Solver>>,
}

impl SolutionAggregator {
    pub async fn aggregate(&self, intent: &Intent, matched: Uint128) -> OptimalFillPlan {
        let remaining = intent.input.amount - matched;
        if remaining.is_zero() {
            return OptimalFillPlan::fully_matched(intent, matched);
        }
        
        let ctx = SolveContext {
            matched_amount: matched,
            remaining,
            oracle_price: self.get_oracle_price(&intent.pair()).await,
        };
        
        // Collect solutions concurrently
        let solutions: Vec<Solution> = futures::future::join_all(
            self.solvers.iter().map(|s| s.solve(intent, &ctx))
        )
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
        
        // Greedy selection: best prices first
        let mut sorted = solutions;
        sorted.sort_by(|a, b| b.fill.price.cmp(&a.fill.price));
        
        let mut selected = Vec::new();
        let mut total_input = Uint128::zero();
        
        for solution in sorted {
            if total_input >= remaining {
                break;
            }
            
            let take = std::cmp::min(remaining - total_input, solution.fill.input_amount);
            selected.push((solution, take));
            total_input += take;
        }
        
        OptimalFillPlan { selected, total_input }
    }
}
```

---

# Part V: Settlement Layer

---

# 12. IBC Settlement Flows

## 12.1 Flow Categories

| Flow | IBC Hops | Latency | Use Case |
|------|----------|---------|----------|
| Same-chain | 0 | ~3s | Intent match on Hub |
| Direct IBC | 1 | ~6s | Hub → Noble |
| Multi-hop PFM | 2+ | ~15-20s | Celestia → Neutron |
| DEX + Forward | 1-2 | ~10-15s | Hub → Osmosis (swap) → Noble |

## 12.2 Flow Decision Tree

```
Intent received
    │
    ├── Same chain? ──────────────► Bank transfer (3s)
    │
    ├── Direct channel exists? ───► Direct IBC (6s)
    │
    ├── Need swap on route? ──────► IBC Hooks + Wasm (10-15s)
    │
    └── Multi-hop required? ──────► PFM forwarding (15-20s)
```

## 12.3 Direct IBC Transfer

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    DIRECT IBC: HUB → NOBLE (6 seconds)                          │
└─────────────────────────────────────────────────────────────────────────────────┘

  COSMOS HUB               SOLVER RELAYER                 NOBLE
      │                          │                          │
 T=0  │ MsgTransfer              │                          │
      │ { channel-750,           │                          │
      │   104M uusdc,            │                          │
      │   noble1user... }        │                          │
      │                          │                          │
      │  SendPacket event ──────►│                          │
      │                          │                          │
 T=1s │                     ┌────┴────┐                     │
      │                     │PRIORITY │                     │
      │                     │ OUR PKT │                     │
      │                     │ FIRST!  │                     │
      │                     └────┬────┘                     │
      │                          │                          │
 T=2s │                          │  MsgRecvPacket          │
      │                          │  ───────────────────────►│
      │                          │                          │
      │                          │                     ┌────┴────┐
      │                          │                     │ Credit  │
      │                          │                     │ to user │
      │                          │                     └────┬────┘
      │                          │                          │
 T=4s │                          │◄──────────────────────── │ Ack
      │                          │                          │
 T=5s │◄─────────────────────────│ MsgAcknowledgement       │
      │                          │                          │
 T=6s │ COMPLETE ✓               │                          │
```

---

# 13. Packet Forward Middleware

For multi-hop routes without direct channels:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    MULTI-HOP PFM: CELESTIA → OSMOSIS → HUB → NEUTRON           │
└─────────────────────────────────────────────────────────────────────────────────┘

User signs ONCE. Memo contains nested forwarding instructions:

{
  "forward": {
    "receiver": "osmo1pfm...",
    "channel": "channel-0",
    "next": {
      "forward": {
        "receiver": "cosmos1pfm...",
        "channel": "channel-569",
        "next": {
          "forward": {
            "receiver": "neutron1user...",
            "channel": "channel-1"
          }
        }
      }
    }
  }
}

CELESTIA         OSMOSIS            HUB              NEUTRON
    │               │                │                  │
T=0 │──Packet 1───►│                │                  │
    │               │                │                  │
T=5s│          Parse PFM            │                  │
    │          Auto-forward         │                  │
    │               │──Packet 2────►│                  │
    │               │                │                  │
T=10s               │           Parse PFM              │
    │               │           Auto-forward           │
    │               │                │──Packet 3──────►│
    │               │                │                  │
T=15s               │                │             Credit user
    │               │                │                  │
    │               │                │◄────────────────│ Ack 3
    │               │◄───────────────│ Ack 2           │
    │◄──────────────│ Ack 1          │                  │
    │               │                │                  │
T=20s COMPLETE ✓    │                │                  │
```

---

# 14. IBC Hooks & Wasm Execution

Execute swaps on intermediate chains atomically:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    IBC HOOKS: HUB → OSMOSIS (SWAP) → NOBLE                      │
└─────────────────────────────────────────────────────────────────────────────────┘

MsgTransfer with Wasm memo:

{
  "wasm": {
    "contract": "osmo1swaprouter...",
    "msg": {
      "swap_exact_amount_in": {
        "routes": [{ "pool_id": "1", "token_out_denom": "uusdc" }],
        "token_out_min_amount": "104000000"
      }
    }
  },
  "forward": {
    "receiver": "noble1user...",
    "channel": "channel-750"
  }
}

COSMOS HUB              OSMOSIS                    NOBLE
    │                      │                          │
T=0 │  MsgTransfer         │                          │
    │  { ATOM + wasm memo }│                          │
    │  ───────────────────►│                          │
    │                      │                          │
T=5s│                 ┌────┴────┐                     │
    │                 │ 1. Recv │                     │
    │                 │    ATOM │                     │
    │                 │ 2. Exec │                     │
    │                 │    swap │                     │
    │                 │ 3. Fwd  │                     │
    │                 │    USDC │                     │
    │                 └────┬────┘                     │
    │                      │                          │
T=8s│                      │  MsgTransfer (USDC)      │
    │                      │  ────────────────────────►│
    │                      │                          │
T=12s                      │                     Credit user
    │                      │◄─────────────────────────│ Ack
    │◄─────────────────────│ Ack                      │
    │                      │                          │
T=15s COMPLETE ✓           │                          │
```

---

# 15. Timeout & Recovery

## 15.1 Timeout Flow

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           IBC TIMEOUT RECOVERY                                   │
└─────────────────────────────────────────────────────────────────────────────────┘

  SOURCE                    RELAYER                    DESTINATION
     │                         │                            │
T=0  │ MsgTransfer             │                            │
     │ timeout: T+10min        │                            │
     │                         │                            │
     │  [Packet committed]     │                            │
     │  [Tokens escrowed]      │                            │
     │                         │                            │
     │  SendPacket ───────────►│                            │
     │                         │                            │
     │                         │     ╳ DELIVERY FAILS       │
     │                         │─────────────────────────►  │
     │                         │     (chain down,           │
     │                         │      relayer issues)       │
     │                         │                            │
     │      ... time passes ...│                            │
     │                         │                            │
T=10m│                    ┌────┴────┐                       │
     │                    │ Timeout │                       │
     │                    │ reached │                       │
     │                    │ Build   │                       │
     │                    │ proof   │                       │
     │                    └────┬────┘                       │
     │                         │                            │
     │  MsgTimeout             │                            │
     │◄────────────────────────│                            │
     │  { proof_unreceived }   │                            │
     │                         │                            │
     │  [Verify proof]         │                            │
     │  [Unescrow tokens]      │                            │
     │  [Return to sender]     │                            │
     │                         │                            │
     │  TOKENS RECOVERED ✓     │                            │
```

## 15.2 Key Safety Property

**Funds cannot be lost, only delayed.** They either:
1. Arrive at destination, OR
2. Return to source via timeout

---

# Part VI: Relayer Economics

---

# 16. The Relayer Problem

## 16.1 Traditional Model Fails

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    THE RELAYER PROBLEM                                           │
└─────────────────────────────────────────────────────────────────────────────────┘

Traditional IBC relies on altruistic relayers:

  User ──► Solver ──► IBC Transfer ──► ??? ──► Public Relayer ──► Destination
                                        │
                                        └── WHO RELAYS?
                                            • No direct profit
                                            • Best effort only
                                            • No accountability
                                            • Single point of failure
```

## 16.2 Solver Capital at Risk

The intent system changes the economics:

```
SOLVER EXPOSURE TIMELINE
════════════════════════

T+0s     Solver commits output (104,000 USDC)
         ┌────────────────────────────────────────────────────────────────┐
         │                                                                │
         │   EXPOSURE WINDOW                                              │
         │                                                                │
         │   Solver has committed output but hasn't received input yet   │
         │                                                                │
         │   RISK: If IBC times out, solver could lose output while      │
         │         user keeps their input                                │
         │                                                                │
         └────────────────────────────────────────────────────────────────┘
T+???    Input confirmed, exposure ends

THE FASTER THE RELAY, THE SHORTER THE EXPOSURE, THE LOWER THE RISK
```

---

# 17. Solver-Integrated Relayers

## 17.1 Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    SOLVER WITH INTEGRATED RELAYER                                │
└─────────────────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────────────────┐
│                              SOLVER NODE                                        │
│                                                                                 │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐          │
│  │  Quote Engine   │     │  Execution      │     │   Settlement    │          │
│  │                 │     │  Engine         │     │   Tracker       │          │
│  └────────┬────────┘     └────────┬────────┘     └────────┬────────┘          │
│           │                       │                       │                    │
│           └───────────────────────┼───────────────────────┘                    │
│                                   │                                            │
│                                   ▼                                            │
│                    ┌──────────────────────────────┐                            │
│                    │      INTEGRATED RELAYER      │                            │
│                    │                              │                            │
│                    │  ┌────────────────────────┐  │                            │
│                    │  │   Packet Prioritizer   │  │                            │
│                    │  │                        │  │                            │
│                    │  │  Priority 1: OUR fills │  │                            │
│                    │  │  Priority 2: Paid relay│  │                            │
│                    │  │  Priority 3: Altruistic│  │                            │
│                    │  └────────────────────────┘  │                            │
│                    │                              │                            │
│                    └──────────────────────────────┘                            │
│                                                                                 │
└────────────────────────────────────────────────────────────────────────────────┘
```

## 17.2 Economic Analysis

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    SHOULD SOLVERS RUN RELAYERS?                                  │
└─────────────────────────────────────────────────────────────────────────────────┘

MONTHLY COSTS:
──────────────
Infrastructure (servers, RPC)     $200 - $500
Gas (submitting proofs)           $50 - $200
Engineering (maintenance)         $100 - $200
                                  ───────────
TOTAL:                            $350 - $900

MONTHLY BENEFITS:
─────────────────
1. Win more auctions (speed)      $150+      (10% more wins)
2. Reduce risk (shorter exposure) $100-300   (lower bond needed)
3. Avoid timeouts (protect capital) $300+    (prevent losses)
4. Relay revenue (paid service)   $250       (1000 pkts @ $0.25)
                                  ───────────
TOTAL:                            $800 - $1,500+

NET BENEFIT: $200 - $900+/month

VERDICT: ✓ Relayer pays for itself for any active solver
```

## 17.3 Implementation

```rust
pub struct SolverRelayer {
    chains: HashMap<String, ChainClient>,
    our_pending: Arc<RwLock<BTreeMap<Priority, PendingPacket>>>,
    paid_queue: Arc<RwLock<PriorityQueue<PaidRelayRequest>>>,
}

impl SolverRelayer {
    pub async fn run(&self) {
        loop {
            // PRIORITY 1: Our packets (protect capital!)
            if let Some(packet) = self.get_most_urgent_own_packet().await {
                self.relay_immediately(packet).await;
                continue;
            }
            
            // PRIORITY 2: Paid requests (revenue)
            if let Some(request) = self.get_profitable_paid_request().await {
                self.relay_for_fee(request).await;
                continue;
            }
            
            // PRIORITY 3: Altruistic (good citizen)
            if let Some(packet) = self.general_queue.pop_front().await {
                self.relay_altruistic(packet).await;
                continue;
            }
            
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    
    pub async fn track_settlement(&self, settlement: &Settlement) {
        for transfer in &settlement.ibc_transfers {
            self.our_pending.write().await.insert(
                Priority::from_exposure(settlement.solver_exposure, transfer.timeout),
                PendingPacket::from(transfer),
            );
        }
    }
}
```

---

# 18. Two-Phase Commit Settlement

Prevents solver losses from asymmetric IBC failures:

```rust
pub struct TwoPhaseSettlement {
    user_escrow: EscrowContract,
    solver_vault: SolverVaultContract,
    relayer: Arc<SolverRelayer>,
    timeouts: TimeoutConfig,
}

impl TwoPhaseSettlement {
    pub async fn execute(&self, intent: &Intent, solution: &Solution) -> Result<(), Error> {
        // ═══════════════════════════════════════════════════════════════════
        // PHASE 1: COMMIT - Both parties lock funds
        // ═══════════════════════════════════════════════════════════════════
        
        // 1a. Lock user's input
        let user_lock = self.user_escrow.lock(&intent.input).await?;
        
        // 1b. Lock solver's output
        let solver_lock = self.solver_vault.lock(
            &solution.solver_id,
            &solution.fill.output_amount,
        ).await?;
        
        // Now both committed - safe to proceed
        
        // ═══════════════════════════════════════════════════════════════════
        // PHASE 2: EXECUTE - Transfer funds
        // ═══════════════════════════════════════════════════════════════════
        
        // 2a. Send output to user via IBC
        let output_tx = self.send_output(&solver_lock, &intent.output).await?;
        
        // Track with our relayer (priority handling)
        self.relayer.track_settlement(&settlement).await;
        
        // 2b. Wait for confirmation
        match self.wait_for_ibc(&output_tx).await {
            Ok(_) => {
                // Success - release user's input to solver
                self.user_escrow.release_to(user_lock, &solution.solver_id).await?;
                self.solver_vault.mark_complete(solver_lock).await?;
                Ok(())
            }
            Err(IbcTimeout) => {
                // Failed - unwind BOTH locks
                self.solver_vault.unlock(solver_lock).await?;  // Output back to solver
                self.user_escrow.refund(user_lock).await?;     // Input back to user
                Err(Error::IbcTimeout)
            }
        }
    }
}
```

## 18.1 Timeout Ordering Rule

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           CRITICAL SAFETY RULE                                   │
│                                                                                  │
│  ESCROW TIMEOUT must be LONGER than IBC TIMEOUT + BUFFER                        │
│                                                                                  │
│  ┌────────────────────────────────────────────────────────────────────────┐    │
│  │                                                                        │    │
│  │  Input Escrow                              Output IBC                  │    │
│  │  ─────────────                              ──────────                 │    │
│  │                                                                        │    │
│  │  Lock: T+0                                  Send: T+5s                 │    │
│  │                                             Timeout: T+10min           │    │
│  │                                                    │                   │    │
│  │  Release: T+15min ◄────── 5 min buffer ───────────►│                   │    │
│  │                                                                        │    │
│  │  If IBC times out at T+10min:                                         │    │
│  │  • We have 5 min to detect                                            │    │
│  │  • Cancel escrow release                                              │    │
│  │  • Refund user's input                                                │    │
│  │  • Solver gets output back (via IBC timeout)                          │    │
│  │                                                                        │    │
│  └────────────────────────────────────────────────────────────────────────┘    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

# 19. Relayer as Profit Center

Solvers can monetize their infrastructure:

```rust
pub struct PaidRelayService {
    relayer: Arc<SolverRelayer>,
    fee_schedule: FeeSchedule,
}

#[derive(Clone)]
pub struct FeeSchedule {
    pub base_fee: Coin,           // $0.10
    pub fast_premium: Coin,       // $0.50 (< 2 blocks)
    pub guaranteed_premium: Coin, // $1.00 (SLA or refund)
}

impl PaidRelayService {
    pub async fn submit(&self, request: RelayRequest) -> Result<Receipt, Error> {
        let fee = self.calculate_fee(&request);
        
        if request.payment.amount < fee {
            return Err(Error::InsufficientFee);
        }
        
        self.relayer.paid_queue.write().await.push(request);
        
        Ok(Receipt {
            request_id: generate_id(),
            estimated_delivery: self.estimate(&request.service_level),
        })
    }
}
```

## 19.1 Ecosystem Dynamics

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    EQUILIBRIUM: SOLVER-RELAYER ECOSYSTEM                         │
└─────────────────────────────────────────────────────────────────────────────────┘

COMPETITIVE DYNAMICS:
─────────────────────

Round 1: No solvers run relayers
  → All depend on public relayers
  → Slow, unreliable fills
  → Poor user experience

Round 2: One solver adds relayer
  → Faster fills, wins more auctions
  → Competitors lose market share
  → Pressure builds

Round 3: Most solvers add relayers
  → Multiple competing relayers
  → Fast, reliable IBC
  → Great user experience

EQUILIBRIUM:
────────────
Solvers WITH relayers:
  ✓ Faster fills → win auctions
  ✓ Lower risk → smaller margins
  ✓ Reliable → good reputation

Solvers WITHOUT relayers:
  ✗ Slower → lose auctions
  ✗ Higher risk → larger margins
  ✗ Unreliable → poor reputation
  ✗ Natural selection → squeezed out

EMERGENT PROPERTY:
──────────────────
Economic incentives naturally solve "who runs relayers?" problem
```

---

# Part VII: Security & Economics

---

# 20. Security Model

## 20.1 Trust Assumptions

| Component | Trust Level | Failure Mode | Mitigation |
|-----------|-------------|--------------|------------|
| Skip Select | Semi-trusted | Censorship | On-chain fallback |
| Solvers | Untrusted | Failed fills | Bond + slashing |
| Settlement Contracts | Trustless | Bugs | Audits |
| IBC | Trustless | Timeout | Auto refund |
| Solver Relayers | Self-interested | Prioritize own | Redundancy |

## 20.2 Slashing Parameters

```rust
pub struct SlashingConfig {
    pub base_slash_pct: Decimal,      // 2% of fill value
    pub repeat_multiplier: Decimal,   // 2x after 3 failures
    pub min_slash: Uint128,           // 10 ATOM
    pub max_slash: Uint128,           // 1,000 ATOM
    pub bond_lock_multiplier: Decimal, // 1.5x fill value
}
```

## 20.3 Attack Mitigations

| Attack | Mitigation |
|--------|------------|
| Solver griefing | Bond slashing + timeout refund |
| Oracle manipulation | Multi-source + bounds |
| Front-running | Batch auctions |
| Relayer withholding | Solver self-relay + redundancy |

---

# 21. Economic Analysis

## 21.1 User Economics

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    USER COST: 10,000 ATOM → USDC                                 │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│                         Traditional DEX    This System     Improvement          │
│  ────────────────────────────────────────────────────────────────────────────   │
│  AMM Slippage           0.30% ($315)      0.05% ($52)*     +$263                │
│  Solver Fee             N/A               0.05% ($52)      -$52                 │
│  Gas                    $0.10             $0.15            -$0.05               │
│  ────────────────────────────────────────────────────────────────────────────   │
│  TOTAL COST             ~$316             ~$104            +$212 (67%)          │
│  Execution Time         6-12 sec          2-5 sec          2-4x faster          │
│                                                                                  │
│  * Much matched directly via intent crossing (zero slippage)                    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## 21.2 Solver Economics

```
DEX ROUTING SOLVER (Zero Capital):
──────────────────────────────────
Revenue per $100k trade:
  Surplus capture (10% of improvement): ~$50
Costs:
  Gas: ~$0.25
  Infrastructure: ~$0.05
Net: ~$49.70/trade


CEX BACKSTOP SOLVER (~$50k Capital):
────────────────────────────────────
Revenue per $100k trade:
  Spread (5 bps): $50
Costs:
  CEX fees (1 bps): $10
  Hedging slippage: $5
  Capital cost: $7/day
Net: ~$28/trade
```

## 21.3 Liquidity Resilience

```
┌────────────────┬────────────┬────────────┬────────────┬────────────┐
│                │ DEX        │ DEX        │ DEX        │ DEX        │
│ Liquidity      │ Healthy    │ -50%       │ -90%       │ Dead       │
│ Source         │            │            │            │            │
├────────────────┼────────────┼────────────┼────────────┼────────────┤
│ Intent Match   │    25%     │    35%     │    45%     │    55%     │
│ DEX Routing    │    55%     │    35%     │    15%     │     5%     │
│ CEX Backstop   │    20%     │    30%     │    40%     │    40%     │
├────────────────┼────────────┼────────────┼────────────┼────────────┤
│ System Works?  │     ✓      │     ✓      │     ✓      │     ✓      │
└────────────────┴────────────┴────────────┴────────────┴────────────┘

KEY INSIGHT: System becomes MORE resilient as DEX liquidity decreases
because intent matching creates liquidity from order flow itself.
```

---

# 22. Failure Modes & Recovery

## 22.1 Failure Types

| Failure | Recovery |
|---------|----------|
| Solver failed fill | Slash bond, retry/refund |
| DEX swap failed | Atomic revert, refund |
| IBC timeout | Auto refund via timeout |
| Partial failure | Deliver succeeded, refund failed |
| Relayer down | Solver self-relay, redundancy |

## 22.2 Recovery Flow

```rust
pub fn handle_failure(
    failure: SettlementFailure,
) -> RecoveryAction {
    match failure {
        SettlementFailure::SolverFailed { solver_id, .. } => {
            // Slash solver, refund from slash fund
            slash_solver(&solver_id);
            RecoveryAction::RetryWithDifferentSolver
        }
        
        SettlementFailure::IbcTimeout { .. } => {
            // Funds auto-return via IBC timeout mechanism
            RecoveryAction::UserCanRetry
        }
        
        SettlementFailure::PartialFailure { succeeded, failed } => {
            // Deliver succeeded portion, refund failed portion
            deliver_partial(&succeeded);
            refund_partial(&failed);
            RecoveryAction::PartialSettlement
        }
    }
}
```

---

# Part VIII: Extensions

---

# 23. Cross-Ecosystem Module (NEAR)

> **Note**: Optional extension. Core Cosmos system operates independently.

## 23.1 When Cross-Ecosystem Helps

| Scenario | Benefit |
|----------|---------|
| NEAR has better price | Route via NEAR for improvement |
| Cosmos liquidity thin | Access deeper NEAR pools |
| New pairs | Enable ATOM/NEAR, TIA/NEAR |

## 23.2 Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         CROSS-ECOSYSTEM EXTENSION                                │
│                                                                                  │
│                     ┌─────────────────────────────────┐                         │
│                     │      CORE COSMOS SYSTEM         │                         │
│                     │      (Parts I-VII)              │                         │
│                     └───────────────┬─────────────────┘                         │
│                                     │                                            │
│                                     │ Extension Interface                        │
│                                     │                                            │
│                     ┌───────────────▼─────────────────┐                         │
│                     │    CROSS-ECOSYSTEM SOLVER       │                         │
│                     │    (competes in auctions)       │                         │
│                     └───────────────┬─────────────────┘                         │
│                                     │                                            │
│                     ┌───────────────▼─────────────────┐                         │
│                     │        OMNI BRIDGE              │                         │
│                     │    ATOM ↔ omni.ATOM (30-60s)    │                         │
│                     │    TIA  ↔ omni.TIA  (30-60s)    │                         │
│                     └───────────────┬─────────────────┘                         │
│                                     │                                            │
│                     ┌───────────────▼─────────────────┐                         │
│                     │        NEAR ECOSYSTEM           │                         │
│                     │    • NEAR Intents Network       │                         │
│                     │    • Ref Finance                │                         │
│                     └─────────────────────────────────┘                         │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## 23.3 Opt-In via Intent Constraints

```rust
pub struct ExecutionConstraints {
    // ... other fields ...
    
    /// Allow cross-ecosystem execution
    /// DEFAULT: false (Cosmos-only)
    pub allow_cross_ecosystem: bool,
    
    /// Maximum bridge latency acceptable
    pub max_bridge_time_secs: Option<u64>,
}
```

## 23.4 Cross-Ecosystem Flow

```
User intent (allow_cross_ecosystem: true)
    │
    ▼
Skip Select auction
    │
    ├── Cosmos solvers quote: 104,000 USDC
    │
    └── Cross-eco solver quotes: 104,800 USDC (via NEAR)
            │
            │ (wins auction)
            ▼
        Solver sends USDC from NEAR inventory → Omni Bridge → User
            │
            │ (~45 seconds)
            ▼
        User receives 104,800 USDC ✓
            │
            │ (async settlement)
            ▼
        User's ATOM → Omni Bridge → NEAR → Solver inventory
```



# Appendix A: API Reference

```yaml
# User API
POST   /v1/intents              # Submit
GET    /v1/intents/{id}         # Status
POST   /v1/intents/{id}/cancel  # Cancel
POST   /v1/intents/{id}/finalize # Accept partial

# Solver API
WS     /v1/solver/stream        # Intent feed
POST   /v1/solutions            # Submit solution
POST   /v1/solvers/register     # Register
GET    /v1/solvers/{id}/stats   # Stats

# Market Data
GET    /v1/pairs                # Pairs
GET    /v1/pairs/{pair}/book    # Book
GET    /v1/pairs/{pair}/price   # Price
```

---

# Appendix B: Glossary

| Term | Definition |
|------|------------|
| Intent | User's desired trade outcome |
| Solver | Entity that fills intents |
| Matching | Crossing opposing intents |
| PFM | Packet Forward Middleware |
| IBC Hooks | Wasm execution on IBC receive |
| Two-Phase Commit | Lock both sides before transfer |

---
