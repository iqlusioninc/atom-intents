# Bidirectional Escrow Design

> **Status**: Design Draft
> **Author**: Claude Code
> **Date**: 2026-01-05

## Overview

Extend the Ethereum escrow system to support **bidirectional** cross-ecosystem transfers with solver fronting:

1. **Inbound** (Ethereum → Cosmos): User escrows on Ethereum, solver fronts on Cosmos ✅ *Implemented*
2. **Outbound** (Cosmos → Ethereum): User escrows on Cosmos, solver fronts on Ethereum 🆕 *This design*

Both directions use the same core pattern: **escrow + solver fronting + proof verification**.

## Motivation

Users want to:
- **Buy**: Swap ETH/USDC for ATOM (inbound) ✅
- **Sell**: Swap ATOM for ETH/USDC on Ethereum (outbound) 🆕

The outbound direction is particularly valuable because:
1. Users often want to exit to stablecoins on Ethereum
2. Ethereum has deeper liquidity for most assets
3. Cosmos finality is fast (~6s), reducing solver risk

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           COSMOS HUB                                     │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    ESCROW CONTRACT                               │   │
│  │                                                                   │   │
│  │  ┌─────────────────┐         ┌──────────────────┐               │   │
│  │  │ INBOUND ESCROW  │         │ OUTBOUND ESCROW  │               │   │
│  │  │ (ETH → Cosmos)  │         │ (Cosmos → ETH)   │               │   │
│  │  │                 │         │                  │               │   │
│  │  │ • EthereumEscrow│         │ • OutboundEscrow │               │   │
│  │  │ • Eureka proofs │         │ • Hyperlane/IBC  │               │   │
│  │  │ • ~20min wait   │         │ • ~6sec finality │               │   │
│  │  └─────────────────┘         └──────────────────┘               │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                │                           │
                │ Eureka (ZK proofs)        │ Hyperlane / Reverse Eureka
                ▼                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           ETHEREUM                                       │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    SOLVER CONTRACTS                              │   │
│  │                                                                   │   │
│  │  User sends ETH ──────────────► Solver receives ETH              │   │
│  │  (Inbound: source)              (Inbound: claims escrow)         │   │
│  │                                                                   │   │
│  │  Solver fronts USDC ◄────────── User receives USDC               │   │
│  │  (Outbound: fronts)             (Outbound: destination)          │   │
│  │                                                                   │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

## State Machines

### Inbound (Ethereum → Cosmos) - Existing

```
┌─────────┐  RegisterEthereumEscrowIntent  ┌─────────┐
│ (none)  │ ─────────────────────────────► │ Pending │
└─────────┘                                └────┬────┘
                                                │
                          NotifyEurekaPacketReceived
                                                │
                                                ▼
                                          ┌──────────┐
                              ┌───────────│ Received │
                              │           └────┬─────┘
                              │                │
                   FrontSettlement    NotifyEurekaFinalized
                              │                │ (no fronting)
                              ▼                ▼
                        ┌─────────┐      ┌───────────┐
                        │ Fronted │      │ Finalized │◄─── (direct settlement)
                        └────┬────┘      └───────────┘
                             │
               NotifyEurekaFinalized
                             │
                             ▼
                       ┌───────────┐
                       │ Finalized │
                       └─────┬─────┘
                             │
                    ClaimEurekaEscrow
                             │
                             ▼
                       ┌─────────┐
                       │ Claimed │
                       └─────────┘
```

### Outbound (Cosmos → Ethereum) - New

```
┌─────────┐  RegisterOutboundEscrowIntent   ┌─────────┐
│ (none)  │ ──────────────────────────────► │ Pending │
└─────────┘   (user locks ATOM on Hub)      └────┬────┘
                                                 │
                              SolverAcceptOutbound
                           (solver commits to fill)
                                                 │
                                                 ▼
                                           ┌──────────┐
                                           │ Accepted │
                                           └────┬─────┘
                                                │
                               NotifyOutboundFilled
                          (proof of ETH transfer to user)
                                                │
                                                ▼
                                           ┌────────┐
                                           │ Filled │
                                           └────┬───┘
                                                │
                                   ReleaseOutboundEscrow
                                 (release ATOM to solver)
                                                │
                                                ▼
                                          ┌──────────┐
                                          │ Released │
                                          └──────────┘

                    ─── OR (failure path) ───

                                           ┌────────┐
                         HandleTimeout ──► │ Failed │ ──► Refund to user
                                           └────────┘
```

