# Collateral Valuation Trait: Unified Multi-Asset Bond System

**Date:** 2026-01-05
**Status:** Draft
**Authors:** Zaki Manian, Philip Offtermatt (Hydro contribution)
**Depends On:** [Solver Bond Redesign](./2026-01-03-solver-bond-redesign.md)

## Summary

A unified trait-based system for accepting multiple collateral types as solver bonds: native tokens (ATOM), LSM shares, and Hydro Inflow vault shares. Includes valuation, haircuts, per-settlement locking, and liquidation via the intent system.

## Motivation

Solvers currently must post idle ATOM as bond collateral. This creates:

1. **Capital inefficiency** - Bonded ATOM earns no yield
2. **Barrier to entry** - Large bond requirements lock up significant capital
3. **Missed opportunities** - Stakers (LSM holders) and Hydro depositors can't participate as solvers

By accepting yield-bearing collateral:
- Solvers earn yield on bonded capital
- LSM holders can use tokenized delegations
- Hydro vault depositors can use their positions
- Capital efficiency improves without reducing security

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Valuation location | Hybrid (on-chain same-chain, cached cross-chain) | Matches reality of multi-chain deployment |
| Haircut model | Fixed per asset class | Simple, predictable, easy to audit |
| Staleness handling | Hard block | Never use bad data, ICQ refresh is fast |
| Multi-asset locking | Solver chooses per settlement | Flexibility, simpler accounting |
| Liquidation output | Always ATOM | Deep liquidity, simple routing |

## Architecture

### Core Types

```rust
/// Supported collateral asset types
#[cw_serde]
pub enum CollateralAsset {
    /// Native liquid token (ATOM, USDC)
    Native { denom: String },

    /// Hydro Inflow vault shares
    HydroVault {
        /// Address of the vault contract
        vault_contract: Addr,
        /// The share token denom (factory/neutron1.../inflow_atom)
        share_denom: String,
        /// Chain ID where vault lives (for ICQ routing)
        chain_id: String,
    },

    /// LSM tokenized delegation
    LsmShare {
        /// Validator operator address
        validator: String,
        /// Share token denom (cosmosvaloper1.../42)
        share_denom: String,
    },
}

/// Asset classification for haircut lookup
#[cw_serde]
pub enum AssetClass {
    Native,
    LsmShare,
    HydroVault,
}

/// Fixed haircuts per asset class (basis points)
pub const HAIRCUT_NATIVE_BPS: u64 = 0;       // 0%
pub const HAIRCUT_LSM_BPS: u64 = 1000;       // 10%
pub const HAIRCUT_HYDRO_BPS: u64 = 2000;     // 20%

/// Maximum staleness for cached cross-chain valuations
pub const MAX_STALENESS_SECONDS: u64 = 3600; // 1 hour
```

### CollateralValuation Trait

```rust
/// Trait for collateral valuation - implemented per asset type
pub trait CollateralValuation {
    /// Get raw value in base denomination (uatom)
    /// Returns error if data is stale or unavailable
    fn get_raw_value(
        &self,
        deps: Deps,
        env: &Env,
        amount: Uint128,
    ) -> Result<Uint128, ContractError>;

    /// Get haircut basis points for this asset class
    fn haircut_bps(&self) -> u64;

    /// Get effective collateral value (raw value minus haircut)
    fn get_collateral_value(
        &self,
        deps: Deps,
        env: &Env,
        amount: Uint128,
    ) -> Result<Uint128, ContractError> {
        let raw = self.get_raw_value(deps, env, amount)?;
        let haircut = raw
            .checked_mul(Uint128::from(self.haircut_bps()))?
            .checked_div(Uint128::from(10000u64))?;
        Ok(raw.checked_sub(haircut)?)
    }

    /// Check if this collateral can be liquidated (has liquidity path)
    fn is_liquidatable(&self, deps: Deps) -> bool;

    /// Get the asset class for grouping
    fn asset_class(&self) -> AssetClass;
}
```

### Native Token Implementation

```rust
impl CollateralValuation for NativeCollateral {
    fn get_raw_value(
        &self,
        _deps: Deps,
        _env: &Env,
        amount: Uint128,
    ) -> Result<Uint128, ContractError> {
        // Native ATOM is 1:1 with base denomination
        if self.denom == "uatom" {
            return Ok(amount);
        }
        // Other native tokens would need oracle - reject for now
        Err(ContractError::UnsupportedNativeDenom {
            denom: self.denom.clone()
        })
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_NATIVE_BPS // 0%
    }

    fn is_liquidatable(&self, _deps: Deps) -> bool {
        true // Native tokens always liquidatable
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Native
    }
}
```

