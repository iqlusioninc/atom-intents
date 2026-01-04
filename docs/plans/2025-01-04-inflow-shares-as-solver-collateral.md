# Inflow Vault Shares as Solver Bond Collateral

## Overview

This design document outlines how to integrate Hydro Inflow vault shares as permissible collateral for solver bonds in the atom-intents settlement system. This allows solvers to use yield-bearing vault positions instead of idle ATOM as their bond collateral.

## Motivation

Currently, solver bonds must be posted in `uatom`. This has drawbacks:
- **Capital inefficiency**: Bonded ATOM sits idle, earning no yield
- **Opportunity cost**: Solvers must choose between staking rewards and providing liquidity
- **Barrier to entry**: Large bond requirements lock up significant capital

By accepting Inflow vault shares (e.g., `iATOM`, `istATOM`) as collateral:
- Solvers earn yield on their bonded capital via Inflow's adapters (Mars, Skip, etc.)
- Capital efficiency improves without reducing security guarantees
- Aligns with Hydro's Protocol-Owned Liquidity vision

## Scope

This document covers:
1. Specifying which vault shares are permissible as collateral
2. Retrieving share values from Inflow vault contracts (same-chain and cross-chain)

Out of scope (future documents):
- Slashing mechanics for vault share collateral
- Liquidation procedures
- Integration with Inflow withdrawal queues

---

## 1. Permissible Vault Shares Registry

### Design

The Settlement Contract maintains a whitelist of accepted collateral types, including both native tokens and Inflow vault shares.

### Data Structures

```rust
/// Configuration for an accepted Inflow vault share as collateral
#[cw_serde]
pub struct AcceptedVaultShare {
    /// The vault share token denom (e.g., "factory/{vault_addr}/inflow_atom")
    pub share_denom: String,

    /// Address of the Inflow vault contract that issued these shares
    pub vault_contract: String,

    /// Chain ID where the vault contract is deployed
    /// If same as settlement contract chain, use direct queries
    /// If different, use ICQ (Interchain Queries)
    pub vault_chain_id: String,

    /// For cross-chain vaults: IBC connection ID to the vault's chain
    pub ibc_connection_id: Option<String>,

    /// The base token denom that this vault's shares are valued in.
    /// This is the denom ON THE SETTLEMENT CONTRACT'S CHAIN.
    /// For example, if the vault holds ATOM and is on Neutron, but settlement
    /// is on Cosmos Hub, this would be "uatom" (the Hub's native denom).
    /// For cross-chain vaults, this may be an IBC denom (e.g., "ibc/...")
    pub base_token_denom: String,

    /// Collateral ratio (e.g., 8000 means 80% of share value counts as collateral)
    /// Applied as haircut to account for withdrawal delays and price volatility
    pub collateral_ratio_bps: u64,

    /// Whether this collateral type is currently active for new bonds
    pub active: bool,

    /// Human-readable name for display
    pub name: String,
}

/// Extended solver registration to support multiple collateral types
#[cw_serde]
pub struct SolverCollateral {
    /// Native token bonds (existing behavior)
    pub native_bonds: Vec<Coin>,

    /// Vault share bonds
    pub vault_share_bonds: Vec<VaultShareBond>,
}

#[cw_serde]
pub struct VaultShareBond {
    /// Reference to the accepted vault share config
    pub share_denom: String,

    /// Amount of vault shares deposited
    pub share_amount: Uint128,

    /// Cached value in base tokens at last valuation
    /// Updated periodically or on-demand
    pub cached_value_base_tokens: Uint128,

    /// Timestamp of last valuation
    pub last_valued_at: u64,
}
```

### Storage

```rust
/// Map of accepted vault share denoms to their configuration
pub const ACCEPTED_VAULT_SHARES: Map<&str, AcceptedVaultShare> = Map::new("accepted_vault_shares");

/// Extended solver data with collateral breakdown
pub const SOLVER_COLLATERAL: Map<&str, SolverCollateral> = Map::new("solver_collateral");
```

### Admin Messages

```rust
pub enum ExecuteMsg {
    // ... existing messages ...

    /// Add a new accepted vault share type (admin only)
    AddAcceptedVaultShare {
        share_denom: String,
        vault_contract: String,
        vault_chain_id: String,
        ibc_connection_id: Option<String>,
        base_token_denom: String,
        collateral_ratio_bps: u64,
        name: String,
    },

    /// Update an existing vault share configuration (admin only)
    UpdateAcceptedVaultShare {
        share_denom: String,
        collateral_ratio_bps: Option<u64>,
        active: Option<bool>,
    },

    /// Remove an accepted vault share type (admin only)
    /// Only allowed if no solvers currently have this collateral
    RemoveAcceptedVaultShare {
        share_denom: String,
    },
}
```

