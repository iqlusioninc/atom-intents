# Phase 1: IBC Eureka Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable atom-intents solvers to source liquidity from Ethereum via IBC Eureka and deliver to Cosmos users.

**Architecture:** Extend the existing settlement and solver layers to support Eureka as an additional settlement path. Solvers compete in the same auction regardless of liquidity source. Users control asset delivery preferences via intent constraints.

**Tech Stack:** Rust, async-trait, cosmwasm-std, serde, reqwest (for Skip Go Eureka API)

---

## Task 1: Extend ExecutionConstraints with AssetPreference

**Files:**
- Modify: `crates/types/src/execution.rs`
- Test: `crates/types/src/execution.rs` (inline tests)

**Step 1: Write the failing test**

Add to `crates/types/src/execution.rs` at the end of the file, inside the existing structure:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_preference_native_only_default() {
        let constraints = ExecutionConstraints::default();
        assert!(matches!(constraints.asset_preference, AssetPreference::NativeOnly));
    }

    #[test]
    fn test_asset_preference_accept_bridged() {
        let constraints = ExecutionConstraints::new(1000)
            .with_asset_preference(AssetPreference::AcceptBridged {
                allowed_denoms: vec!["eureka/usdc".to_string()],
            });

        match &constraints.asset_preference {
            AssetPreference::AcceptBridged { allowed_denoms } => {
                assert!(allowed_denoms.contains(&"eureka/usdc".to_string()));
            }
            _ => panic!("Expected AcceptBridged"),
        }
    }

    #[test]
    fn test_settlement_preference_default_cost() {
        let constraints = ExecutionConstraints::default();
        assert!(matches!(constraints.settlement_preference, SettlementPreference::Cost));
    }

    #[test]
    fn test_settlement_preference_latency() {
        let constraints = ExecutionConstraints::new(1000)
            .with_settlement_preference(SettlementPreference::Latency);
        assert!(matches!(constraints.settlement_preference, SettlementPreference::Latency));
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-types test_asset_preference
```

Expected: FAIL with "cannot find type `AssetPreference`"

**Step 3: Write minimal implementation**

Add these types and update `ExecutionConstraints` in `crates/types/src/execution.rs`:

```rust
use cosmwasm_schema::cw_serde;

/// User preference for asset delivery
#[cw_serde]
pub enum AssetPreference {
    /// Only accept canonical native denoms (uatom, Noble USDC, etc.)
    NativeOnly,

    /// Accept bridged representations from specific sources
    AcceptBridged {
        allowed_denoms: Vec<String>,
    },

    /// Accept any fungible equivalent
    AnyEquivalent,
}

impl Default for AssetPreference {
    fn default() -> Self {
        Self::NativeOnly
    }
}

/// User preference for settlement path selection
#[cw_serde]
pub enum SettlementPreference {
    /// Prefer cheapest settlement path (default)
    Cost,
    /// Prefer fastest settlement path
    Latency,
}

impl Default for SettlementPreference {
    fn default() -> Self {
        Self::Cost
    }
}

/// Constraints on how intent can be executed
#[cw_serde]
pub struct ExecutionConstraints {
    /// Absolute deadline (Unix timestamp in seconds)
    pub deadline: u64,

    /// Maximum IBC hops allowed
    pub max_hops: Option<u32>,

    /// Venues to exclude
    pub excluded_venues: Vec<String>,

    /// Maximum solver fee (basis points)
    pub max_solver_fee_bps: Option<u32>,

    /// Allow cross-ecosystem execution (NEAR, etc.) - DEPRECATED, use asset_preference
    pub allow_cross_ecosystem: bool,

    /// Maximum bridge latency acceptable (seconds)
    pub max_bridge_time_secs: Option<u64>,

    /// Asset delivery preference
    pub asset_preference: AssetPreference,

    /// Settlement path preference (cost vs latency)
    pub settlement_preference: SettlementPreference,

    /// Maximum acceptable settlement time in seconds
    pub max_settlement_secs: Option<u64>,
}

impl ExecutionConstraints {
    pub fn new(deadline: u64) -> Self {
        Self {
            deadline,
            max_hops: Some(3),
            excluded_venues: vec![],
            max_solver_fee_bps: Some(50),
            allow_cross_ecosystem: false,
            max_bridge_time_secs: None,
            asset_preference: AssetPreference::default(),
            settlement_preference: SettlementPreference::default(),
            max_settlement_secs: None,
        }
    }

    pub fn with_asset_preference(mut self, pref: AssetPreference) -> Self {
        self.asset_preference = pref;
        self
    }

    pub fn with_settlement_preference(mut self, pref: SettlementPreference) -> Self {
        self.settlement_preference = pref;
        self
    }

    pub fn with_max_settlement_secs(mut self, secs: u64) -> Self {
        self.max_settlement_secs = Some(secs);
        self
    }

    // ... keep existing builder methods ...
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self::new(0)
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-types test_asset_preference
cargo test -p atom-intents-types test_settlement_preference
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/types/src/execution.rs
git commit -m "feat(types): add AssetPreference and SettlementPreference to ExecutionConstraints

Extend intent constraints to support cross-ecosystem asset delivery:
- AssetPreference: NativeOnly (default), AcceptBridged, AnyEquivalent
- SettlementPreference: Cost (default), Latency
- max_settlement_secs for time-bounded settlements

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 2: Add Eureka Flow Type to Settlement

**Files:**
- Modify: `crates/settlement/src/ibc.rs`
- Test: `crates/settlement/src/ibc.rs` (inline tests)

**Step 1: Write the failing test**

Add to tests in `crates/settlement/src/ibc.rs`:

```rust
#[test]
fn test_eureka_flow_type() {
    let flow = IbcFlowType::Eureka {
        direction: EurekaDirection::EthereumToCosmos,
        eth_address: Some("0x1234567890abcdef".to_string()),
    };

    match flow {
        IbcFlowType::Eureka { direction, eth_address } => {
            assert!(matches!(direction, EurekaDirection::EthereumToCosmos));
            assert_eq!(eth_address, Some("0x1234567890abcdef".to_string()));
        }
        _ => panic!("Expected Eureka flow type"),
    }
}

#[test]
fn test_eureka_timeout_calculation() {
    let flow = IbcFlowType::Eureka {
        direction: EurekaDirection::CosmosToEthereum,
        eth_address: None,
    };

    let timeout = calculate_timeout(&flow, 60);
    assert_eq!(timeout, 300); // 5x multiplier for Eureka
}

#[test]
fn test_build_eureka_memo() {
    let memo = build_eureka_memo("0xRecipient", Some("swap_callback"));
    let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

    assert_eq!(parsed["eureka"]["recipient"], "0xRecipient");
    assert_eq!(parsed["eureka"]["callback"], "swap_callback");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-settlement test_eureka
```

Expected: FAIL with "cannot find variant `Eureka`"

**Step 3: Write minimal implementation**

Add to `crates/settlement/src/ibc.rs`:

```rust
/// Direction of Eureka transfer
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EurekaDirection {
    /// Cosmos Hub → Ethereum
    CosmosToEthereum,
    /// Ethereum → Cosmos Hub
    EthereumToCosmos,
}

/// IBC flow type for settlement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IbcFlowType {
    /// Same chain transfer (~3s)
    SameChain,

    /// Direct IBC transfer (~6s)
    DirectIbc { channel: String },

    /// Multi-hop via Packet Forward Middleware (~15-20s)
    MultiHopPfm { hops: Vec<PfmHop> },

    /// IBC Hooks with Wasm execution (~10-15s)
    IbcHooksWasm { contract: String, msg: String },

    /// IBC Eureka to/from Ethereum (~15-30s)
    Eureka {
        direction: EurekaDirection,
        eth_address: Option<String>,
    },
}

/// Calculate appropriate IBC timeout based on flow type
pub fn calculate_timeout(flow_type: &IbcFlowType, base_timeout_secs: u64) -> u64 {
    let multiplier = match flow_type {
        IbcFlowType::SameChain => 1,
        IbcFlowType::DirectIbc { .. } => 2,
        IbcFlowType::MultiHopPfm { hops } => 2 + hops.len() as u64,
        IbcFlowType::IbcHooksWasm { .. } => 3,
        IbcFlowType::Eureka { .. } => 5, // Eureka needs more time for ZK proof generation
    };

    base_timeout_secs * multiplier
}

/// Build Eureka-specific memo for IBC transfers
pub fn build_eureka_memo(eth_recipient: &str, callback: Option<&str>) -> String {
    let mut memo = serde_json::json!({
        "eureka": {
            "recipient": eth_recipient,
        }
    });

    if let Some(cb) = callback {
        memo["eureka"]["callback"] = serde_json::Value::String(cb.to_string());
    }

    memo.to_string()
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-settlement test_eureka
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/settlement/src/ibc.rs
git commit -m "feat(settlement): add Eureka flow type for Ethereum connectivity

Add IBC Eureka support to settlement layer:
- EurekaDirection enum (CosmosToEthereum, EthereumToCosmos)
- Eureka variant in IbcFlowType
- 5x timeout multiplier for ZK proof generation
- build_eureka_memo helper for Eureka-specific transfers

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 3: Add Ecosystem and SettlementPath Types

**Files:**
- Create: `crates/types/src/ecosystem.rs`
- Modify: `crates/types/src/lib.rs`
- Test: `crates/types/src/ecosystem.rs` (inline tests)

**Step 1: Write the failing test**

Create `crates/types/src/ecosystem.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecosystem_display() {
        assert_eq!(Ecosystem::Cosmos.as_str(), "cosmos");
        assert_eq!(Ecosystem::Ethereum.as_str(), "ethereum");
        assert_eq!(Ecosystem::Near.as_str(), "near");
    }

    #[test]
    fn test_settlement_path_cosmos_ibc() {
        let path = SettlementPath::CosmosIbc {
            channel: "channel-141".to_string(),
            is_multi_hop: false,
        };

        assert_eq!(path.ecosystem(), Ecosystem::Cosmos);
        assert_eq!(path.estimated_time_secs(), 6);
    }

    #[test]
    fn test_settlement_path_eureka() {
        let path = SettlementPath::Eureka {
            direction: crate::EurekaDirection::EthereumToCosmos,
            eth_address: Some("0x123".to_string()),
        };

        assert_eq!(path.ecosystem(), Ecosystem::Ethereum);
        assert!(path.estimated_time_secs() >= 15);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-types test_ecosystem
```

Expected: FAIL with "cannot find module `ecosystem`"

**Step 3: Write minimal implementation**

Create `crates/types/src/ecosystem.rs`:

```rust
use cosmwasm_schema::cw_serde;

/// Supported ecosystems for liquidity sourcing
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum Ecosystem {
    Cosmos,
    Ethereum,
    Near,
}

impl Ecosystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cosmos => "cosmos",
            Self::Ethereum => "ethereum",
            Self::Near => "near",
        }
    }
}