### LSM Share Implementation

```rust
impl CollateralValuation for LsmShareCollateral {
    fn get_raw_value(
        &self,
        deps: Deps,
        _env: &Env,
        amount: Uint128,
    ) -> Result<Uint128, ContractError> {
        // Query validator's delegation info from staking module
        let validator_info: ValidatorResponse = deps.querier.query(
            &QueryRequest::Staking(StakingQuery::Validator {
                address: self.validator.clone(),
            })
        )?;

        let validator = validator_info.validator
            .ok_or(ContractError::ValidatorNotFound)?;

        // Exchange rate: tokens / shares
        // LSM share value = amount * (validator.tokens / validator.shares)
        let value = amount
            .checked_multiply_ratio(
                validator.tokens,
                validator.delegator_shares,
            )?;

        Ok(value)
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_LSM_BPS // 10%
    }

    fn is_liquidatable(&self, deps: Deps) -> bool {
        // Check validator is not jailed
        if let Ok(resp) = deps.querier.query::<ValidatorResponse>(
            &QueryRequest::Staking(StakingQuery::Validator {
                address: self.validator.clone(),
            })
        ) {
            return resp.validator.map(|v| !v.jailed).unwrap_or(false);
        }
        false
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::LsmShare
    }
}
```

### Hydro Vault Implementation

```rust
/// Cached pool info for cross-chain vaults
#[cw_serde]
pub struct CachedPoolInfo {
    pub total_shares_issued: Uint128,
    pub total_pool_value: Uint128,
    pub updated_at: u64,
}

/// Storage for cached cross-chain valuations
pub const HYDRO_POOL_CACHE: Map<&str, CachedPoolInfo> = Map::new("hydro_pool_cache");

impl CollateralValuation for HydroVaultCollateral {
    fn get_raw_value(
        &self,
        deps: Deps,
        env: &Env,
        amount: Uint128,
    ) -> Result<Uint128, ContractError> {
        let pool_info = self.get_pool_info(deps, env)?;

        // share_value = amount * (total_pool_value / total_shares_issued)
        if pool_info.total_shares_issued.is_zero() {
            return Ok(amount); // 1:1 if no shares yet
        }

        let value = amount.checked_multiply_ratio(
            pool_info.total_pool_value,
            pool_info.total_shares_issued,
        )?;

        Ok(value)
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_HYDRO_BPS // 20%
    }

    fn is_liquidatable(&self, deps: Deps) -> bool {
        let env = Env::default();
        self.get_pool_info(deps, &env).is_ok()
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::HydroVault
    }
}

impl HydroVaultCollateral {
    /// Get pool info - direct query for same-chain, cached for cross-chain
    fn get_pool_info(
        &self,
        deps: Deps,
        env: &Env,
    ) -> Result<CachedPoolInfo, ContractError> {
        let current_chain = env.block.chain_id.clone();

        if self.chain_id == current_chain {
            // Same-chain: direct query to vault contract
            self.query_pool_info_direct(deps)
        } else {
            // Cross-chain: use cached ICQ result
            self.get_cached_pool_info(deps, env)
        }
    }

    /// Direct query for same-chain vaults
    fn query_pool_info_direct(
        &self,
        deps: Deps,
    ) -> Result<CachedPoolInfo, ContractError> {
        let response: ControlCenterPoolInfoResponse = deps.querier
            .query_wasm_smart(
                self.vault_contract.to_string(),
                &HydroVaultQueryMsg::ControlCenterPoolInfo {},
            )?;

        Ok(CachedPoolInfo {
            total_shares_issued: response.total_shares_issued,
            total_pool_value: response.total_pool_value,
            updated_at: 0, // Not cached, always fresh
        })
    }

    /// Get cached result for cross-chain vaults
    fn get_cached_pool_info(
        &self,
        deps: Deps,
        env: &Env,
    ) -> Result<CachedPoolInfo, ContractError> {
        let cached = HYDRO_POOL_CACHE
            .load(deps.storage, &self.share_denom)
            .map_err(|_| ContractError::NoCachedPoolInfo {
                share_denom: self.share_denom.clone(),
            })?;

        // Check staleness - hard block if stale
        let age = env.block.time.seconds().saturating_sub(cached.updated_at);
        if age > MAX_STALENESS_SECONDS {
            return Err(ContractError::StalePoolInfo {
                share_denom: self.share_denom.clone(),
                age_seconds: age,
            });
        }

        Ok(cached)
    }
}
```

