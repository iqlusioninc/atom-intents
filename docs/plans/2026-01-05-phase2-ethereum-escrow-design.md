# Phase 2: Ethereum Escrow Flow Design

> **Status:** Design document for future implementation
> **Depends on:** Phase 1 Eureka Integration (complete)

## Overview

Phase 2 enables Ethereum users to participate in atom-intents auctions by escrowing funds via IBC Eureka. Solvers front Cosmos-side funds immediately and take on settlement risk.

## The Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Ethereum → Cosmos Escrow Flow                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ETHEREUM SIDE                        COSMOS HUB                            │
│  ─────────────                        ──────────                            │
│                                                                             │
│  1. User signs intent                 2. Intent submitted to Hub            │
│     (wants ATOM, has USDC)               (input: ETH USDC via Eureka)       │
│                                                                             │
│  3. User sends Eureka packet ───────────────────────────────────────────►   │
│     to escrow contract                4. Hub receives packet notification   │
│     (with intent_id in memo)             (escrow pending Eureka finality)   │
│                                                                             │
│                                       5. Solvers see escrowed intent        │
│                                          - Input: pending Eureka packet     │
│                                          - Risk: Eureka settlement          │
│                                                                             │
│                                       6. Solver bids, wins auction          │
│                                          - Fronts ATOM immediately          │
│                                          - Posts higher bond (settlement    │
│                                            risk premium)                    │
│                                                                             │
│                                       7. User receives ATOM instantly       │
│                                                                             │
│  8. Eureka ZK proof finalized ──────────────────────────────────────────►   │
│     (~15-30 seconds)                  9. Escrow releases to solver          │
│                                          - Solver receives bridged USDC     │
│                                          - Bond returned                    │
│                                                                             │
│  FAILURE CASE:                                                              │
│  ─────────────                                                              │
│  If Eureka packet fails/times out:    - Solver bond covers user's ATOM      │
│                                       - Solver takes loss                   │
│                                       - User keeps ATOM + ETH USDC returned │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Intent Origin

**Question:** Where is the intent created and signed?

**Answer:** Cosmos Hub. The intent references an Ethereum address and pending Eureka packet.

```rust
pub struct EthereumEscrowedIntent {
    /// Standard intent fields
    pub base: Intent,

    /// Ethereum address that will send the Eureka packet
    pub eth_sender: String,

    /// Expected Eureka packet hash (optional, for pre-commitment)
    pub expected_packet_hash: Option<String>,

    /// Eureka escrow status
    pub escrow_status: EscrowStatus,
}

pub enum EscrowStatus {
    /// Waiting for Eureka packet to arrive
    Pending,

    /// Eureka packet received, awaiting ZK proof finality
    Received { packet_id: String, received_at: u64 },

    /// ZK proof verified, funds fully escrowed
    Finalized { packet_id: String, finalized_at: u64 },

    /// Packet failed or timed out
    Failed { reason: String },
}
```

### 2. Escrow Mechanism

**On Ethereum:**
- User sends IBC Eureka transfer with special memo
- Memo contains: `{"atom_intents": {"intent_id": "...", "hub_address": "cosmos1..."}}`
- Funds are locked in Eureka bridge contract

**On Cosmos Hub:**
- Escrow contract receives packet notification
- Intent becomes "active" once packet is seen
- Full settlement occurs after ZK proof verification

### 3. Solver Risk Model

Solvers take on **settlement risk** - the risk that the Eureka packet fails after they've already fronted funds.

```rust
pub struct SettlementRiskPricing {
    /// Base probability of Eureka failure (historical data)
    pub failure_probability: Decimal,

    /// Time until ZK proof expected (affects risk window)
    pub expected_finality_secs: u64,

    /// Required bond multiplier (e.g., 2.0x = 200% bond)
    pub bond_multiplier: Decimal,

    /// Solver's risk premium in basis points
    pub risk_premium_bps: u32,
}

impl SettlementRiskPricing {
    /// Calculate bond requirement for fronting
    pub fn required_bond(&self, fronted_amount: Uint128) -> Uint128 {
        // Bond must cover: fronted amount + potential slippage + penalty
        fronted_amount * self.bond_multiplier
    }

    /// Calculate risk-adjusted quote
    pub fn adjust_quote(&self, base_output: Uint128) -> Uint128 {
        // Deduct risk premium from output
        let premium = base_output * self.risk_premium_bps / 10000;
        base_output - premium
    }
}
```

### 4. Packet Monitoring

The system needs to monitor incoming Eureka packets:

