# Unified Cross-Ecosystem Intent System Design

## Overview

This document specifies how atom-intents integrates with IBC Eureka (Ethereum) and NEAR Intents (via Omnibridge) to create a unified liquidity aggregation system with Cosmos Hub as the routing layer.

## Goals

### Primary (This Design)
- **Liquidity aggregation**: Solvers source fills from Cosmos (IBC), Ethereum (Eureka), and NEAR (Omnibridge)
- **Hub routing**: Cosmos Hub coordinates auctions; settlements flow direct to destinations

### Future Exploration (Documented Below)
- Universal intent portability (users submit from any ecosystem)
- Federated solver network (solvers registered anywhere can fill anywhere)

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Settlement preference default | Cost-first | Let solver competition surface cheapest viable path |
| Routing model | Pass-through | Hub coordinates but doesn't bottleneck settlements |
| Solver model | Specialized cross-ecosystem solvers | Clean separation; solvers manage own bridge risk |
| NEAR bridge | Omnibridge | Available now, NEAR's canonical infrastructure |
| Asset delivery | User-configurable preference | Users control what they receive; solvers control sourcing |
| Settlement verification | Bridge-native | Trust each bridge's confirmation mechanism |
| Trust/security model | Solver's choice | No system-imposed security rankings; solvers self-select ecosystems |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           UNIFIED INTENT SYSTEM                                  │
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │                         USER LAYER                                       │   │
│  │        Cosmos Wallets → Submit intents with AssetPreference             │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                      │                                          │
│                                      ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │                    COORDINATION LAYER (Go Fast)                          │   │
│  │                                                                          │   │
│  │   Auction Engine receives quotes from ALL solver types:                  │   │
│  │   • Cosmos-native solvers (IBC Classic)                                  │   │
│  │   • Eureka solvers (Ethereum liquidity)                                  │   │
│  │   • NEAR solvers (Omnibridge liquidity)                                  │   │
│  │                                                                          │   │
│  │   Selection: Best price meeting user's constraints                       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                      │                                          │
│         ┌────────────────────────────┼────────────────────────────┐            │
│         ▼                            ▼                            ▼            │
│  ┌─────────────┐            ┌─────────────────┐           ┌─────────────┐      │
│  │   COSMOS    │            │    ETHEREUM     │           │    NEAR     │      │
│  │   (IBC)     │            │    (Eureka)     │           │  (Omnibridge)│     │
│  │             │            │                 │           │              │      │
│  │  ~3-6s      │            │  ~15-30s        │           │  ~30-60s     │      │
│  │  ~$0.01     │            │  ~$1-5          │           │  ~$0.10      │      │
│  └─────────────┘            └─────────────────┘           └──────────────┘      │
│         │                            │                            │            │
│         └────────────────────────────┼────────────────────────────┘            │
│                                      ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │                    COSMOS HUB (Settlement Layer)                         │   │
│  │                                                                          │   │
│  │   • Escrow contracts (lock user funds)                                   │   │
│  │   • Settlement contracts (verify delivery, release bonds)                │   │
│  │   • Solver registry (bonds, reputation)                                  │   │
│  │   • Pass-through routing (direct settlement where possible)              │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**Key principles:**
- Hub coordinates but doesn't bottleneck - settlements flow direct
- All solver types compete in same auction on price
- User constraints determine solver eligibility
- Bridge-native verification for each ecosystem

---

## Extended Intent Specification

```rust
pub struct Intent {
    // === Existing fields (unchanged) ===
    pub id: String,
    pub version: String,
    pub nonce: u64,
    pub user: String,
    pub input: Asset,
    pub output: OutputSpec,
    pub fill_config: FillConfig,
    pub signature: Binary,
    pub public_key: Binary,
    pub created_at: u64,
    pub expires_at: u64,

    // === Extended constraints ===
    pub constraints: ExecutionConstraints,
}

pub struct ExecutionConstraints {
    // === Existing fields ===
    pub max_slippage_bps: u32,
    pub excluded_venues: Vec<String>,
    pub max_solver_fee_bps: Option<u32>,

    // === Asset delivery (user's concern) ===

    /// What asset forms are acceptable for delivery
    /// Default: NativeOnly
    pub asset_preference: AssetPreference,

    /// Maximum acceptable settlement time
    /// Default: None (no limit)
    pub max_settlement_secs: Option<u64>,

    /// Settlement path preference (cost vs latency)
    /// Default: Cost
    pub settlement_preference: SettlementPreference,
}

pub enum AssetPreference {
    /// Only canonical native denoms (uatom, Noble USDC, etc.)
    /// Solver must deliver native - how they source is their problem
    NativeOnly,

    /// Accept bridged representations from specific ecosystems
    /// e.g., user willing to receive eureka/USDC or omni/USDC
    AcceptBridged {
        allowed_denoms: Vec<String>,  // Explicit: ["eureka/usdc", "omni/usdc"]
    },

    /// Accept any fungible equivalent
    AnyEquivalent,
}

pub enum SettlementPreference {
    Cost,      // Cheapest path (default)
    Latency,   // Fastest path
}
```