## Data Structures

### Outbound Escrow State

```rust
/// Outbound escrow for Cosmos → external chain transfers
#[cw_serde]
pub struct OutboundEscrow {
    /// Intent ID
    pub intent_id: String,

    /// User who escrowed funds on Hub
    pub user: Addr,

    /// Amount escrowed on Hub
    pub escrowed_amount: Uint128,

    /// Denom escrowed (e.g., "uatom")
    pub escrowed_denom: String,

    /// Destination chain (e.g., "ethereum", "base")
    pub destination_chain: String,

    /// User's address on destination chain
    pub destination_address: String,

    /// Expected output amount on destination
    pub expected_output: Uint128,

    /// Expected output denom (e.g., "USDC")
    pub expected_output_denom: String,

    /// When escrow was created
    pub created_at: u64,

    /// Timeout for solver to fill
    pub timeout: u64,

    /// Current status
    pub status: OutboundEscrowStatus,

    /// Solver info (once accepted)
    pub solver: Option<OutboundSolverInfo>,

    /// Proof of fill (once filled)
    pub fill_proof: Option<FillProof>,
}

#[cw_serde]
pub enum OutboundEscrowStatus {
    /// Waiting for solver to accept
    Pending,
    /// Solver committed to fill
    Accepted,
    /// Solver filled on destination, proof provided
    Filled,
    /// Escrow released to solver
    Released,
    /// Failed (timeout, invalid proof)
    Failed { reason: String },
    /// Refunded to user
    Refunded,
}

#[cw_serde]
pub struct OutboundSolverInfo {
    pub solver_id: String,
    pub solver_hub_address: Addr,
    pub solver_dest_address: String,
    pub accepted_at: u64,
    /// Bond posted by solver (optional for outbound)
    pub bond_amount: Option<Uint128>,
}

#[cw_serde]
pub struct FillProof {
    /// Proof type (Hyperlane, Eureka, etc.)
    pub proof_type: ProofType,
    /// Raw proof data
    pub proof_data: Binary,
    /// Transaction hash on destination chain
    pub tx_hash: String,
    /// Block number on destination chain
    pub block_number: u64,
    /// When proof was submitted
    pub submitted_at: u64,
}

#[cw_serde]
pub enum ProofType {
    /// Hyperlane message with validator signatures
    Hyperlane,
    /// IBC Eureka reverse proof (if available)
    EurekaReverse,
    /// Optimistic with challenge period
    Optimistic { challenge_period_secs: u64 },
    /// Trusted relayer attestation (for testnets)
    TrustedRelayer,
}
```

### Contract Messages

```rust
#[cw_serde]
pub enum ExecuteMsg {
    // ... existing messages ...

    // ═══════════════════════════════════════════════════════════════
    // OUTBOUND ESCROW MESSAGES (Cosmos → External)
    // ═══════════════════════════════════════════════════════════════

    /// User locks funds for outbound transfer
    /// Funds attached to message
    RegisterOutboundEscrowIntent {
        intent_id: String,
        destination_chain: String,
        destination_address: String,
        expected_output: Uint128,
        expected_output_denom: String,
        timeout_secs: u64,
    },

    /// Solver commits to fill the outbound intent
    SolverAcceptOutbound {
        intent_id: String,
        solver_id: String,
        solver_dest_address: String,
        /// Optional bond for extra security
        bond_amount: Option<Uint128>,
    },

    /// Submit proof that solver filled on destination
    NotifyOutboundFilled {
        intent_id: String,
        proof_type: ProofType,
        proof_data: Binary,
        tx_hash: String,
        block_number: u64,
    },

    /// Release escrowed funds to solver after proof verification
    ReleaseOutboundEscrow {
        intent_id: String,
    },

    /// Handle timeout - refund user if solver didn't fill
    RefundOutboundEscrow {
        intent_id: String,
    },

    /// Challenge an optimistic proof (if using optimistic proving)
    ChallengeOutboundProof {
        intent_id: String,
        challenge_data: Binary,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    // ... existing queries ...

    #[returns(OutboundEscrowStatusResponse)]
    OutboundEscrowStatus { intent_id: String },

    #[returns(OutboundEscrowsResponse)]
    PendingOutboundEscrows {
        destination_chain: Option<String>,
        start_after: Option<String>,
        limit: Option<u32>,
    },
}
```