## Bond Pool Integration

### Data Structures

```rust
/// A solver's collateral deposit
#[cw_serde]
pub struct CollateralDeposit {
    pub asset: CollateralAsset,
    pub amount: Uint128,
    /// Amount currently locked in active settlements
    pub locked_amount: Uint128,
}

/// Solver's complete bond pool
#[cw_serde]
pub struct SolverBondPool {
    pub solver_id: String,
    pub deposits: Vec<CollateralDeposit>,
}

/// Record of what's locked for a specific settlement
#[cw_serde]
pub struct LockedCollateral {
    pub asset: CollateralAsset,
    pub amount: Uint128,
    pub value: Uint128, // Value at time of locking
}

/// Storage
pub const SOLVER_BOND_POOLS: Map<&str, SolverBondPool> = Map::new("solver_bond_pools");
```

### Bond Pool Methods

```rust
impl SolverBondPool {
    /// Get total collateral value across all deposits
    pub fn total_collateral_value(
        &self,
        deps: Deps,
        env: &Env,
    ) -> Result<Uint128, ContractError> {
        let mut total = Uint128::zero();

        for deposit in &self.deposits {
            let value = get_collateral_value(deps, env, &deposit.asset, deposit.amount)?;
            total = total.checked_add(value)?;
        }

        Ok(total)
    }

    /// Get available (unlocked) collateral value
    pub fn available_collateral_value(
        &self,
        deps: Deps,
        env: &Env,
    ) -> Result<Uint128, ContractError> {
        let mut total = Uint128::zero();

        for deposit in &self.deposits {
            let available = deposit.amount.checked_sub(deposit.locked_amount)?;
            let value = get_collateral_value(deps, env, &deposit.asset, available)?;
            total = total.checked_add(value)?;
        }

        Ok(total)
    }

    /// Lock collateral for a settlement (solver specifies which asset)
    pub fn lock_for_settlement(
        &mut self,
        deps: Deps,
        env: &Env,
        asset_class: AssetClass,
        required_value: Uint128, // 1.5x fill value
    ) -> Result<LockedCollateral, ContractError> {
        // Find deposit matching requested asset class
        let deposit = self.deposits
            .iter_mut()
            .find(|d| get_asset_class(&d.asset) == asset_class)
            .ok_or(ContractError::NoCollateralOfType {
                asset_class: asset_class.clone()
            })?;

        // Calculate how much raw amount needed for required value
        let raw_amount = reverse_haircut_calculation(
            deps, env, &deposit.asset, required_value
        )?;

        let available = deposit.amount.checked_sub(deposit.locked_amount)?;
        if available < raw_amount {
            return Err(ContractError::InsufficientCollateral {
                required: raw_amount,
                available,
            });
        }

        // Lock it
        deposit.locked_amount = deposit.locked_amount.checked_add(raw_amount)?;

        Ok(LockedCollateral {
            asset: deposit.asset.clone(),
            amount: raw_amount,
            value: required_value,
        })
    }

    /// Unlock collateral after settlement completes
    pub fn unlock(
        &mut self,
        asset: &CollateralAsset,
        amount: Uint128
    ) -> Result<(), ContractError> {
        let deposit = self.deposits
            .iter_mut()
            .find(|d| &d.asset == asset)
            .ok_or(ContractError::DepositNotFound)?;

        deposit.locked_amount = deposit.locked_amount.checked_sub(amount)?;
        Ok(())
    }
}
```

## Liquidation Flow

### Liquidation Intent