**Default behavior** (if user specifies nothing):
- `NativeOnly` assets (backwards compatible)
- `Cost` preference
- No settlement time limit

**Cross-ecosystem opt-in example:**
```rust
ExecutionConstraints {
    asset_preference: AssetPreference::AcceptBridged {
        allowed_denoms: vec![
            "uusdc".into(),           // Noble native
            "eureka/usdc".into(),     // Ethereum-origin via Eureka
        ],
    },
    settlement_preference: SettlementPreference::Cost,
    max_settlement_secs: Some(120),
    ..Default::default()
}
```

**Trust model:**
- Users specify what assets they're willing to receive
- Solvers self-select which ecosystems they operate on based on their own risk assessment
- A solver that doesn't trust Omnibridge simply doesn't run a NEAR solver
- No system-imposed security ranking

---

## Solver Types and Competition

Three solver categories compete in the same auction:

```rust
/// All solvers implement the same trait - they're peers in the auction
pub trait Solver: Send + Sync {
    fn id(&self) -> &str;
    fn ecosystem(&self) -> Ecosystem;           // Which liquidity source
    fn supported_pairs(&self) -> &[TradingPair];
    fn capabilities(&self) -> &SolverCapabilities;

    /// Quote includes settlement cost/time estimates
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution>;
}

pub struct Solution {
    pub solver_id: String,
    pub intent_id: String,

    // === Pricing ===
    pub output_amount: Uint128,           // What user receives
    pub solver_fee_bps: u32,

    // === Settlement metadata ===
    pub ecosystem: Ecosystem,             // Where liquidity sourced
    pub settlement_path: SettlementPath,  // How it gets to user
    pub estimated_time_secs: u64,
    pub estimated_cost_usd: Decimal,      // Settlement cost

    // === Asset info ===
    pub output_denom: String,             // Actual denom delivered
    pub is_native: bool,                  // Native or bridged asset
}

pub enum SettlementPath {
    /// Cosmos IBC Classic
    CosmosIbc {
        flow_type: IbcFlowType,           // Same-chain, Direct, PFM, Hooks
    },

    /// Ethereum via IBC Eureka
    Eureka {
        direction: EurekaDirection,
        eth_address: Option<String>,       // If touching Ethereum
    },

    /// NEAR via Omnibridge
    NearOmnibridge {
        direction: OmnibridgeDirection,
        near_account: Option<String>,
    },
}

pub enum Ecosystem {
    Cosmos,
    Ethereum,
    Near,
}
```

**Auction flow with cross-ecosystem solvers:**

```
User Intent: Sell 1000 ATOM for USDC (preference: Cost, asset: NativeOnly)
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           GO FAST AUCTION                                    │
│                                                                              │
│  Cosmos Solver A:     1,005 USDC  │  ~6s   │  $0.01  │  IBC to Noble       │
│  Cosmos Solver B:     1,003 USDC  │  ~8s   │  $0.02  │  PFM via Osmosis    │
│  Eureka Solver:       1,012 USDC  │  ~25s  │  $3.00  │  Eureka from ETH    │
│  NEAR Solver:         1,008 USDC  │  ~45s  │  $0.15  │  Omnibridge         │
│                                                                              │
│  Net to user (output - settlement cost):                                     │
│  • Cosmos A:  1,005 - 0.01 = 1,004.99 USDC  ← WINNER (Cost preference)      │
│  • Cosmos B:  1,003 - 0.02 = 1,002.98 USDC                                  │
│  • Eureka:    1,012 - 3.00 = 1,009.00 USDC  ← Would win on large orders    │
│  • NEAR:      1,008 - 0.15 = 1,007.85 USDC                                  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key insight:** Eureka/NEAR solvers win when their liquidity advantage exceeds their higher settlement costs. For large orders where Cosmos liquidity is thin, cross-ecosystem solvers become competitive.

---

## Settlement Flows by Ecosystem

### Cosmos IBC (Existing)

```
User (Hub) ──IBC Transfer──► Destination Chain
                │
                ▼
         IBC Acknowledgement
                │
                ▼
         Settlement Contract releases solver bond