/// Direction of Eureka transfer
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum EurekaDirection {
    CosmosToEthereum,
    EthereumToCosmos,
}

/// Direction of Omnibridge transfer (for future NEAR integration)
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum OmnibridgeDirection {
    CosmosToNear,
    NearToCosmos,
}

/// Settlement path describing how assets reach the user
#[cw_serde]
pub enum SettlementPath {
    /// Cosmos IBC Classic
    CosmosIbc {
        channel: String,
        is_multi_hop: bool,
    },

    /// Ethereum via IBC Eureka
    Eureka {
        direction: EurekaDirection,
        eth_address: Option<String>,
    },

    /// NEAR via Omnibridge (future)
    NearOmnibridge {
        direction: OmnibridgeDirection,
        near_account: Option<String>,
    },
}

impl SettlementPath {
    /// Get the ecosystem this settlement path uses
    pub fn ecosystem(&self) -> Ecosystem {
        match self {
            Self::CosmosIbc { .. } => Ecosystem::Cosmos,
            Self::Eureka { .. } => Ecosystem::Ethereum,
            Self::NearOmnibridge { .. } => Ecosystem::Near,
        }
    }

    /// Estimated time for this settlement path
    pub fn estimated_time_secs(&self) -> u64 {
        match self {
            Self::CosmosIbc { is_multi_hop, .. } => {
                if *is_multi_hop { 20 } else { 6 }
            }
            Self::Eureka { .. } => 25,
            Self::NearOmnibridge { .. } => 45,
        }
    }