```rust
/// Liquidation intent created when slashing non-native collateral
#[cw_serde]
pub struct LiquidationIntent {
    /// Reference to the failed settlement
    pub source_settlement_id: String,

    /// The slashed solver (excluded from bidding)
    pub slashed_solver: String,

    /// Seized collateral to liquidate
    pub collateral: LockedCollateral,

    /// Minimum ATOM output (covers user compensation)
    pub min_output_amount: Uint128,

    /// User to receive liquidation proceeds
    pub beneficiary: Addr,

    /// Deadline for liquidation
    pub timeout: Timestamp,

    /// Current status
    pub status: LiquidationStatus,
}

#[cw_serde]
pub enum LiquidationStatus {
    Pending,
    Auctioning,
    Settled { output_amount: Uint128 },
    Failed { reason: String },
}

pub const LIQUIDATION_INTENTS: Map<&str, LiquidationIntent> = Map::new("liquidation_intents");
```

### Slashing Handler

```rust
/// Execute slashing with automatic liquidation routing
pub fn execute_slash_solver(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    settlement_id: String,
    solver_id: String,
    slash_amount: Uint128, // Amount owed to user in ATOM
) -> Result<Response, ContractError> {
    // Only admin can slash
    assert_admin(deps.as_ref(), &info.sender)?;

    let settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;
    let mut bond_pool = SOLVER_BOND_POOLS.load(deps.storage, &solver_id)?;

    // Get the locked collateral for this settlement
    let locked = settlement.locked_collateral
        .ok_or(ContractError::NoLockedCollateral)?;

    match locked.asset.asset_class() {
        AssetClass::Native => {
            // Native ATOM: direct transfer to user
            slash_native_collateral(
                deps,
                &mut bond_pool,
                &locked,
                slash_amount,
                &settlement.user,
            )
        },
        AssetClass::LsmShare | AssetClass::HydroVault => {
            // Non-native: create liquidation intent
            create_liquidation_intent(
                deps,
                env,
                &settlement_id,
                &solver_id,
                locked,
                slash_amount,
                &settlement.user,
            )
        },
    }
}

/// Slash native collateral directly
fn slash_native_collateral(
    deps: DepsMut,
    bond_pool: &mut SolverBondPool,
    locked: &LockedCollateral,
    slash_amount: Uint128,
    beneficiary: &Addr,
) -> Result<Response, ContractError> {
    // Deduct from solver's bond pool
    bond_pool.unlock(&locked.asset, locked.amount)?;

    // Remove slashed amount from deposits
    let deposit = bond_pool.deposits
        .iter_mut()
        .find(|d| &d.asset == &locked.asset)
        .ok_or(ContractError::DepositNotFound)?;
    deposit.amount = deposit.amount.checked_sub(slash_amount)?;

    SOLVER_BOND_POOLS.save(deps.storage, &bond_pool.solver_id, bond_pool)?;

    // Transfer to user
    let send_msg = BankMsg::Send {
        to_address: beneficiary.to_string(),
        amount: vec![coin(slash_amount.u128(), "uatom")],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "slash_native")
        .add_attribute("amount", slash_amount))
}

/// Create liquidation intent for non-native collateral
fn create_liquidation_intent(
    deps: DepsMut,
    env: Env,
    settlement_id: &str,
    solver_id: &str,
    locked: LockedCollateral,
    min_output: Uint128,
    beneficiary: &Addr,
) -> Result<Response, ContractError> {
    let intent_id = format!("liq_{}", settlement_id);

    let intent = LiquidationIntent {
        source_settlement_id: settlement_id.to_string(),
        slashed_solver: solver_id.to_string(),
        collateral: locked,
        min_output_amount: min_output,
        beneficiary: beneficiary.clone(),
        timeout: env.block.time.plus_seconds(3600), // 1 hour
        status: LiquidationStatus::Pending,
    };

    LIQUIDATION_INTENTS.save(deps.storage, &intent_id, &intent)?;

    Ok(Response::new()
        .add_attribute("action", "create_liquidation_intent")
        .add_attribute("intent_id", intent_id)
        .add_attribute("collateral_type", format!("{:?}", intent.collateral.asset))
        .add_attribute("min_output", min_output))
}
```

### Liquidation Auction