```rust
#[async_trait]
pub trait EurekaPacketMonitor {
    /// Watch for Eureka packets matching intent criteria
    async fn watch_for_packet(
        &self,
        intent_id: &str,
        eth_sender: &str,
        expected_amount: Uint128,
        timeout: Duration,
    ) -> Result<EurekaPacketStatus, MonitorError>;

    /// Get current status of a known packet
    async fn get_packet_status(&self, packet_id: &str) -> Result<EurekaPacketStatus, MonitorError>;

    /// Verify ZK proof for finality
    async fn verify_finality(&self, packet_id: &str) -> Result<bool, MonitorError>;
}

pub enum EurekaPacketStatus {
    /// Not yet seen
    NotFound,

    /// Packet received, ZK proof pending
    Pending {
        packet_id: String,
        amount: Uint128,
        sender: String,
        received_at: u64,
    },

    /// ZK proof verified, fully finalized
    Finalized {
        packet_id: String,
        amount: Uint128,
        proof_block: u64,
    },

    /// Failed or timed out
    Failed { reason: String },
}
```

### 5. Settlement Contract Changes

The escrow contract needs new capabilities:

```rust
pub enum ExecuteMsg {
    // ... existing messages ...

    /// Register an intent with pending Eureka escrow
    RegisterEthereumEscrowIntent {
        intent: Intent,
        eth_sender: String,
        expected_amount: Uint128,
        eureka_timeout_secs: u64,
    },

    /// Called by relayer when Eureka packet arrives
    NotifyEurekaPacketReceived {
        intent_id: String,
        packet_id: String,
        amount: Uint128,
        sender: String,
    },

    /// Called by relayer when ZK proof is verified
    NotifyEurekaFinalized {
        intent_id: String,
        packet_id: String,
        proof_block: u64,
    },

    /// Solver fronts funds before Eureka finality
    FrontSettlement {
        intent_id: String,
        solution: Solution,
        /// Additional bond for settlement risk
        risk_bond: Uint128,
    },

    /// Release escrowed funds to solver after finality
    ClaimEurekaEscrow {
        intent_id: String,
        packet_id: String,
    },
}
```

### 6. User Experience

From the user's perspective:

1. **Connect Ethereum wallet** to atom-intents frontend
2. **Create intent** specifying desired Cosmos asset
3. **Sign Eureka transfer** from Ethereum
4. **Wait ~15-30 seconds** for solver to fill
5. **Receive Cosmos asset** (may be faster than Eureka finality if solver fronts)

The user doesn't need to understand the settlement mechanics - they just see fast execution.

## Implementation Tasks (Future)

### Task 1: Ethereum Escrow Intent Type
- Add `EthereumEscrowedIntent` to types crate
- Add `EscrowStatus` enum
- Update intent validation

### Task 2: Eureka Packet Monitor
- Implement packet monitoring service
- Subscribe to Eureka IBC events
- Track packet → intent mapping

### Task 3: Settlement Risk Pricing
- Add risk calculation logic
- Bond multiplier configuration
- Risk premium in solver quotes

### Task 4: Escrow Contract Updates
- Add Ethereum escrow messages
- Implement fronting logic
- Add finality verification

### Task 5: Solver Updates
- Add settlement risk to EurekaSolver
- Implement fronting decision logic
- Update bond calculations

### Task 6: Frontend Integration
- Ethereum wallet connection
- Intent creation with Eureka escrow
- Transaction signing flow

### Task 7: Integration Tests
- Full flow tests
- Failure case handling
- Timeout scenarios

## Risk Considerations

### For Solvers

| Risk | Mitigation |
|------|------------|
| Eureka packet fails after fronting | Bond covers loss, priced into risk premium |
| ZK proof takes longer than expected | Timeout-based bond release |
| Ethereum reorg invalidates packet | Monitor for reorgs, adjust finality threshold |
| User front-runs with different packet | Match intent to specific packet hash |

### For Users

| Risk | Mitigation |
|------|------------|
| No solver willing to front | Fall back to waiting for Eureka finality |
| Solver defaults | Bond covers user's expected output |
| Eureka bridge failure | Funds returned on Ethereum side |

## Comparison: Phase 1 vs Phase 2

| Aspect | Phase 1 (Complete) | Phase 2 (Future) |
|--------|-------------------|------------------|
| Direction | Cosmos user → ETH liquidity | ETH user → Cosmos liquidity |
| User escrow | On Cosmos Hub | On Ethereum via Eureka |
| Solver risk | Minimal (normal execution) | Settlement risk (fronting) |
| Complexity | Medium | High |
| Use case | Cosmos users want better prices | ETH users want Cosmos assets |

## Open Questions

1. **Bond custody:** Should risk bonds be held in the main escrow or separate contract?

2. **Partial fronting:** Can solvers front partial amounts for large intents?

3. **MEV considerations:** How to prevent MEV extraction on Eureka packets?

4. **Multi-solver fronting:** Can multiple solvers share settlement risk?

5. **Insurance pool:** Should there be a shared insurance pool for settlement failures?

## Next Steps

1. Review this design with stakeholders
2. Merge Phase 1 Eureka integration
3. Prototype packet monitoring
4. Implement Phase 2 in dedicated branch
