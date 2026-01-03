# Solver Bond Redesign: Per-Settlement Locking with LSM Support

**Date:** 2026-01-03
**Status:** Draft
**Authors:** Zaki Manian

## Summary

Replace the fixed registration bond with a per-settlement locking mechanism (1.5x fill value) and add support for LSM shares as bond collateral with liquidation via the intent system.

## Motivation

The current implementation uses a fixed registration bond (e.g., 100 ATOM) that covers all settlements. This creates three problems:

1. **Solver insolvency risk** - A solver with 100 ATOM bond could be solving $1M+ in volume. If they abandon multiple settlements simultaneously, the bond is insufficient to compensate users.

2. **Griefing attacks** - A malicious solver could win many auctions, abandon them all, lose their small bond, but cause significant disruption and user opportunity cost.

3. **Incentive misalignment** - Bond doesn't scale with settlement size, so solvers have disproportionately low skin in the game for large settlements.

Additionally, requiring liquid tokens (ATOM, USDC) as bonds excludes stakers who hold LSM shares and would otherwise participate as solvers.

## Design

### Per-Settlement Bond Locking

When a solver commits to a settlement, 1.5x the fill value is locked from their bond pool:

```
Solver bond pool: 1000 ATOM

Settlement A (100 ATOM fill):
  - Locks: 150 ATOM
  - Available: 850 ATOM

Settlement B (200 ATOM fill):
  - Locks: 300 ATOM
  - Available: 550 ATOM

Settlement A completes:
  - Unlocks: 150 ATOM
  - Available: 700 ATOM
```

If a solver's available bond is insufficient, they cannot commit to additional settlements until existing ones complete.

### Bond Pool Structure

```rust
pub struct SolverBondPool {
    /// Total bond value deposited (in normalized units)
    pub total_value: Uint128,

    /// Amount currently locked in active settlements
    pub locked_value: Uint128,

    /// Individual bond deposits (supports multiple asset types)
    pub deposits: Vec<BondDeposit>,
}

pub struct BondDeposit {
    /// The bonded asset
    pub asset: BondAsset,

    /// Amount deposited
    pub amount: Uint128,

    /// Normalized value after haircut (for LSM shares)
    pub normalized_value: Uint128,

    /// Amount currently locked
    pub locked_amount: Uint128,
}

pub enum BondAsset {
    /// Native liquid token (ATOM, USDC, etc.)
    Native { denom: String },

    /// LSM share (liquid staking module tokenized delegation)
    LsmShare {
        validator: String,
        denom: String,
    },
}
```

### LSM Share Valuation

LSM shares are accepted with a fixed 10% haircut to account for liquidation friction:

```
100 stATOM LSM shares → 90 ATOM bond value
```

This haircut provides buffer for:
- Auction slippage during liquidation
- Time value of delayed compensation
- Market volatility during liquidation window

Combined with 1.5x overcollateralization, effective coverage is 1.35x (1.5 × 0.9) which remains conservative.

### Liquidation Flow