### Query Messages

```rust
pub enum QueryMsg {
    // ... existing messages ...

    /// List all accepted vault share types
    #[returns(AcceptedVaultSharesResponse)]
    AcceptedVaultShares {},

    /// Get details for a specific vault share type
    #[returns(AcceptedVaultShare)]
    AcceptedVaultShare { share_denom: String },

    /// Get solver's total collateral value (native + vault shares)
    #[returns(SolverCollateralValueResponse)]
    SolverCollateralValue { solver_id: String },
}

#[cw_serde]
pub struct SolverCollateralValueResponse {
    /// Total collateral value in base tokens (uatom equivalent)
    pub total_value: Uint128,

    /// Breakdown by collateral type
    pub native_value: Uint128,
    pub vault_shares_value: Uint128,

    /// Individual vault share positions
    pub vault_share_positions: Vec<VaultSharePosition>,
}

#[cw_serde]
pub struct VaultSharePosition {
    pub share_denom: String,
    pub share_amount: Uint128,
    pub raw_value: Uint128,           // Value before haircut
    pub collateral_value: Uint128,    // Value after haircut
    pub collateral_ratio_bps: u64,
    pub last_valued_at: u64,
}
```

---

## 2. Value Retrieval: Same-Chain vs Cross-Chain

The Settlement Contract needs to determine the current value of vault shares. The approach differs based on whether the Inflow vault is on the same chain or a different chain.

### 2.1 Same-Chain Value Retrieval

When the Inflow vault contract is deployed on the same chain as the Settlement Contract (e.g., both on Neutron), we use direct contract queries.

#### Inflow Vault Query Interface

The Inflow vault exposes the `ControlCenterPoolInfo` query which returns the aggregate pool state:

```rust
// From hydro/packages/interface/src/inflow_vault.rs
pub enum QueryMsg {
    /// Get pool info in the format expected by the Control Center
    /// Returns total shares issued and total pool value in base tokens
    #[returns(ControlCenterPoolInfoResponse)]
    ControlCenterPoolInfo {},

    // ... other queries ...
}

/// Response for ControlCenterPoolInfo query
/// This provides the essential data needed to compute share value
pub struct ControlCenterPoolInfoResponse {
    /// Total vault shares currently issued
    pub total_shares_issued: Uint128,

    /// Total pool value in base tokens
    pub total_pool_value: Uint128,
}
```

#### Share Value Calculation

Given the `ControlCenterPoolInfo` response, we compute the value of a specific share amount:

```
share_value = (share_amount * total_pool_value) / total_shares_issued
```

This gives us the value in the vault's base token denomination.

#### Implementation

```rust
/// Cached pool info from vault queries
#[cw_serde]
pub struct CachedVaultPoolInfo {
    /// Total shares issued by the vault
    pub total_shares_issued: Uint128,

    /// Total pool value in base tokens
    pub total_pool_value: Uint128,

    /// When this cache entry was last updated
    pub updated_at: u64,
}

/// Query the ControlCenterPoolInfo from a same-chain Inflow vault
fn query_vault_pool_info_same_chain(
    deps: Deps,
    vault_contract: &str,
) -> StdResult<ControlCenterPoolInfoResponse> {
    deps.querier.query_wasm_smart(
        vault_contract,
        &InflowVaultQueryMsg::ControlCenterPoolInfo {},
    )
}

/// Calculate the base token value of vault shares using pool info
fn calculate_share_value(
    share_amount: Uint128,
    pool_info: &ControlCenterPoolInfoResponse,
) -> StdResult<Uint128> {
    if pool_info.total_shares_issued.is_zero() {
        // No shares issued yet; 1:1 ratio
        return Ok(share_amount);
    }

    // share_value = (share_amount * total_pool_value) / total_shares_issued
    share_amount
        .checked_multiply_ratio(pool_info.total_pool_value, pool_info.total_shares_issued)
        .map_err(|e| StdError::generic_err(format!("overflow in share value calculation: {}", e)))
}

/// Query the value of vault shares from a same-chain Inflow vault
fn query_vault_share_value_same_chain(
    deps: Deps,
    vault_contract: &str,
    share_amount: Uint128,
) -> StdResult<Uint128> {
    let pool_info = query_vault_pool_info_same_chain(deps, vault_contract)?;
    calculate_share_value(share_amount, &pool_info)
}

/// Calculate total collateral value for a solver (same-chain vaults)
fn calculate_solver_collateral_value_same_chain(
    deps: Deps,
    solver_collateral: &SolverCollateral,
) -> StdResult<Uint128> {
    let mut total_value = Uint128::zero();

    // Add native bonds
    for coin in &solver_collateral.native_bonds {
        if coin.denom == "uatom" {
            total_value = total_value.checked_add(coin.amount)?;
        }
        // TODO: Handle other native tokens with oracle prices
    }

    // Add vault share bonds
    for bond in &solver_collateral.vault_share_bonds {
        let config = ACCEPTED_VAULT_SHARES.load(deps.storage, &bond.share_denom)?;

        if config.vault_chain_id == CURRENT_CHAIN_ID {
            // Same-chain: direct query to get current pool info
            let raw_value = query_vault_share_value_same_chain(
                deps,
                &config.vault_contract,
                bond.share_amount,
            )?;

            // Apply collateral ratio haircut
            let collateral_value = raw_value
                .checked_mul(Uint128::from(config.collateral_ratio_bps))?
                .checked_div(Uint128::from(10000u64))?;

            total_value = total_value.checked_add(collateral_value)?;
        }
    }

    Ok(total_value)
}
```