```

- **Verification:** IBC ACK (light client proven)
- **Latency:** 3-20s depending on flow type
- **Cost:** ~$0.01-0.05

### Ethereum via IBC Eureka

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  COSMOS → ETHEREUM                                                           │
│                                                                              │
│  User (Hub) ──IBC v2 Transfer──► Cosmos Hub Eureka Module                   │
│                                          │                                   │
│                                          ▼                                   │
│                                    SP1 ZK Proof generated                    │
│                                    (Succinct Prover Network)                 │
│                                          │                                   │
│                                          ▼                                   │
│                                    Ethereum ICS26Router                      │
│                                    verifies proof                            │
│                                          │                                   │
│                                          ▼                                   │
│                                    User receives on Ethereum                 │
│                                                                              │
│  Verification: IBC ACK relayed back (ZK-verified state)                     │
│  Latency: ~15-30s                                                           │
│  Cost: ~$1-5 (Ethereum gas)                                                 │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│  ETHEREUM → COSMOS (Solver sourcing from ETH liquidity)                      │
│                                                                              │
│  Solver (ETH) ──ICS20Transfer.sol──► Lock tokens on Ethereum                │
│                                          │                                   │
│                                          ▼                                   │
│                                    Ethereum Light Client                     │
│                                    (08-wasm on Hub) verifies                 │
│                                          │                                   │
│                                          ▼                                   │
│                                    User receives tokens                      │
│                                                                              │
│  Verification: Ethereum state proof via 08-wasm light client                │
│  Latency: ~15-30s                                                           │
│  Cost: ~$1-5 (Ethereum gas)                                                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

### NEAR via Omnibridge

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  NEAR → COSMOS (Solver sourcing from NEAR liquidity)                         │
│                                                                              │
│  Solver (NEAR) ──Omnibridge──► Lock tokens on NEAR                          │
│                                      │                                       │
│                                      ▼                                       │
│                                Bridge attestation                            │
│                                      │                                       │
│                                      ▼                                       │
│                                Cosmos Hub receives                           │
│                                      │                                       │
│                                      ▼                                       │
│                                User receives tokens                          │
│                                (omni.USDC or swapped to native)              │
│                                                                              │
│  Verification: Omnibridge attestation/proof                                 │
│  Latency: ~30-60s                                                           │
│  Cost: ~$0.10                                                               │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Settlement Verification Registry

```rust
pub trait SettlementVerifier: Send + Sync {
    async fn verify_delivery(
        &self,
        settlement_id: &str,
        expected_recipient: &str,
        expected_amount: Uint128,
        expected_denom: &str,
    ) -> Result<VerificationResult>;
}

pub enum VerificationResult {
    Confirmed { proof: VerificationProof },
    Pending { estimated_completion_secs: u64 },
    Failed { reason: String },
    Timeout,
}

pub enum VerificationProof {
    IbcAck { sequence: u64, ack_data: Binary },
    EurekaZkProof { proof_hash: String, block_height: u64 },
    OmnibridgeAttestation { attestation_id: String, signatures: Vec<String> },
}
```

---

## Unified Route Registry

```rust
pub struct UnifiedRouteRegistry {
    /// Cosmos IBC routes (existing)
    cosmos_routes: HashMap<(ChainId, ChainId), Vec<CosmosRoute>>,

    /// Eureka routes (Cosmos ↔ Ethereum)
    eureka_routes: HashMap<(ChainId, ChainId), EurekaRoute>,

    /// Omnibridge routes (Cosmos ↔ NEAR)
    omnibridge_routes: HashMap<(ChainId, ChainId), OmnibridgeRoute>,
}

impl UnifiedRouteRegistry {
    pub fn find_routes(
        &self,
        source: &ChainId,
        dest: &ChainId,
        constraints: &ExecutionConstraints,
    ) -> Vec<UnifiedRoute> {
        let mut routes = vec![];

        routes.extend(self.find_cosmos_routes(source, dest));
        routes.extend(self.find_eureka_routes(source, dest));
        routes.extend(self.find_omnibridge_routes(source, dest));

        // Filter by max_settlement_secs if specified
        if let Some(max_secs) = constraints.max_settlement_secs {
            routes.retain(|r| r.estimated_time_secs <= max_secs);
        }

        // Sort by user's preference
        routes.sort_by(|a, b| match constraints.settlement_preference {
            SettlementPreference::Cost => a.estimated_cost_usd.cmp(&b.estimated_cost_usd),
            SettlementPreference::Latency => a.estimated_time_secs.cmp(&b.estimated_time_secs),
        });

        routes
    }
}