When a solver with LSM bonds is slashed:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. SLASH TRIGGERED                                          │
│    - Admin calls execute_slash_solver                       │
│    - Slash amount calculated (base_slash_bps of fill value) │
└─────────────────────────┬───────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. BOND ASSET CHECK                                         │
│    - If liquid token: transfer directly to compensation     │
│    - If LSM share: proceed to liquidation intent            │
└─────────────────────────┬───────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. LIQUIDATION INTENT CREATED                               │
│    - Input: seized LSM shares                               │
│    - Output: liquid ATOM (or user's original input token)   │
│    - Beneficiary: wronged user                              │
│    - Timeout: 1 hour (configurable)                         │
│    - Priority: high (system-generated)                      │
└─────────────────────────┬───────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. AUCTION RUNS                                             │
│    - Other solvers bid to buy LSM shares                    │
│    - Standard intent auction mechanics                      │
│    - Slashed solver excluded from bidding                   │
└─────────────────────────┬───────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. SETTLEMENT                                               │
│    - Winning solver receives LSM shares                     │
│    - User receives liquid tokens                            │
│    - Excess (if any) returned to slashed solver             │
└─────────────────────────────────────────────────────────────┘
```

### Liquidation Intent Structure

```rust
pub struct LiquidationIntent {
    /// Reference to original failed settlement
    pub source_settlement_id: String,

    /// The slashed solver (excluded from bidding)
    pub slashed_solver: Addr,

    /// LSM shares being liquidated
    pub lsm_shares: Coin,

    /// Minimum output (slash amount owed to user)
    pub min_output: Uint128,

    /// Output denomination (liquid token)
    pub output_denom: String,

    /// Beneficiary (wronged user)
    pub beneficiary: Addr,

    /// Liquidation timeout
    pub timeout: Timestamp,
}
```

### Timeout Handling

If liquidation intent times out without being filled:

1. LSM shares remain seized (not returned to slashed solver)
2. System retries with relaxed parameters:
   - Lower `min_output` (accept worse price)
   - Longer timeout
3. After N retries, fallback to:
   - Protocol treasury absorbs LSM shares, compensates user
   - Or: user receives LSM shares directly (their choice)

### State Changes

**Settlement contract additions:**

```rust
// In settlement state
pub struct Settlement {
    // ... existing fields ...

    /// Bond amount locked for this settlement
    pub locked_bond_amount: Uint128,

    /// Bond assets locked (for LSM tracking)
    pub locked_bond_assets: Vec<LockedBondAsset>,
}

pub struct LockedBondAsset {
    pub asset: BondAsset,
    pub amount: Uint128,
}
```

**New messages:**

```rust
pub enum ExecuteMsg {
    // ... existing messages ...

    /// Deposit bond (native or LSM)
    DepositBond { asset: BondAsset },

    /// Withdraw available (unlocked) bond
    WithdrawBond { asset: BondAsset, amount: Uint128 },

    /// Internal: lock bond for settlement
    LockBondForSettlement {
        settlement_id: String,
        amount: Uint128
    },

    /// Internal: unlock bond after settlement completes
    UnlockBond { settlement_id: String },

    /// Internal: create liquidation intent for LSM slashing
    CreateLiquidationIntent {
        settlement_id: String,
        beneficiary: Addr,
    },
}
```

### Configuration

```rust
pub struct BondConfig {
    /// Multiplier for bond locking (1.5 = 150% of fill value)
    pub lock_multiplier: Decimal,

    /// Haircut for LSM shares (0.1 = 10% discount)
    pub lsm_haircut: Decimal,

    /// Accepted LSM share denoms (whitelist)
    pub accepted_lsm_denoms: Vec<String>,

    /// Liquidation intent timeout
    pub liquidation_timeout_seconds: u64,

    /// Max liquidation retries before fallback
    pub max_liquidation_retries: u8,
}
```

## Migration Path

1. **Phase 1: Add per-settlement locking for native tokens**
   - Implement lock/unlock mechanics
   - Existing solvers' fixed bonds become their pool
   - No breaking changes to registration

2. **Phase 2: Add LSM share support**
   - Implement BondAsset enum and deposits
   - Add haircut calculation
   - Add LSM denom whitelist

3. **Phase 3: Add liquidation intents**
   - Implement LiquidationIntent type
   - Add auction exclusion for slashed solver
   - Add timeout/retry logic

## Security Considerations

1. **Reentrancy** - Bond locking must be atomic with settlement commitment. Use checks-effects-interactions pattern.

2. **Sandwich attacks on liquidation** - Solvers could manipulate LSM prices before bidding. Mitigated by:
   - Short auction windows
   - Minimum output based on pre-slash valuation
   - Haircut buffer

3. **LSM validator risk** - If underlying validator is slashed, LSM shares lose value. Mitigated by:
   - 10% haircut buffer
   - Whitelist only reputable validator LSM denoms
   - 1.5x overcollateralization

4. **Liquidation failure** - If no solver bids on liquidation intent, user waits indefinitely. Mitigated by:
   - Progressive price reduction on retries
   - Protocol treasury backstop as last resort
   - User can opt to receive LSM shares directly

## Open Questions

1. Should the 10% haircut be configurable per LSM denom based on validator reputation?

2. Should liquidation intents have priority queue access (skip normal auction)?

3. Should we implement partial bond withdrawal (only unlocked portion) or require full withdrawal + re-deposit?

## References

- Original specification: `docs/SPECIFICATION.md` (lines 1630-1650)
- Current bond implementation: `contracts/settlement/src/state.rs`
- Slashing logic: `contracts/settlement/src/handlers.rs:345-408`