### 2.2 Cross-Chain Value Retrieval (ICQ)

We can use IBC async queries to fetch `ControlCenterPoolInfo`.

```rust
/// Request vault pool info via IBC for share valuation
fn request_vault_pool_info_ibc(
    deps: DepsMut,
    env: Env,
    channel_id: String,
    vault_contract: String,
    share_denom: String,
) -> Result<Response, ContractError> {
    let query_request = InflowQueryRequest {
        vault_contract,
        query: InflowVaultQueryMsg::ControlCenterPoolInfo {},
        callback_id: share_denom.clone(),
    };

    let ibc_msg = IbcMsg::SendPacket {
        channel_id,
        data: to_json_binary(&query_request)?,
        timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(300)),
    };

    // Mark this query as pending
    PENDING_POOL_INFO_QUERIES.save(deps.storage, &share_denom, &PendingQuery {
        requested_at: env.block.time.seconds(),
    })?;

    Ok(Response::new()
        .add_message(ibc_msg)
        .add_attribute("action", "request_vault_pool_info"))
}

/// Handle IBC acknowledgement with ControlCenterPoolInfo result
fn ibc_packet_ack(
    deps: DepsMut,
    env: Env,
    ack: IbcPacketAckMsg,
) -> Result<Response, ContractError> {
    let response: ControlCenterPoolInfoResponse = from_json(&ack.acknowledgement.data)?;
    let share_denom = ack.original_packet.callback_id;

    // Cache the pool info for ratio calculations
    CACHED_VAULT_POOL_INFO.save(deps.storage, &share_denom, &CachedVaultPoolInfo {
        total_shares_issued: response.total_shares_issued,
        total_pool_value: response.total_pool_value,
        updated_at: env.block.time.seconds(),
    })?;

    PENDING_POOL_INFO_QUERIES.remove(deps.storage, &share_denom);

    Ok(Response::new()
        .add_attribute("action", "pool_info_received")
        .add_attribute("share_denom", share_denom))
}
```

### 2.3 Valuation Staleness and Safety

Regardless of retrieval method, we must handle stale valuations. For cross-chain vaults, we use cached `ControlCenterPoolInfo` data and compute the share value using the same ratio calculation.

