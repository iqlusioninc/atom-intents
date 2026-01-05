# Phase 2: Ethereum Escrow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Enable Ethereum users to escrow funds via IBC Eureka, with solvers fronting Cosmos-side funds and taking settlement risk.

**Architecture:** Extend types, add packet monitoring, update escrow contract, and enhance EurekaSolver with fronting capability.

**Tech Stack:** Rust, async-trait, cosmwasm-std, tokio, serde

---

## Task 1: Add EscrowStatus and EthereumEscrowedIntent Types

**Files:**
- Create: `crates/types/src/ethereum_escrow.rs`
- Modify: `crates/types/src/lib.rs`
- Test: `crates/types/src/ethereum_escrow.rs` (inline tests)

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escrow_status_pending() {
        let status = EscrowStatus::Pending;
        assert!(!status.is_finalized());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_escrow_status_received() {
        let status = EscrowStatus::Received {
            packet_id: "pkt-123".to_string(),
            received_at: 1704067200,
        };
        assert!(!status.is_finalized());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_escrow_status_finalized() {
        let status = EscrowStatus::Finalized {
            packet_id: "pkt-123".to_string(),
            finalized_at: 1704067230,
        };
        assert!(status.is_finalized());
    }

    #[test]
    fn test_ethereum_escrowed_intent() {
        let intent = EthereumEscrowedIntent {
            base: mock_intent(),
            eth_sender: "0x1234...".to_string(),
            expected_packet_hash: Some("0xabcd...".to_string()),
            escrow_status: EscrowStatus::Pending,
        };
        assert!(intent.is_pending());
    }
}
```

**Step 2: Implement types**

```rust
use cosmwasm_schema::cw_serde;
use crate::Intent;

/// Status of Eureka escrow for an Ethereum-originated intent
#[cw_serde]
pub enum EscrowStatus {
    Pending,
    Received { packet_id: String, received_at: u64 },
    Finalized { packet_id: String, finalized_at: u64 },
    Failed { reason: String },
}

impl EscrowStatus {
    pub fn is_finalized(&self) -> bool {
        matches!(self, Self::Finalized { .. })
    }
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Intent with Ethereum-side escrow via IBC Eureka
#[cw_serde]
pub struct EthereumEscrowedIntent {
    pub base: Intent,
    pub eth_sender: String,
    pub expected_packet_hash: Option<String>,
    pub escrow_status: EscrowStatus,
}

impl EthereumEscrowedIntent {
    pub fn is_pending(&self) -> bool {
        matches!(self.escrow_status, EscrowStatus::Pending)
    }
    pub fn is_ready_for_fronting(&self) -> bool {
        matches!(self.escrow_status, EscrowStatus::Received { .. })
    }
    pub fn is_finalized(&self) -> bool {
        self.escrow_status.is_finalized()
    }
}
```

**Step 3: Update lib.rs exports**

**Step 4: Run tests, commit**

---

## Task 2: Add Settlement Risk Pricing Types

**Files:**
- Create: `crates/types/src/risk.rs`
- Modify: `crates/types/src/lib.rs`
- Test: `crates/types/src/risk.rs` (inline tests)

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::Uint128;
    use rust_decimal_macros::dec;

    #[test]
    fn test_required_bond_calculation() {
        let pricing = SettlementRiskPricing {
            failure_probability: dec!(0.001), // 0.1%
            expected_finality_secs: 30,
            bond_multiplier: dec!(2.0),
            risk_premium_bps: 50, // 0.5%
        };

        let fronted = Uint128::new(1_000_000);
        let bond = pricing.required_bond(fronted);
        assert_eq!(bond, Uint128::new(2_000_000)); // 2x
    }

    #[test]
    fn test_risk_adjusted_quote() {
        let pricing = SettlementRiskPricing {
            failure_probability: dec!(0.001),
            expected_finality_secs: 30,
            bond_multiplier: dec!(2.0),
            risk_premium_bps: 100, // 1%
        };

        let base_output = Uint128::new(1_000_000);
        let adjusted = pricing.adjust_quote(base_output);
        assert_eq!(adjusted, Uint128::new(990_000)); // 1% deducted
    }
}
```

**Step 2: Implement SettlementRiskPricing**

```rust
use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;

#[cw_serde]
pub struct SettlementRiskPricing {
    pub failure_probability: Decimal,
    pub expected_finality_secs: u64,
    pub bond_multiplier: Decimal,
    pub risk_premium_bps: u32,
}

impl SettlementRiskPricing {
    pub fn required_bond(&self, fronted_amount: Uint128) -> Uint128 {
        let multiplied = fronted_amount.u128() as f64 * self.bond_multiplier.to_string().parse::<f64>().unwrap();
        Uint128::new(multiplied as u128)
    }

    pub fn adjust_quote(&self, base_output: Uint128) -> Uint128 {
        let premium = base_output.u128() * self.risk_premium_bps as u128 / 10000;
        Uint128::new(base_output.u128() - premium)
    }

    pub fn default_eureka() -> Self {
        Self {
            failure_probability: Decimal::from_str("0.001").unwrap(),
            expected_finality_secs: 30,
            bond_multiplier: Decimal::from_str("2.0").unwrap(),
            risk_premium_bps: 50,
        }
    }
}
```

---

## Task 3: Add EurekaPacketStatus and MonitorError Types

**Files:**
- Create: `crates/types/src/packet.rs`
- Modify: `crates/types/src/lib.rs`

**Types needed:**
- `EurekaPacketStatus` enum (NotFound, Pending, Finalized, Failed)
- `MonitorError` enum for packet monitoring errors
- `EurekaPacketInfo` struct with packet details

---

## Task 4: Implement Eureka Packet Monitor Trait and Mock

**Files:**
- Create: `crates/relayer/src/eureka_monitor.rs`
- Modify: `crates/relayer/src/lib.rs`

**Implement:**
- `EurekaPacketMonitor` trait with async methods
- `MockEurekaMonitor` for testing
- `watch_for_packet`, `get_packet_status`, `verify_finality` methods

---

## Task 5: Add Escrow Contract Messages for Ethereum Escrow

**Files:**
- Modify: `contracts/escrow/src/msg.rs`
- Modify: `contracts/escrow/src/contract.rs`
- Modify: `contracts/escrow/src/state.rs`

**New messages:**
- `RegisterEthereumEscrowIntent`
- `NotifyEurekaPacketReceived`
- `NotifyEurekaFinalized`
- `FrontSettlement`
- `ClaimEurekaEscrow`

---

## Task 6: Implement Escrow Contract Ethereum Escrow Logic

**Files:**
- Modify: `contracts/escrow/src/contract.rs`
- Create: `contracts/escrow/src/ethereum_escrow.rs`

**Implement:**
- Intent registration with pending escrow
- Packet notification handling
- Fronting logic with risk bond
- Escrow claim after finality

---

## Task 7: Update EurekaSolver with Fronting Capability

**Files:**
- Modify: `crates/solver/src/eureka.rs`

**Add:**
- `can_front_settlement()` method
- `calculate_fronting_bond()` method
- Risk-adjusted quoting for fronted solutions
- Settlement risk tracking

---

## Task 8: Integration Tests for Ethereum Escrow Flow

**Files:**
- Create: `crates/solver/tests/ethereum_escrow_tests.rs`
- Create: `contracts/escrow/tests/ethereum_escrow_tests.rs`

**Test scenarios:**
- Happy path: packet received, fronted, finalized
- Failure case: packet fails, bond slashed
- Timeout case: packet times out

---

## Task 9: Update lib.rs Exports

**Files:**
- `crates/types/src/lib.rs`
- `crates/relayer/src/lib.rs`
- `contracts/escrow/src/lib.rs`

---

## Task 10: Final Verification

- Run all workspace tests
- Clippy with -D warnings
- Cargo fmt