pub struct UnifiedRoute {
    pub id: String,
    pub ecosystem: Ecosystem,
    pub path: RoutePath,
    pub estimated_time_secs: u64,
    pub estimated_cost_usd: Decimal,
    pub output_denom: String,
    pub is_native_output: bool,
}

pub enum RoutePath {
    Cosmos(CosmosRoutePath),
    Eureka(EurekaRoutePath),
    Omnibridge(OmnibridgeRoutePath),
}

pub struct CosmosRoutePath {
    pub flow_type: IbcFlowType,
    pub hops: Vec<IbcHop>,
}

pub struct EurekaRoutePath {
    pub hub_to_eth_channel: String,
    pub eth_contract: String,
    pub direction: EurekaDirection,
}

pub struct OmnibridgeRoutePath {
    pub cosmos_endpoint: String,
    pub near_endpoint: String,
    pub direction: OmnibridgeDirection,
}
```

---

## Cross-Ecosystem Solver Implementation

```rust
/// Example: Solver that sources from NEAR liquidity
pub struct NearCrossEcoSolver {
    id: String,

    // === NEAR-side infrastructure ===
    near_rpc: NearRpcClient,
    near_account: AccountId,
    omnibridge_client: OmnibridgeClient,

    // === Cosmos-side infrastructure ===
    cosmos_client: CosmosClient,
    hub_address: String,

    // === Inventory tracking ===
    near_inventory: HashMap<String, Uint128>,
    cosmos_inventory: HashMap<String, Uint128>,

    // === Risk parameters (solver's choice) ===
    max_bridge_exposure: Uint128,
    min_profit_margin_bps: u32,
}

impl Solver for NearCrossEcoSolver {
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution> {
        // 1. Check if user's asset_preference allows our output
        let can_deliver_bridged = self.can_deliver_bridged(&intent.constraints);
        let must_convert_to_native = !can_deliver_bridged;

        // 2. Get NEAR-side price (e.g., from Ref Finance)
        let near_quote = self.get_near_price(&intent.input, &intent.output).await?;

        // 3. Calculate bridge costs
        let bridge_cost = self.omnibridge_client.estimate_cost().await?;

        // 4. If must deliver native, add conversion cost
        let conversion_cost = if must_convert_to_native {
            self.estimate_conversion_cost(&near_quote.output_denom).await?
        } else {
            Decimal::zero()
        };

        // 5. Calculate net output to user
        let total_cost = bridge_cost + conversion_cost;
        let net_output = near_quote.output_amount - total_cost.to_amount();

        // 6. Check profitability
        if !self.is_profitable(net_output, &intent.input) {
            return Err(SolveError::BelowMinProfit);
        }

        // 7. Build solution
        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            output_amount: net_output,
            ecosystem: Ecosystem::Near,
            settlement_path: SettlementPath::NearOmnibridge {
                direction: OmnibridgeDirection::NearToCosmos,
                near_account: Some(self.near_account.to_string()),
            },
            estimated_time_secs: 45,
            estimated_cost_usd: total_cost,
            output_denom: if must_convert_to_native {
                intent.output.denom.clone()
            } else {
                format!("omni/{}", near_quote.output_denom)
            },
            is_native: must_convert_to_native,
        })
    }
}
```

### Solver Execution Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  NEAR SOLVER WINS AUCTION - EXECUTION FLOW                                   │
│                                                                              │
│  1. User's ATOM locked in Hub escrow                                        │
│         │                                                                    │
│         ▼                                                                    │
│  2. Solver initiates on NEAR side:                                          │
│     - Swaps inventory on Ref Finance (or uses existing USDC)                │
│     - Calls Omnibridge to send USDC → Cosmos                                │
│         │                                                                    │
│         ▼                                                                    │
│  3. Omnibridge transfer (~30-60s):                                          │
│     - NEAR side: tokens locked/burned                                       │
│     - Bridge attestation                                                    │
│     - Cosmos side: omni.USDC minted                                         │
│         │                                                                    │
│         ▼                                                                    │
│  4. If user wants native (AssetPreference::NativeOnly):                     │
│     - Solver swaps omni.USDC → native USDC on Cosmos DEX                    │
│         │                                                                    │
│         ▼                                                                    │
│  5. Solver delivers to user's destination address                           │
│         │                                                                    │
│         ▼                                                                    │
│  6. Settlement verification (Omnibridge attestation)                        │
│         │                                                                    │
│         ▼                                                                    │
│  7. Escrow releases user's ATOM to solver                                   │
│     - Solver may bridge ATOM → NEAR to replenish inventory                  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Future Exploration

### Universal Intent Portability

**Concept:** Users submit intents from ANY ecosystem, not just Cosmos.

```
CURRENT: Hub-centric
  Cosmos User ──intent──► Hub ──► Solvers compete ──► Settlement