```rust
/// Maximum age of a cached pool info before it's considered stale
pub const MAX_POOL_INFO_AGE_SECONDS: u64 = 3600; // 1 hour

/// Storage for cached pool info (cross-chain vaults)
pub const CACHED_VAULT_POOL_INFO: Map<&str, CachedVaultPoolInfo> = Map::new("cached_vault_pool_info");

/// Get the collateral value, with staleness checks
fn get_vault_share_collateral_value(
    deps: Deps,
    env: &Env,
    share_denom: &str,
    share_amount: Uint128,
) -> Result<Uint128, ContractError> {
    let config = ACCEPTED_VAULT_SHARES.load(deps.storage, share_denom)?;

    // Get raw value in base tokens
    let raw_value = if config.vault_chain_id == CURRENT_CHAIN_ID {
        // Same-chain: direct query to vault contract
        query_vault_share_value_same_chain(deps, &config.vault_contract, share_amount)?
    } else {
        // Cross-chain: use cached pool info
        let cached = CACHED_VAULT_POOL_INFO
            .load(deps.storage, share_denom)
            .map_err(|_| ContractError::NoCachedPoolInfo {
                share_denom: share_denom.to_string(),
            })?;

        // Check staleness
        let age = env.block.time.seconds().saturating_sub(cached.updated_at);
        if age > MAX_POOL_INFO_AGE_SECONDS {
            return Err(ContractError::StalePoolInfo {
                share_denom: share_denom.to_string(),
                age_seconds: age,
                max_age_seconds: MAX_POOL_INFO_AGE_SECONDS,
            });
        }

        // Calculate value using the same formula as same-chain
        // share_value = (share_amount * total_pool_value) / total_shares_issued
        calculate_share_value_from_cached(share_amount, &cached)?
    };

    // Apply collateral ratio haircut
    Ok(apply_haircut(raw_value, config.collateral_ratio_bps))
}

/// Calculate share value from cached pool info
fn calculate_share_value_from_cached(
    share_amount: Uint128,
    cached: &CachedVaultPoolInfo,
) -> StdResult<Uint128> {
    if cached.total_shares_issued.is_zero() {
        // No shares issued yet; 1:1 ratio
        return Ok(share_amount);
    }

    share_amount
        .checked_multiply_ratio(cached.total_pool_value, cached.total_shares_issued)
        .map_err(|e| StdError::generic_err(format!("overflow in share value calculation: {}", e)))
}

/// Apply collateral ratio haircut to raw value
fn apply_haircut(value: Uint128, ratio_bps: u64) -> Uint128 {
    value
        .checked_mul(Uint128::from(ratio_bps))
        .unwrap_or(Uint128::zero())
        .checked_div(Uint128::from(10000u64))
        .unwrap_or(Uint128::zero())
}
```

### 2.4 Base Token Denomination Handling

When computing collateral values, we need to ensure all values are expressed in a common denomination. The `base_token_denom` field in `AcceptedVaultShare` specifies what denomination the vault's base token maps to on the settlement chain.

```rust
/// Get the total collateral value in the settlement chain's native token (e.g., uatom)
fn get_total_collateral_value_in_native(
    deps: Deps,
    env: &Env,
    solver_collateral: &SolverCollateral,
) -> Result<Uint128, ContractError> {
    let mut total_value = Uint128::zero();

    // Add native bonds (already in native denomination)
    for coin in &solver_collateral.native_bonds {
        if coin.denom == NATIVE_DENOM {  // e.g., "uatom"
            total_value = total_value.checked_add(coin.amount)?;
        }
        // Other native tokens would need oracle conversion
    }

    // Add vault share bonds
    for bond in &solver_collateral.vault_share_bonds {
        let config = ACCEPTED_VAULT_SHARES.load(deps.storage, &bond.share_denom)?;

        // Get value in the vault's base token
        let value_in_base_token = get_vault_share_collateral_value(
            deps,
            env,
            &bond.share_denom,
            bond.share_amount,
        )?;

        // If base_token_denom matches native, use directly
        // Otherwise, would need oracle conversion (future work)
        if config.base_token_denom == NATIVE_DENOM {
            total_value = total_value.checked_add(value_in_base_token)?;
        } else {
            // TODO: Convert via oracle
            // For now, only accept vaults with native base token
            return Err(ContractError::UnsupportedBaseToken {
                share_denom: bond.share_denom.clone(),
                base_token: config.base_token_denom,
            });
        }
    }

    Ok(total_value)
}
```

---

## 3. Solver Registration with Vault Shares

### Extended Registration Flow