    /// Estimated cost in USD for this settlement path
    pub fn estimated_cost_usd(&self) -> f64 {
        match self {
            Self::CosmosIbc { is_multi_hop, .. } => {
                if *is_multi_hop { 0.02 } else { 0.01 }
            }
            Self::Eureka { .. } => 3.0,
            Self::NearOmnibridge { .. } => 0.10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecosystem_display() {
        assert_eq!(Ecosystem::Cosmos.as_str(), "cosmos");
        assert_eq!(Ecosystem::Ethereum.as_str(), "ethereum");
        assert_eq!(Ecosystem::Near.as_str(), "near");
    }

    #[test]
    fn test_settlement_path_cosmos_ibc() {
        let path = SettlementPath::CosmosIbc {
            channel: "channel-141".to_string(),
            is_multi_hop: false,
        };

        assert_eq!(path.ecosystem(), Ecosystem::Cosmos);
        assert_eq!(path.estimated_time_secs(), 6);
    }

    #[test]
    fn test_settlement_path_eureka() {
        let path = SettlementPath::Eureka {
            direction: EurekaDirection::EthereumToCosmos,
            eth_address: Some("0x123".to_string()),
        };

        assert_eq!(path.ecosystem(), Ecosystem::Ethereum);
        assert!(path.estimated_time_secs() >= 15);
    }

    #[test]
    fn test_settlement_path_costs() {
        let cosmos = SettlementPath::CosmosIbc {
            channel: "channel-0".to_string(),
            is_multi_hop: false,
        };
        let eureka = SettlementPath::Eureka {
            direction: EurekaDirection::EthereumToCosmos,
            eth_address: None,
        };

        assert!(cosmos.estimated_cost_usd() < eureka.estimated_cost_usd());
    }
}
```

Update `crates/types/src/lib.rs` to export the new module:

```rust
mod ecosystem;
pub use ecosystem::*;
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-types test_ecosystem
cargo test -p atom-intents-types test_settlement_path
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/types/src/ecosystem.rs crates/types/src/lib.rs
git commit -m "feat(types): add Ecosystem and SettlementPath types

New types for cross-ecosystem support:
- Ecosystem enum (Cosmos, Ethereum, Near)
- EurekaDirection, OmnibridgeDirection enums
- SettlementPath enum with cost/time estimates

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 4: Extend Solution Type with Ecosystem Metadata

**Files:**
- Modify: `crates/types/src/solution.rs`
- Test: `crates/types/src/solution.rs` (inline tests)

**Step 1: Write the failing test**

Add to `crates/types/src/solution.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ecosystem, SettlementPath, EurekaDirection};

    #[test]
    fn test_solution_metadata_cosmos() {
        let metadata = SolutionMetadata {
            ecosystem: Ecosystem::Cosmos,
            settlement_path: SettlementPath::CosmosIbc {
                channel: "channel-141".to_string(),
                is_multi_hop: false,
            },
            estimated_time_secs: 6,
            estimated_cost_usd: "0.01".to_string(),
            output_denom: "uusdc".to_string(),
            is_native_output: true,
        };

        assert_eq!(metadata.ecosystem, Ecosystem::Cosmos);
        assert!(metadata.is_native_output);
    }

    #[test]
    fn test_solution_metadata_eureka() {
        let metadata = SolutionMetadata {
            ecosystem: Ecosystem::Ethereum,
            settlement_path: SettlementPath::Eureka {
                direction: EurekaDirection::EthereumToCosmos,
                eth_address: Some("0x123".to_string()),
            },
            estimated_time_secs: 25,
            estimated_cost_usd: "3.00".to_string(),
            output_denom: "eureka/usdc".to_string(),
            is_native_output: false,
        };

        assert_eq!(metadata.ecosystem, Ecosystem::Ethereum);
        assert!(!metadata.is_native_output);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-types test_solution_metadata
```

Expected: FAIL with "cannot find struct `SolutionMetadata`"

**Step 3: Write minimal implementation**

Add to `crates/types/src/solution.rs`:

```rust
use crate::{Ecosystem, SettlementPath};

/// Metadata about a solution's settlement characteristics
#[cw_serde]
pub struct SolutionMetadata {
    /// Which ecosystem the liquidity is sourced from
    pub ecosystem: Ecosystem,

    /// How the settlement will occur
    pub settlement_path: SettlementPath,

    /// Estimated settlement time in seconds
    pub estimated_time_secs: u64,

    /// Estimated settlement cost in USD
    pub estimated_cost_usd: String,

    /// Actual denomination that will be delivered
    pub output_denom: String,

    /// Whether output is native (true) or bridged (false)
    pub is_native_output: bool,
}

impl SolutionMetadata {
    /// Create metadata for a Cosmos IBC settlement
    pub fn cosmos_ibc(channel: &str, output_denom: &str, is_multi_hop: bool) -> Self {
        Self {
            ecosystem: Ecosystem::Cosmos,
            settlement_path: SettlementPath::CosmosIbc {
                channel: channel.to_string(),
                is_multi_hop,
            },
            estimated_time_secs: if is_multi_hop { 20 } else { 6 },
            estimated_cost_usd: if is_multi_hop { "0.02".to_string() } else { "0.01".to_string() },
            output_denom: output_denom.to_string(),
            is_native_output: true,
        }
    }

    /// Create metadata for an Eureka settlement
    pub fn eureka(
        direction: crate::EurekaDirection,
        eth_address: Option<String>,
        output_denom: &str,
        is_native: bool,
    ) -> Self {
        Self {
            ecosystem: Ecosystem::Ethereum,
            settlement_path: SettlementPath::Eureka {
                direction,
                eth_address,
            },
            estimated_time_secs: 25,
            estimated_cost_usd: "3.00".to_string(),
            output_denom: output_denom.to_string(),
            is_native_output: is_native,
        }
    }
}
```

Also update the `Solution` struct to include optional metadata:

```rust
/// A solver's proposed solution for an intent
#[cw_serde]
pub struct Solution {
    /// Solver identifier
    pub solver_id: String,

    /// Intent being solved
    pub intent_id: String,

    /// Proposed fill details
    pub fill: ProposedFill,

    /// How the fill will be executed
    pub execution: ExecutionPlan,

    /// Solution validity deadline (Unix timestamp)
    pub valid_until: u64,

    /// Bond amount committed by solver
    pub bond: Uint128,

    /// Cross-ecosystem metadata (optional for backwards compatibility)
    pub metadata: Option<SolutionMetadata>,
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-types test_solution_metadata
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/types/src/solution.rs
git commit -m "feat(types): add SolutionMetadata for cross-ecosystem solutions

Add metadata to Solution type:
- SolutionMetadata struct with ecosystem, path, cost, time
- Helper constructors for Cosmos IBC and Eureka settlements
- Optional metadata field on Solution for backwards compatibility

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 5: Extend Skip Go Client for Eureka API

**Files:**
- Modify: `crates/solver/src/skipgo.rs`
- Test: `crates/solver/src/skipgo.rs` (inline tests)

**Step 1: Write the failing test**

Add to tests in `crates/solver/src/skipgo.rs`:

```rust
#[test]
fn test_eureka_route_request() {
    let request = EurekaRouteRequest {
        amount_in: "1000000".to_string(),
        source_asset_denom: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC on ETH
        source_asset_chain_id: "1".to_string(), // Ethereum mainnet
        dest_asset_denom: "uusdc".to_string(),
        dest_asset_chain_id: "noble-1".to_string(),
    };

    assert_eq!(request.source_asset_chain_id, "1");
    assert_eq!(request.dest_asset_chain_id, "noble-1");
}

#[test]
fn test_is_ethereum_chain() {
    assert!(SkipGoClient::is_ethereum_chain("1"));
    assert!(SkipGoClient::is_ethereum_chain("ethereum"));
    assert!(!SkipGoClient::is_ethereum_chain("cosmoshub-4"));
    assert!(!SkipGoClient::is_ethereum_chain("osmosis-1"));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-solver test_eureka_route
cargo test -p atom-intents-solver test_is_ethereum
```

Expected: FAIL with "cannot find struct `EurekaRouteRequest`"

**Step 3: Write minimal implementation**

Add to `crates/solver/src/skipgo.rs`:

```rust
/// Eureka-specific route request for Ethereum ↔ Cosmos
#[derive(Debug, Serialize)]
pub struct EurekaRouteRequest {
    pub amount_in: String,
    pub source_asset_denom: String,
    pub source_asset_chain_id: String,
    pub dest_asset_denom: String,
    pub dest_asset_chain_id: String,
}

/// Eureka route response
#[derive(Debug, Deserialize)]
pub struct EurekaRouteResponse {
    pub amount_in: String,
    pub amount_out: String,
    pub operations: Vec<EurekaOperation>,
    pub estimated_time_seconds: u64,
    pub estimated_fees_usd: String,
}

#[derive(Debug, Deserialize)]
pub struct EurekaOperation {
    pub op_type: String,
    pub from_chain_id: String,
    pub to_chain_id: String,
}

impl SkipGoClient {
    /// Check if a chain ID represents Ethereum
    pub fn is_ethereum_chain(chain_id: &str) -> bool {
        matches!(chain_id, "1" | "ethereum" | "eth-mainnet")
    }

    /// Get a quote for an Eureka transfer (Ethereum ↔ Cosmos)
    pub async fn get_eureka_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
        source_chain: &str,
        dest_chain: &str,
    ) -> Result<EurekaQuote, DexError> {
        let request = EurekaRouteRequest {
            amount_in: amount.to_string(),
            source_asset_denom: input_denom.to_string(),
            source_asset_chain_id: source_chain.to_string(),
            dest_asset_denom: output_denom.to_string(),
            dest_asset_chain_id: dest_chain.to_string(),
        };

        let url = format!("{}/v2/fungible/route", self.base_url);

        debug!("Querying Skip Go Eureka route: {} with {:?}", url, request);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Skip Go Eureka API error: {} - {}", status, body);
            return Err(DexError::QueryFailed(format!("HTTP {}: {}", status, body)));
        }

        let route: EurekaRouteResponse = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse response: {}", e)))?;

        let output_amount = route
            .amount_out
            .parse::<u128>()
            .map_err(|e| DexError::QueryFailed(format!("Invalid amount_out: {}", e)))?;

        Ok(EurekaQuote {
            input_amount: amount,
            output_amount,
            estimated_time_secs: route.estimated_time_seconds,
            estimated_cost_usd: route.estimated_fees_usd,
            is_eureka: true,
        })
    }
}

/// Quote from Eureka route
#[derive(Debug, Clone)]
pub struct EurekaQuote {
    pub input_amount: u128,
    pub output_amount: u128,
    pub estimated_time_secs: u64,
    pub estimated_cost_usd: String,
    pub is_eureka: bool,
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-solver test_eureka_route
cargo test -p atom-intents-solver test_is_ethereum
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/solver/src/skipgo.rs
git commit -m "feat(solver): extend Skip Go client with Eureka support

Add Eureka API integration to Skip Go client:
- EurekaRouteRequest/Response types
- is_ethereum_chain helper
- get_eureka_quote method for Ethereum ↔ Cosmos quotes
- EurekaQuote type with time/cost estimates

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 6: Create Eureka Solver Implementation

**Files:**
- Create: `crates/solver/src/eureka.rs`
- Modify: `crates/solver/src/lib.rs`
- Test: `crates/solver/src/eureka.rs` (inline tests)

**Step 1: Write the failing test**

Create `crates/solver/src/eureka.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, Intent, OutputSpec, ExecutionConstraints, AssetPreference};
    use cosmwasm_std::Uint128;

    fn mock_intent(asset_pref: AssetPreference) -> Intent {
        Intent {
            id: "test-intent-1".to_string(),
            version: "1".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uatom".to_string(),
                amount: Uint128::new(1_000_000),
            },
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(10_000_000),
            },
            fill_config: Default::default(),
            constraints: ExecutionConstraints::new(9999999999)
                .with_asset_preference(asset_pref),
            signature: Default::default(),
            public_key: Default::default(),
            created_at: 0,
            expires_at: 9999999999,
        }
    }

    #[test]
    fn test_eureka_solver_can_deliver_native() {
        let solver = EurekaSolver::new("test-eureka", SkipGoClient::mainnet());

        // NativeOnly - solver must convert bridged to native
        let intent = mock_intent(AssetPreference::NativeOnly);
        assert!(!solver.can_deliver_bridged(&intent.constraints));

        // AcceptBridged with eureka/usdc - can deliver bridged
        let intent = mock_intent(AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        });
        assert!(solver.can_deliver_bridged(&intent.constraints));
    }

    #[test]
    fn test_eureka_solver_id() {
        let solver = EurekaSolver::new("my-eureka-solver", SkipGoClient::mainnet());
        assert_eq!(solver.id(), "my-eureka-solver");
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-solver test_eureka_solver
```

Expected: FAIL with "cannot find struct `EurekaSolver`"

**Step 3: Write minimal implementation**

Create `crates/solver/src/eureka.rs`:

```rust
//! Eureka cross-ecosystem solver
//!
//! Sources liquidity from Ethereum via IBC Eureka and delivers to Cosmos users.

use async_trait::async_trait;
use atom_intents_types::{
    AssetPreference, Ecosystem, EurekaDirection, ExecutionConstraints, ExecutionPlan,
    Intent, ProposedFill, Solution, SolutionMetadata, SolveContext, SolverCapabilities,
    SolverCapacity, TradingPair,
};
use cosmwasm_std::Uint128;
use tracing::{debug, info, warn};

use crate::{SkipGoClient, SolveError, Solver};

/// Eureka solver that sources liquidity from Ethereum
pub struct EurekaSolver {
    id: String,
    skip_go: SkipGoClient,
    supported_pairs: Vec<TradingPair>,
    capabilities: SolverCapabilities,
}

impl EurekaSolver {
    pub fn new(id: impl Into<String>, skip_go: SkipGoClient) -> Self {
        Self {
            id: id.into(),
            skip_go,
            supported_pairs: vec![
                // ATOM/USDC via Ethereum liquidity
                TradingPair {
                    base: "uatom".to_string(),
                    quote: "uusdc".to_string(),
                },
                // OSMO/USDC via Ethereum liquidity
                TradingPair {
                    base: "uosmo".to_string(),
                    quote: "uusdc".to_string(),
                },
            ],
            capabilities: SolverCapabilities {
                dex_routing: false,
                intent_matching: false,
                cex_backstop: false,
                cross_ecosystem: true,
                max_fill_size_usd: 1_000_000,
            },
        }
    }

    /// Check if user's asset preference allows bridged assets
    pub fn can_deliver_bridged(&self, constraints: &ExecutionConstraints) -> bool {
        match &constraints.asset_preference {
            AssetPreference::NativeOnly => false,
            AssetPreference::AcceptBridged { allowed_denoms } => {
                allowed_denoms.iter().any(|d| d.starts_with("eureka/"))
            }
            AssetPreference::AnyEquivalent => true,
        }
    }

    /// Check if settlement time is acceptable
    fn is_time_acceptable(&self, constraints: &ExecutionConstraints) -> bool {
        match constraints.max_settlement_secs {
            Some(max) => max >= 25, // Eureka takes ~25s
            None => true,
        }
    }
}

#[async_trait]
impl Solver for EurekaSolver {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_pairs(&self) -> &[TradingPair] {
        &self.supported_pairs
    }

    fn capabilities(&self) -> &SolverCapabilities {
        &self.capabilities
    }

    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError> {
        // Check time constraints
        if !self.is_time_acceptable(&intent.constraints) {
            return Err(SolveError::ConstraintViolation(
                "Settlement time exceeds max_settlement_secs".to_string(),
            ));
        }

        let can_deliver_bridged = self.can_deliver_bridged(&intent.constraints);

        info!(
            "Eureka solver {} quoting intent {}, can_deliver_bridged={}",
            self.id, intent.id, can_deliver_bridged
        );

        // Get quote from Skip Go Eureka API
        let quote = self
            .skip_go
            .get_eureka_quote(
                &intent.input.denom,
                &intent.output.denom,
                intent.input.amount.u128(),
                "1", // Ethereum mainnet
                &intent.output.chain_id,
            )
            .await
            .map_err(|e| SolveError::QuoteFailed(e.to_string()))?;

        // If user wants native and we got bridged, we need conversion cost
        let (final_output, output_denom, is_native) = if can_deliver_bridged {
            (quote.output_amount, format!("eureka/{}", intent.output.denom), false)
        } else {
            // Deduct estimated conversion cost (~0.3%)
            let conversion_fee = quote.output_amount * 3 / 1000;
            (quote.output_amount - conversion_fee, intent.output.denom.clone(), true)
        };

        // Check if output meets minimum
        if final_output < intent.output.min_amount.u128() {
            return Err(SolveError::InsufficientOutput {
                required: intent.output.min_amount.u128(),
                available: final_output,
            });
        }

        let metadata = SolutionMetadata::eureka(
            EurekaDirection::EthereumToCosmos,
            None,
            &output_denom,
            is_native,
        );

        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: intent.input.amount,
                output_amount: Uint128::new(final_output),
                solver_fee_bps: 10, // 0.1% solver fee
            },
            execution: ExecutionPlan::CrossEcosystem {
                bridge: "eureka".to_string(),
                target: "ethereum".to_string(),
            },
            valid_until: intent.expires_at,
            bond: Uint128::new(final_output / 10), // 10% bond
            metadata: Some(metadata),
        })
    }

    async fn capacity(&self, pair: &TradingPair) -> Result<SolverCapacity, SolveError> {
        // Eureka has deep Ethereum liquidity
        Ok(SolverCapacity {
            max_immediate: Uint128::new(1_000_000_000_000), // $1M+
            available_liquidity: Uint128::new(1_000_000_000_000),
            estimated_time_ms: 25_000, // 25 seconds
        })
    }

    async fn health_check(&self) -> bool {
        // TODO: Check Skip Go Eureka API health
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, OutputSpec, FillConfig};
    use cosmwasm_std::Binary;

    fn mock_intent(asset_pref: AssetPreference) -> Intent {
        Intent {
            id: "test-intent-1".to_string(),
            version: "1".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uatom".to_string(),
                amount: Uint128::new(1_000_000),
            },
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(10_000_000),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::new(9999999999)
                .with_asset_preference(asset_pref),
            signature: Binary::default(),
            public_key: Binary::default(),
            created_at: 0,
            expires_at: 9999999999,
        }
    }

    #[test]
    fn test_eureka_solver_can_deliver_native() {
        let solver = EurekaSolver::new("test-eureka", SkipGoClient::mainnet());

        let intent = mock_intent(AssetPreference::NativeOnly);
        assert!(!solver.can_deliver_bridged(&intent.constraints));

        let intent = mock_intent(AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        });
        assert!(solver.can_deliver_bridged(&intent.constraints));

        let intent = mock_intent(AssetPreference::AnyEquivalent);
        assert!(solver.can_deliver_bridged(&intent.constraints));
    }

    #[test]
    fn test_eureka_solver_id() {
        let solver = EurekaSolver::new("my-eureka-solver", SkipGoClient::mainnet());
        assert_eq!(solver.id(), "my-eureka-solver");
    }

    #[test]
    fn test_eureka_solver_capabilities() {
        let solver = EurekaSolver::new("test", SkipGoClient::mainnet());
        let caps = solver.capabilities();

        assert!(caps.cross_ecosystem);
        assert!(!caps.dex_routing);
        assert_eq!(caps.max_fill_size_usd, 1_000_000);
    }

    #[test]
    fn test_time_constraint_check() {
        let solver = EurekaSolver::new("test", SkipGoClient::mainnet());

        // No limit - acceptable
        let constraints = ExecutionConstraints::new(9999999999);
        assert!(solver.is_time_acceptable(&constraints));

        // 30s limit - acceptable (Eureka is ~25s)
        let constraints = ExecutionConstraints::new(9999999999)
            .with_max_settlement_secs(30);
        assert!(solver.is_time_acceptable(&constraints));

        // 10s limit - not acceptable
        let constraints = ExecutionConstraints::new(9999999999)
            .with_max_settlement_secs(10);
        assert!(!solver.is_time_acceptable(&constraints));
    }
}
```

Update `crates/solver/src/lib.rs`:

```rust
mod eureka;
pub use eureka::EurekaSolver;
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-solver test_eureka_solver
cargo test -p atom-intents-solver test_time_constraint
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/solver/src/eureka.rs crates/solver/src/lib.rs
git commit -m "feat(solver): add EurekaSolver for Ethereum liquidity

Implement Eureka cross-ecosystem solver:
- Sources liquidity from Ethereum via Skip Go Eureka API
- Respects AssetPreference (NativeOnly vs AcceptBridged)
- Handles conversion costs when delivering native
- Time constraint validation (25s minimum)
- Full Solver trait implementation

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 7: Add Eureka Routes to Route Registry

**Files:**
- Modify: `crates/settlement/src/routing.rs`
- Test: `crates/settlement/src/routing.rs` (inline tests)

**Step 1: Write the failing test**

Add to tests in `crates/settlement/src/routing.rs`:

```rust
#[test]
fn test_eureka_route_to_ethereum() {
    let registry = RouteRegistry::with_mainnet_routes();

    let route = registry.find_route("cosmoshub-4", "ethereum-1");
    assert!(route.is_some());

    let route = route.unwrap();
    assert_eq!(route.dest_chain, "ethereum-1");
    assert!(route.estimated_time_seconds >= 15);
}

#[test]
fn test_eureka_route_from_ethereum() {
    let registry = RouteRegistry::with_mainnet_routes();

    let route = registry.find_route("ethereum-1", "cosmoshub-4");
    assert!(route.is_some());

    let route = route.unwrap();
    assert_eq!(route.source_chain, "ethereum-1");
}

#[test]
fn test_is_eureka_route() {
    let eureka_route = Route {
        source_chain: "cosmoshub-4".to_string(),
        dest_chain: "ethereum-1".to_string(),
        hops: vec![RouteHop {
            chain_id: "ethereum-1".to_string(),
            channel_id: "channel-eureka".to_string(),
            port_id: "transfer".to_string(),
        }],
        estimated_time_seconds: 25,
        estimated_cost_units: 3_000_000, // ~$3 in gas
    };

    assert!(eureka_route.is_eureka());

    let cosmos_route = Route {
        source_chain: "cosmoshub-4".to_string(),
        dest_chain: "osmosis-1".to_string(),
        hops: vec![RouteHop {
            chain_id: "osmosis-1".to_string(),
            channel_id: "channel-141".to_string(),
            port_id: "transfer".to_string(),
        }],
        estimated_time_seconds: 6,
        estimated_cost_units: 50000,
    };

    assert!(!cosmos_route.is_eureka());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p atom-intents-settlement test_eureka_route
cargo test -p atom-intents-settlement test_is_eureka
```

Expected: FAIL with "no route found" or "method not found"

**Step 3: Write minimal implementation**

Add to `crates/settlement/src/routing.rs`:

```rust
impl Route {
    /// Check if this is an Eureka route (involves Ethereum)
    pub fn is_eureka(&self) -> bool {
        self.source_chain.starts_with("ethereum") ||
        self.dest_chain.starts_with("ethereum") ||
        self.hops.iter().any(|h| h.chain_id.starts_with("ethereum"))
    }
}

impl RouteRegistry {
    /// Create a registry with mainnet routes including Eureka
    pub fn with_mainnet_routes() -> Self {
        let channel_registry = ChannelRegistry::with_mainnet_channels();
        let mut registry = Self::new(channel_registry);

        // ... existing routes ...

        // === Eureka Routes (Cosmos Hub ↔ Ethereum) ===

        // Cosmos Hub → Ethereum via Eureka
        registry.add_route(Route {
            source_chain: "cosmoshub-4".to_string(),
            dest_chain: "ethereum-1".to_string(),
            hops: vec![RouteHop {
                chain_id: "ethereum-1".to_string(),
                channel_id: "channel-eureka".to_string(),
                port_id: "transfer".to_string(),
            }],
            estimated_time_seconds: 25,
            estimated_cost_units: 3_000_000, // ~$3 in Ethereum gas
        });

        // Ethereum → Cosmos Hub via Eureka
        registry.add_route(Route {
            source_chain: "ethereum-1".to_string(),
            dest_chain: "cosmoshub-4".to_string(),
            hops: vec![RouteHop {
                chain_id: "cosmoshub-4".to_string(),
                channel_id: "channel-eureka".to_string(),
                port_id: "transfer".to_string(),
            }],
            estimated_time_seconds: 25,
            estimated_cost_units: 3_000_000,
        });

        // Ethereum → Osmosis via Eureka + IBC
        registry.add_route(Route {
            source_chain: "ethereum-1".to_string(),
            dest_chain: "osmosis-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-eureka".to_string(),
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "osmosis-1".to_string(),
                    channel_id: "channel-141".to_string(),
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 31, // 25s Eureka + 6s IBC
            estimated_cost_units: 3_050_000,
        });

        registry
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test -p atom-intents-settlement test_eureka_route
cargo test -p atom-intents-settlement test_is_eureka
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/settlement/src/routing.rs
git commit -m "feat(settlement): add Eureka routes to RouteRegistry

Add IBC Eureka routes for Ethereum connectivity:
- cosmoshub-4 ↔ ethereum-1 direct Eureka routes
- ethereum-1 → osmosis-1 via Hub (Eureka + IBC)
- is_eureka() helper method on Route
- 25s estimated time, ~$3 gas cost estimates

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 8: Integration Test

**Files:**
- Create: `crates/solver/tests/eureka_integration_tests.rs`

**Step 1: Write the integration test**

```rust
//! Integration tests for Eureka solver

use atom_intents_solver::{EurekaSolver, SkipGoClient, Solver};
use atom_intents_types::{
    Asset, AssetPreference, ExecutionConstraints, FillConfig, Intent, OutputSpec,
    SettlementPreference, SolveContext,
};
use cosmwasm_std::{Binary, Uint128};

fn create_test_intent(
    asset_pref: AssetPreference,
    settlement_pref: SettlementPreference,
) -> Intent {
    Intent {
        id: "integration-test-1".to_string(),
        version: "1".to_string(),
        nonce: 1,
        user: "cosmos1testuser".to_string(),
        input: Asset {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            amount: Uint128::new(100_000_000), // 100 ATOM
        },
        output: OutputSpec {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uusdc".to_string(),
            min_amount: Uint128::new(500_000_000), // 500 USDC min
        },
        fill_config: FillConfig::default(),
        constraints: ExecutionConstraints::new(9999999999)
            .with_asset_preference(asset_pref)
            .with_settlement_preference(settlement_pref),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 0,
        expires_at: 9999999999,
    }
}

#[test]
fn test_eureka_solver_setup() {
    let solver = EurekaSolver::new("integration-test-solver", SkipGoClient::mainnet());

    assert_eq!(solver.id(), "integration-test-solver");
    assert!(solver.capabilities().cross_ecosystem);
    assert!(!solver.supported_pairs().is_empty());
}

#[test]
fn test_eureka_solver_constraint_validation() {
    let solver = EurekaSolver::new("test", SkipGoClient::mainnet());

    // Test NativeOnly preference
    let intent = create_test_intent(AssetPreference::NativeOnly, SettlementPreference::Cost);
    assert!(!solver.can_deliver_bridged(&intent.constraints));

    // Test AcceptBridged preference
    let intent = create_test_intent(
        AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        },
        SettlementPreference::Cost,
    );
    assert!(solver.can_deliver_bridged(&intent.constraints));

    // Test AnyEquivalent preference
    let intent = create_test_intent(AssetPreference::AnyEquivalent, SettlementPreference::Cost);
    assert!(solver.can_deliver_bridged(&intent.constraints));
}

#[tokio::test]
async fn test_eureka_solver_capacity() {
    let solver = EurekaSolver::new("test", SkipGoClient::mainnet());
    let pair = solver.supported_pairs().first().unwrap().clone();

    let capacity = solver.capacity(&pair).await.unwrap();

    // Eureka has deep liquidity
    assert!(capacity.max_immediate > Uint128::zero());
    assert!(capacity.estimated_time_ms >= 20_000); // At least 20s
}

#[tokio::test]
#[ignore] // Requires network access to Skip Go API
async fn test_eureka_solver_live_quote() {
    let solver = EurekaSolver::new("live-test", SkipGoClient::mainnet());

    let intent = create_test_intent(
        AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        },
        SettlementPreference::Cost,
    );

    let ctx = SolveContext {
        matched_amount: Uint128::zero(),
        remaining: intent.input.amount,
        oracle_price: "10.0".to_string(),
    };

    match solver.solve(&intent, &ctx).await {
        Ok(solution) => {
            println!("Got solution: {:?}", solution);
            assert_eq!(solution.solver_id, "live-test");
            assert!(solution.fill.output_amount > Uint128::zero());
            assert!(solution.metadata.is_some());
        }
        Err(e) => {
            println!("Solve error (may be expected): {:?}", e);
        }
    }
}
```

**Step 2: Run tests**

```bash
cargo test -p atom-intents-solver --test eureka_integration_tests
```

For live API test:
```bash
cargo test -p atom-intents-solver --test eureka_integration_tests -- --ignored
```

**Step 3: Commit**

```bash
git add crates/solver/tests/eureka_integration_tests.rs
git commit -m "test(solver): add Eureka integration tests

Integration tests for EurekaSolver:
- Solver setup and configuration
- Constraint validation (AssetPreference)
- Capacity queries
- Live quote test (ignored by default, requires network)

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 9: Update lib.rs Exports and Documentation

**Files:**
- Modify: `crates/types/src/lib.rs`
- Modify: `crates/solver/src/lib.rs`
- Modify: `crates/settlement/src/lib.rs`

**Step 1: Update exports**

Update `crates/types/src/lib.rs`:
```rust
mod asset;
mod cancellation;
mod ecosystem;  // NEW
mod execution;
mod fill;
mod intent;
mod solution;
mod trading;
mod verification;

pub use asset::*;
pub use cancellation::*;
pub use ecosystem::*;  // NEW
pub use execution::*;
pub use fill::*;
pub use intent::*;
pub use solution::*;
pub use trading::*;
pub use verification::*;
```

Update `crates/solver/src/lib.rs`:
```rust
mod aggregator;
mod astroport;
mod cex;
mod denom;
mod dex;
mod error;
mod eureka;  // NEW
mod fees;
mod oracle;
mod osmosis;
mod reputation;
mod skipgo;
mod traits;

pub use aggregator::*;
pub use astroport::*;
pub use cex::*;
pub use denom::*;
pub use dex::*;
pub use error::*;
pub use eureka::*;  // NEW
pub use fees::*;
pub use oracle::*;
pub use osmosis::*;
pub use reputation::*;
pub use skipgo::*;
pub use traits::*;
```

Update `crates/settlement/src/lib.rs` to export new types:
```rust
pub use ibc::{EurekaDirection, build_eureka_memo};  // NEW exports
```

**Step 2: Verify compilation**

```bash
cargo build --workspace
cargo test --workspace
```

**Step 3: Commit**

```bash
git add crates/types/src/lib.rs crates/solver/src/lib.rs crates/settlement/src/lib.rs
git commit -m "chore: update lib.rs exports for Eureka integration

Export new modules and types:
- types: ecosystem module (Ecosystem, SettlementPath, etc.)
- solver: eureka module (EurekaSolver)
- settlement: EurekaDirection, build_eureka_memo

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 10: Final Verification

**Step 1: Run all tests**

```bash
cargo test --workspace
```

**Step 2: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

**Step 3: Run fmt**

```bash
cargo fmt --all
```

**Step 4: Create final commit if any formatting changes**

```bash
git add -A
git commit -m "style: apply cargo fmt

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Summary

This plan implements Phase 1 Eureka integration in 10 tasks:

| Task | Component | Description |
|------|-----------|-------------|
| 1 | types | AssetPreference and SettlementPreference |
| 2 | settlement | Eureka flow type and timeout |
| 3 | types | Ecosystem and SettlementPath types |
| 4 | types | SolutionMetadata for cross-ecosystem |
| 5 | solver | Skip Go Eureka API extension |
| 6 | solver | EurekaSolver implementation |
| 7 | settlement | Eureka routes in RouteRegistry |
| 8 | solver | Integration tests |
| 9 | all | Export updates |
| 10 | all | Final verification |

After completion, solvers can source liquidity from Ethereum via IBC Eureka and compete in atom-intents auctions alongside native Cosmos solvers.