FUTURE: Universal portability
  Cosmos User ──intent──►┐
                         │
  Ethereum User ─intent─►├──► Unified Auction ──► Best Execution
                         │
  NEAR User ────intent──►┘
```

**Key questions to explore:**

| Question | Considerations |
|----------|----------------|
| Where does the auction run? | Hub as coordinator? Off-chain with multi-ecosystem submission? |
| Intent format compatibility | Can Cosmos intents, NEAR intents, and CoW Protocol orders share a format? |
| Escrow across ecosystems | User locks funds on origin chain - how does cross-chain escrow work? |
| Settlement atomicity | How to guarantee atomic swap when user is on Ethereum, solver on NEAR? |

**Potential approaches:**

1. **Hub as universal coordinator** - All intents bridge to Hub for auction
2. **Federated auction with local escrow** - Auction runs off-chain, escrow on each ecosystem
3. **Intent relaying** - Relay intent (not funds) to Hub; settlement happens cross-chain

### Federated Solver Network

**Concept:** Solvers registered on any system can fulfill intents from any other system.

```
CURRENT: Ecosystem-specific solvers
  atom-intents auction ◄── Cosmos solvers, Cross-eco solvers
  NEAR intents auction  ◄── NEAR solvers
  (Separate pools)

FUTURE: Federated network
  Unified Solver Registry
    Solver A (bonded on Hub)  ──► can fill: Cosmos, NEAR, ETH
    Solver B (bonded on NEAR) ──► can fill: NEAR, Cosmos

  Bond recognized across ecosystems via:
    • IBC Eureka state proofs
    • Omnibridge attestations
    • ZK proofs of stake
```

**Key questions to explore:**

| Question | Considerations |
|----------|----------------|
| Bond portability | How to prove solver is bonded on Chain A when filling on Chain B? |
| Slashing across ecosystems | If solver misbehaves on NEAR, how to slash bond on Cosmos? |
| Reputation aggregation | Unified reputation score across all ecosystems? |

**Potential approaches:**

1. **Cross-chain bond proofs** - ZK/light client proof of bond submitted to other chains
2. **Replicated bonds** - Solver bonds on each ecosystem (capital inefficient)
3. **Shared security layer** - External provider (EigenLayer, Babylon)

### NEAR Intents Protocol Alignment

**Concept:** Deeper integration beyond Omnibridge liquidity access.

**Questions to explore:**
- Can a single solver implementation satisfy both atom-intents `Solver` trait and NEAR `Market Maker` interface?
- Could intents be format-compatible, enabling cross-submission?
- Could auctions share order flow for better price discovery?

### IBC Eureka Expansion

**Concept:** As Eureka adds Solana, Base, Arbitrum - extend Hub routing.

```
Future Eureka ecosystem (per roadmap):

                         ┌──────────┐
                         │  Solana  │
                         └────┬─────┘
                              │
┌──────────┐    ┌─────────────┼─────────────┐    ┌──────────┐
│ Arbitrum ├────┤         COSMOS HUB        ├────┤   Base   │
└──────────┘    │      (Eureka Router)      │    └──────────┘
                └─────────────┬─────────────┘
                              │
                    ┌─────────┴─────────┐
                    │                   │
               ┌────┴────┐         ┌────┴────┐
               │Ethereum │         │  NEAR   │
               │(Eureka) │         │(Omni)   │
               └─────────┘         └─────────┘
```

**When Eureka adds NEAR:** Evaluate Eureka-NEAR vs Omnibridge; possibly support both.

### Exploration Prioritization

| Concept | Value | Complexity | Priority |
|---------|-------|------------|----------|
| Universal intent portability | High | High | Medium-term |
| Federated solver network | High | Very High | Long-term |
| NEAR Intents alignment | Medium | Medium | Opportunistic |
| Eureka expansion (Solana, etc.) | High | Low | Follow Eureka roadmap |

---

## References

- [IBC Eureka Technical Overview](https://docs.skip.build/go/eureka/eureka-tech-overview)
- [Succinct SP1 + IBC Integration](https://blog.succinct.xyz/ibc/)
- [cosmos/solidity-ibc-eureka](https://github.com/cosmos/solidity-ibc-eureka)
- [NEAR Intents Documentation](https://docs.near-intents.org/near-intents)
- [IBC v2 Specification](https://github.com/cosmos/ibc-go/issues/6985)