## Proof Mechanisms

### Option 1: Hyperlane (Recommended for MVP)

Hyperlane provides cross-chain messaging with validator signatures.

```
Ethereum                              Cosmos Hub
    │                                     │
    │  Solver sends USDC to user          │
    │  ────────────────────────►          │
    │                                     │
    │  Hyperlane validators attest        │
    │  ────────────────────────►          │
    │                                     │
    │                          Verify Hyperlane message
    │                          Release escrow to solver
    │                                     │
```

**Pros:**
- Already integrated with Skip Go Fast
- Fast finality (~minutes)
- Battle-tested

**Cons:**
- Requires Hyperlane deployment on both chains
- Trust in Hyperlane validator set

### Option 2: IBC Eureka Reverse

If Eureka supports bidirectional proofs:

```
Ethereum                              Cosmos Hub
    │                                     │
    │  Solver sends USDC to user          │
    │  ────────────────────────►          │
    │                                     │
    │  ZK proof of ETH state              │
    │  ────────────────────────►          │
    │                                     │
    │                          Verify ZK proof
    │                          Release escrow to solver
    │                                     │
```

**Pros:**
- Trustless (ZK proofs)
- Consistent with inbound flow

**Cons:**
- May not be available yet
- Higher latency for proof generation

### Option 3: Optimistic with Challenge

For lower latency with security backstop:

```
Ethereum                              Cosmos Hub
    │                                     │
    │  Solver sends USDC to user          │
    │  ────────────────────────►          │
    │                                     │
    │  Solver submits proof               │
    │  ────────────────────────►          │
    │                                     │
    │                          Start challenge period (1hr)
    │                                     │
    │  (no challenge)                     │
    │                          Release escrow to solver
    │                                     │
```

**Pros:**
- Fast for honest solvers
- Can use any proof type

**Cons:**
- Challenge period delays settlement
- Need fraud proof system

## Risk Comparison

| Factor | Inbound (ETH→Cosmos) | Outbound (Cosmos→ETH) |
|--------|---------------------|----------------------|
| **Escrow location** | Ethereum | Cosmos Hub |
| **Fronting location** | Cosmos Hub | Ethereum |
| **Finality wait** | ~20 min (ETH) | ~6 sec (Cosmos) |
| **Solver risk exposure** | High (20 min) | Low (6 sec + proof time) |
| **Bond requirement** | Higher (1.5x) | Lower (1.1-1.2x) |
| **Proof mechanism** | Eureka ZK | Hyperlane/Optimistic |
| **Reorg risk** | High (ETH reorgs) | Low (Cosmos fast finality) |

## Settlement Risk Pricing (Outbound)

```rust
impl SettlementRiskPricing {
    /// Outbound has lower risk due to Cosmos fast finality
    pub fn default_outbound_ethereum() -> Self {
        Self {
            failure_probability: "0.0001".to_string(),
            expected_finality_secs: 300,  // 5 min (proof generation + relay)
            bond_multiplier: "1.2".to_string(),  // Lower than inbound
        }
    }

    pub fn default_outbound_base() -> Self {
        Self {
            failure_probability: "0.0002".to_string(),
            expected_finality_secs: 300,
            bond_multiplier: "1.2".to_string(),
        }
    }
}
```

## Flow Examples