```rust
/// Solver submits a bid to execute a liquidation intent
pub fn execute_bid_on_liquidation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intent_id: String,
    offered_output: Uint128, // ATOM amount solver will deliver
) -> Result<Response, ContractError> {
    let mut intent = LIQUIDATION_INTENTS.load(deps.storage, &intent_id)?;

    // Validate
    if intent.status != LiquidationStatus::Pending
        && intent.status != LiquidationStatus::Auctioning {
        return Err(ContractError::LiquidationNotOpen);
    }
    if info.sender.to_string() == intent.slashed_solver {
        return Err(ContractError::SlashedSolverCannotBid);
    }
    if offered_output < intent.min_output_amount {
        return Err(ContractError::BidBelowMinimum {
            offered: offered_output,
            minimum: intent.min_output_amount,
        });
    }
    if env.block.time > intent.timeout {
        return Err(ContractError::LiquidationExpired);
    }

    // Update status and store bid
    intent.status = LiquidationStatus::Auctioning;
    LIQUIDATION_INTENTS.save(deps.storage, &intent_id, &intent)?;

    LIQUIDATION_BIDS.save(
        deps.storage,
        (&intent_id, info.sender.as_str()),
        &LiquidationBid {
            solver: info.sender.clone(),
            offered_output,
            submitted_at: env.block.time,
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "bid_on_liquidation")
        .add_attribute("intent_id", intent_id)
        .add_attribute("solver", info.sender)
        .add_attribute("offered_output", offered_output))
}

/// Finalize auction and execute winning bid
pub fn execute_finalize_liquidation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intent_id: String,
) -> Result<Response, ContractError> {
    let mut intent = LIQUIDATION_INTENTS.load(deps.storage, &intent_id)?;

    // Find highest bid
    let winning_bid = find_highest_bid(deps.as_ref(), &intent_id)?;

    // Transfer collateral to winning solver
    let collateral_transfer = transfer_collateral(
        &intent.collateral,
        &winning_bid.solver,
    )?;

    // Winning solver must have attached ATOM payment
    let payment = info.funds
        .iter()
        .find(|c| c.denom == "uatom")
        .ok_or(ContractError::NoPaymentAttached)?;

    if payment.amount < winning_bid.offered_output {
        return Err(ContractError::InsufficientPayment);
    }

    // Send ATOM to beneficiary (user)
    let user_payment = BankMsg::Send {
        to_address: intent.beneficiary.to_string(),
        amount: vec![coin(winning_bid.offered_output.u128(), "uatom")],
    };

    // Return excess to solver if overpaid
    let mut msgs: Vec<CosmosMsg> = vec![collateral_transfer, user_payment.into()];

    let excess = payment.amount.checked_sub(winning_bid.offered_output)?;
    if !excess.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: winning_bid.solver.to_string(),
            amount: vec![coin(excess.u128(), "uatom")],
        }.into());
    }

    // Update intent status
    intent.status = LiquidationStatus::Settled {
        output_amount: winning_bid.offered_output,
    };
    LIQUIDATION_INTENTS.save(deps.storage, &intent_id, &intent)?;

    // Clean up solver's bond pool
    let mut bond_pool = SOLVER_BOND_POOLS.load(
        deps.storage,
        &intent.slashed_solver
    )?;
    remove_slashed_collateral(&mut bond_pool, &intent.collateral)?;
    SOLVER_BOND_POOLS.save(deps.storage, &intent.slashed_solver, &bond_pool)?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("action", "finalize_liquidation")
        .add_attribute("intent_id", intent_id)
        .add_attribute("winner", winning_bid.solver)
        .add_attribute("output_amount", winning_bid.offered_output))
}

/// Helper to build collateral transfer message
fn transfer_collateral(
    collateral: &LockedCollateral,
    recipient: &Addr,
) -> Result<CosmosMsg, ContractError> {
    let denom = match &collateral.asset {
        CollateralAsset::Native { denom } => denom.clone(),
        CollateralAsset::HydroVault { share_denom, .. } => share_denom.clone(),
        CollateralAsset::LsmShare { share_denom, .. } => share_denom.clone(),
    };

    Ok(BankMsg::Send {
        to_address: recipient.to_string(),
        amount: vec![coin(collateral.amount.u128(), denom)],
    }.into())
}
```

## ICQ Integration for Cross-Chain Vaults