```rust
pub fn execute_register_solver_with_collateral(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    solver_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut native_bonds: Vec<Coin> = vec![];
    let mut vault_share_bonds: Vec<VaultShareBond> = vec![];

    // Process all sent funds
    for coin in info.funds {
        if coin.denom == "uatom" {
            native_bonds.push(coin);
        } else if ACCEPTED_VAULT_SHARES.has(deps.storage, &coin.denom) {
            let share_config = ACCEPTED_VAULT_SHARES.load(deps.storage, &coin.denom)?;

            if !share_config.active {
                return Err(ContractError::CollateralTypeNotActive {
                    denom: coin.denom,
                });
            }

            // Get current value
            let value = get_vault_share_collateral_value(
                deps.as_ref(),
                &env,
                &coin.denom,
                coin.amount,
            )?;

            vault_share_bonds.push(VaultShareBond {
                share_denom: coin.denom,
                share_amount: coin.amount,
                cached_value_base_tokens: value,
                last_valued_at: env.block.time.seconds(),
            });
        } else {
            return Err(ContractError::UnsupportedCollateralType {
                denom: coin.denom,
            });
        }
    }

    // Calculate total collateral value
    let native_value: Uint128 = native_bonds.iter().map(|c| c.amount).sum();
    let vault_shares_value: Uint128 = vault_share_bonds.iter()
        .map(|b| b.cached_value_base_tokens)
        .sum();
    let total_value = native_value.checked_add(vault_shares_value)?;

    // Check minimum bond requirement
    if total_value < config.min_solver_bond {
        return Err(ContractError::InsufficientBond {
            required: config.min_solver_bond.to_string(),
            provided: total_value.to_string(),
        });
    }

    // Save collateral
    SOLVER_COLLATERAL.save(deps.storage, &solver_id, &SolverCollateral {
        native_bonds,
        vault_share_bonds,
    })?;

    // Create solver record (existing logic)
    let solver = RegisteredSolver {
        id: solver_id.clone(),
        operator: info.sender.clone(),
        bond_amount: total_value,  // Now represents total collateral value
        active: true,
        total_settlements: 0,
        failed_settlements: 0,
        registered_at: env.block.time.seconds(),
    };

    SOLVERS.save(deps.storage, &solver_id, &solver)?;

    Ok(Response::new()
        .add_attribute("action", "register_solver")
        .add_attribute("solver_id", solver_id)
        .add_attribute("native_collateral", native_value)
        .add_attribute("vault_shares_collateral", vault_shares_value)
        .add_attribute("total_collateral", total_value))
}
```

---

## 4. Security Considerations

### 4.1 Collateral Ratio (Haircut)

Vault shares should have a collateral ratio < 100% to account for:
- **Withdrawal delays**: Inflow uses a queue-based withdrawal system
- **Price volatility**: Share values can fluctuate
- **Slippage risk**: Large withdrawals may impact the vault

Recommended ratios:
- ATOM-based vaults (hATOM): 80-85%
- LST-based vaults (hstATOM): 75-80%
- Stablecoin vaults (hUSD): 90-95%
- BTC vaults (hBTC): 70-75%

### 4.2 Valuation Freshness

- Same-chain queries: Always fresh (real-time)
- ICQ: Updated every N blocks (configurable)
- Oracle: Must be submitted within MAX_VALUATION_AGE_SECONDS

If valuation is stale:
- New registrations with that collateral type are rejected
- Existing solvers can still operate but may face restrictions

---

## 5. Implementation Plan

### Phase 1: Same-Chain Support
1. Add `AcceptedVaultShare` storage and admin messages
2. Implement `query_vault_share_value_same_chain`
3. Extend `RegisteredSolver` to track collateral breakdown
4. Update slashing logic to handle vault shares

### Phase 2: Cross-Chain Support (ICQ)
1. Add ICQ registration for cross-chain vaults
2. Implement callback handlers for ICQ results
3. Add caching layer with staleness checks
4. Test with Neutron ↔ Cosmos Hub

### Phase 3: Oracle Fallback
1. Add oracle whitelist management
2. Implement `execute_submit_vault_valuation`
3. Add monitoring/alerting for stale valuations

---

## 6. Open Questions

1. **Slashing vault shares**: When slashing, do we:
   - Burn the shares directly?
   - Initiate a withdrawal and slash the proceeds?
   - Transfer shares to a penalty pool?

2. **Partial collateral types**: Can a solver mix ATOM + iATOM + istATOM?
   - Current design: Yes, with individual tracking
   - Alternative: Require single collateral type per solver

3. **Rebalancing**: Can solvers swap collateral types after registration?
   - Needs careful handling to avoid undercollateralization

4. **Withdrawal queue interaction**: If vault has pending withdrawals, does that affect share value?
   - Inflow already subtracts `withdrawal_queue_base_tokens` from total value

---

## References

- [Inflow Vault Interface](/Users/offtermatt/projects/hydro/packages/interface/src/inflow_vault.rs)
- [Inflow Vault Contract](/Users/offtermatt/projects/hydro/contracts/inflow/vault/src/contract.rs)
- [Settlement Contract State](/Users/offtermatt/projects/atom-intents/contracts/settlement/src/state.rs)
- [Neutron ICQ Documentation](https://docs.neutron.org/neutron/modules/interchain-queries/overview)