### Example 1: Sell ATOM for USDC on Ethereum

```
1. User: "I want to sell 100 ATOM for USDC on Ethereum"

2. User calls RegisterOutboundEscrowIntent
   - Locks 100 ATOM in escrow
   - destination_chain: "ethereum"
   - destination_address: "0xuser..."
   - expected_output: 800 USDC (at current rate)
   - timeout: 1 hour

3. Solver sees pending outbound escrow
   - Evaluates: 100 ATOM worth ~$800, user wants 800 USDC
   - Spread: ~0% (solver makes money on rebalancing)

4. Solver calls SolverAcceptOutbound
   - Commits to fill within timeout
   - Posts optional 20 ATOM bond

5. Solver sends 800 USDC to 0xuser on Ethereum

6. Solver calls NotifyOutboundFilled
   - Submits Hyperlane proof of transfer

7. Contract verifies proof

8. Solver calls ReleaseOutboundEscrow
   - Receives 100 ATOM + bond back
```

### Example 2: Timeout / No Fill

```
1. User locks 100 ATOM for outbound

2. No solver accepts (bad rate, no liquidity)

3. Timeout expires (1 hour)

4. Anyone calls RefundOutboundEscrow
   - User receives 100 ATOM back
```

### Example 3: Solver Accepts but Doesn't Fill

```
1. User locks 100 ATOM

2. Solver accepts with 20 ATOM bond

3. Solver fails to fill on Ethereum

4. Timeout expires

5. User calls RefundOutboundEscrow
   - User receives 100 ATOM back
   - Solver loses 20 ATOM bond (slashed)
```

## Implementation Plan

### Phase 1: Core Outbound Escrow
- [ ] Add `OutboundEscrow` state storage
- [ ] Implement `RegisterOutboundEscrowIntent`
- [ ] Implement `SolverAcceptOutbound`
- [ ] Implement `RefundOutboundEscrow`
- [ ] Add query functions
- [ ] Unit tests

### Phase 2: Proof Verification
- [ ] Implement `NotifyOutboundFilled` with TrustedRelayer proof (testnet)
- [ ] Add Hyperlane proof verification
- [ ] Implement `ReleaseOutboundEscrow`
- [ ] Integration tests with mock proofs

### Phase 3: Security Hardening
- [ ] Add optional solver bonding
- [ ] Implement `ChallengeOutboundProof` (if using optimistic)
- [ ] Adversarial tests
- [ ] Audit

### Phase 4: Production
- [ ] Deploy to testnet
- [ ] Connect to Hyperlane testnet
- [ ] End-to-end testing
- [ ] Mainnet deployment

## Integration with Skip Go Fast

The outbound flow aligns with Skip Go Fast's `fillOrder` pattern:

| Skip Go Fast | Our Outbound Escrow |
|--------------|---------------------|
| `submitOrder` on source | `RegisterOutboundEscrowIntent` |
| Solver monitors events | Solver queries `PendingOutboundEscrows` |
| `fillOrder` on destination | Solver sends on Ethereum |
| `initiateSettlement` | `NotifyOutboundFilled` + `ReleaseOutboundEscrow` |

This enables Skip solvers to participate in our outbound flow with minimal changes.

## Open Questions

1. **Proof mechanism**: Should we start with Hyperlane or TrustedRelayer for testnet?

2. **Bond requirements**: Should outbound require solver bonds, or is the "accepted" commitment enough?

3. **Challenge period**: If using optimistic proofs, what's the right challenge period? (1 hour? 4 hours?)

4. **Multi-hop**: Should we support Cosmos → Base (via Ethereum) in a single intent?

5. **Partial fills**: Should outbound support partial fills like inbound?

## References

- [Skip Go Fast Documentation](https://docs.skip.build/go/advanced-transfer/go-fast)
- [Hyperlane Documentation](https://docs.hyperlane.xyz/)
- [IBC Eureka Specification](https://github.com/cosmos/ibc/tree/main/spec/eureka)
- [Existing Ethereum Escrow Implementation](../contracts/escrow/src/contract.rs)