```rust
/// Request pool info refresh via ICQ
pub fn execute_request_pool_info_refresh(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    share_denom: String,
) -> Result<Response, ContractError> {
    let vault_config = ACCEPTED_HYDRO_VAULTS.load(deps.storage, &share_denom)?;

    // Build ICQ request for ControlCenterPoolInfo
    let query_data = to_json_binary(&HydroVaultQueryMsg::ControlCenterPoolInfo {})?;

    let icq_msg = NeutronMsg::register_interchain_query(
        QueryType::KV,
        vault_config.ibc_connection_id,
        vec![KVKey {
            path: format!("wasm/contract/{}", vault_config.vault_contract),
            key: query_data,
        }],
        env.block.height + 100, // Update every 100 blocks
    );

    Ok(Response::new()
        .add_message(icq_msg)
        .add_attribute("action", "request_pool_info_refresh")
        .add_attribute("share_denom", share_denom))
}

/// Handle ICQ callback with pool info
pub fn sudo_icq_result(
    deps: DepsMut,
    env: Env,
    query_id: u64,
    result: InterchainQueryResult,
) -> Result<Response, ContractError> {
    let share_denom = ICQ_QUERY_MAP.load(deps.storage, query_id)?;

    let pool_info: ControlCenterPoolInfoResponse = from_json(&result.kv_results[0].value)?;

    HYDRO_POOL_CACHE.save(
        deps.storage,
        &share_denom,
        &CachedPoolInfo {
            total_shares_issued: pool_info.total_shares_issued,
            total_pool_value: pool_info.total_pool_value,
            updated_at: env.block.time.seconds(),
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "icq_pool_info_updated")
        .add_attribute("share_denom", share_denom))
}
```

## Message Definitions

```rust
#[cw_serde]
pub enum ExecuteMsg {
    // === Collateral Management ===

    /// Deposit collateral to solver bond pool
    DepositCollateral {},

    /// Withdraw available (unlocked) collateral
    WithdrawCollateral {
        asset: CollateralAsset,
        amount: Uint128,
    },

    // === ICQ Management ===

    /// Request refresh of cross-chain vault pool info
    RequestPoolInfoRefresh { share_denom: String },

    // === Liquidation ===

    /// Bid on a liquidation intent
    BidOnLiquidation {
        intent_id: String,
        offered_output: Uint128,
    },

    /// Finalize liquidation auction (winner calls with payment)
    FinalizeLiquidation { intent_id: String },

    // === Admin ===

    /// Slash a solver (admin only)
    SlashSolver {
        settlement_id: String,
        solver_id: String,
        slash_amount: Uint128,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Get solver's bond pool details
    #[returns(SolverBondPoolResponse)]
    SolverBondPool { solver_id: String },

    /// Get total and available collateral value for a solver
    #[returns(CollateralValueResponse)]
    SolverCollateralValue { solver_id: String },

    /// Get liquidation intent details
    #[returns(LiquidationIntent)]
    LiquidationIntent { intent_id: String },

    /// List active liquidation intents
    #[returns(LiquidationIntentsResponse)]
    ActiveLiquidations {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    /// Get cached pool info for a Hydro vault
    #[returns(CachedPoolInfo)]
    HydroPoolInfo { share_denom: String },
}
```

## Security Considerations

1. **Staleness attacks** - Attacker waits for cache to become stale, then exploits outdated valuation. Mitigated by hard block on stale data.

2. **Validator slashing** - LSM shares can lose value if validator is slashed between valuation and settlement. Mitigated by 10% haircut buffer.

3. **Hydro vault withdrawal queue** - Vault shares may not be instantly liquid. Mitigated by 20% haircut and liquidation via intent auction (solvers price in illiquidity).

4. **ICQ manipulation** - Malicious relayer could submit false ICQ results. Mitigated by using Neutron's native ICQ with cryptographic proofs.

5. **Liquidation auction griefing** - Bidder wins then doesn't pay. Mitigated by requiring payment attached to finalization tx.

## Migration Path

1. **Phase 1**: Deploy trait + Native implementation only
2. **Phase 2**: Add LSM share support (Hub-native, no ICQ needed)
3. **Phase 3**: Add Hydro vault support with ICQ infrastructure
4. **Phase 4**: Enable liquidation intents for non-native collateral

## References

- [Solver Bond Redesign](./2026-01-03-solver-bond-redesign.md)
- [Hydro Inflow Vault Shares as Collateral (PR #14)](https://github.com/iqlusioninc/atom-intents/pull/14)
- [Hydro ControlCenterPoolInfo Query (PR #363)](https://github.com/informalsystems/hydro/pull/363)
- [Cosmos SDK LSM ADR-061](https://docs.cosmos.network/v0.47/architecture/adr-061-liquid-staking)
- [Neutron ICQ Documentation](https://docs.neutron.org/neutron/modules/interchain-queries/overview)
