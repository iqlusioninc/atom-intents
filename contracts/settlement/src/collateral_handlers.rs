use atom_intents_types::collateral::{
    apply_haircut, calculate_lock_requirement, AssetClass, CachedPoolInfo, CollateralAsset,
    LiquidationIntent, LiquidationStatus, LockedCollateral, SolverBondPool, MAX_STALENESS_SECONDS,
};
use cosmwasm_std::{BankMsg, Coin, DepsMut, Env, MessageInfo, Response, Storage, Uint128};

use crate::error::ContractError;
use crate::state::{
    Settlement, BOND_POOLS, CACHED_POOL_INFO, CONFIG, LIQUIDATION_INTENTS,
    SETTLEMENT_LIQUIDATIONS, SOLVERS,
};

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL DEPOSIT / WITHDRAW
// ═══════════════════════════════════════════════════════════════════════════

/// Deposit collateral to a solver's bond pool
pub fn execute_deposit_collateral(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    solver_id: String,
    asset: CollateralAsset,
) -> Result<Response, ContractError> {
    // Verify solver exists and caller is operator
    let solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

    if info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    // Find the matching funds sent
    let denom = asset.denom();
    let amount: Uint128 = info
        .funds
        .iter()
        .filter(|c| c.denom == denom)
        .map(|c| c.amount)
        .sum();

    if amount.is_zero() {
        return Err(ContractError::InsufficientFunds {
            required: "non-zero".to_string(),
            provided: "0".to_string(),
        });
    }

    // Load or create bond pool
    let mut pool = BOND_POOLS
        .may_load(deps.storage, &solver_id)?
        .unwrap_or_else(|| SolverBondPool::new(solver_id.clone()));

    // Add the deposit
    pool.add_deposit(asset.clone(), amount);

    BOND_POOLS.save(deps.storage, &solver_id, &pool)?;

    Ok(Response::new()
        .add_attribute("action", "deposit_collateral")
        .add_attribute("solver_id", solver_id)
        .add_attribute("asset_class", format!("{:?}", asset.asset_class()))
        .add_attribute("denom", denom)
        .add_attribute("amount", amount))
}

/// Withdraw unlocked collateral from a solver's bond pool
pub fn execute_withdraw_collateral(
    deps: DepsMut,
    info: MessageInfo,
    solver_id: String,
    asset: CollateralAsset,
    amount: Uint128,
) -> Result<Response, ContractError> {
    // Verify solver exists and caller is operator
    let solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

    if info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    // Load bond pool
    let mut pool =
        BOND_POOLS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::NoBondPool {
                solver_id: solver_id.clone(),
            })?;

    // Find the deposit
    let deposit = pool.find_by_asset_mut(&asset).ok_or_else(|| {
        ContractError::InsufficientCollateral {
            required: amount.to_string(),
            available: "0".to_string(),
        }
    })?;

    // Check available (unlocked) amount
    if deposit.available() < amount {
        return Err(ContractError::InsufficientCollateral {
            required: amount.to_string(),
            available: deposit.available().to_string(),
        });
    }

    // Reduce the deposit
    deposit.amount = deposit.amount.checked_sub(amount).unwrap();

    BOND_POOLS.save(deps.storage, &solver_id, &pool)?;

    // Send the tokens back
    let denom = asset.denom().to_string();
    let send_msg = BankMsg::Send {
        to_address: solver.operator.to_string(),
        amount: vec![Coin { denom: denom.clone(), amount }],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "withdraw_collateral")
        .add_attribute("solver_id", solver_id)
        .add_attribute("denom", denom)
        .add_attribute("amount", amount))
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL LOCKING (CALLED DURING SETTLEMENT)
// ═══════════════════════════════════════════════════════════════════════════

/// Lock collateral for a settlement (1.5x fill value)
/// Called internally when a settlement is created
pub fn lock_collateral_for_settlement(
    storage: &mut dyn Storage,
    solver_id: &str,
    fill_value: Uint128,
    preferred_asset_class: Option<AssetClass>,
) -> Result<LockedCollateral, ContractError> {
    let required_value = calculate_lock_requirement(fill_value);

    // Load bond pool
    let mut pool =
        BOND_POOLS
            .load(storage, solver_id)
            .map_err(|_| ContractError::NoBondPool {
                solver_id: solver_id.to_string(),
            })?;

    // Find a deposit with sufficient available value
    // Priority: preferred class > Native > LSM > Hydro
    let classes_to_try = if let Some(pref) = preferred_asset_class {
        vec![pref, AssetClass::Native, AssetClass::LsmShare, AssetClass::HydroVault]
    } else {
        vec![AssetClass::Native, AssetClass::LsmShare, AssetClass::HydroVault]
    };

    let mut selected: Option<(CollateralAsset, Uint128, Uint128)> = None;

    for class in classes_to_try {
        if let Some(deposit) = pool.find_by_class_mut(class) {
            let haircut_bps = deposit.asset.haircut_bps();
            let available = deposit.available();

            // Calculate value after haircut
            let available_value = apply_haircut(available, haircut_bps);

            if available_value >= required_value {
                // Calculate raw amount needed
                // required_value = raw_amount * (10000 - haircut) / 10000
                // raw_amount = required_value * 10000 / (10000 - haircut)
                let effective_bps = 10000u64.saturating_sub(haircut_bps);
                let raw_amount = if effective_bps == 0 {
                    return Err(ContractError::InsufficientCollateral {
                        required: required_value.to_string(),
                        available: "0".to_string(),
                    });
                } else {
                    required_value
                        .checked_mul(Uint128::from(10000u64))
                        .unwrap()
                        .checked_div(Uint128::from(effective_bps))
                        .unwrap()
                };

                // Lock the amount
                deposit.lock(raw_amount)?;

                selected = Some((deposit.asset.clone(), raw_amount, required_value));
                break;
            }
        }
    }

    let (asset, amount, value) = selected.ok_or_else(|| ContractError::InsufficientCollateral {
        required: required_value.to_string(),
        available: "insufficient across all asset classes".to_string(),
    })?;

    BOND_POOLS.save(storage, solver_id, &pool)?;

    Ok(LockedCollateral { asset, amount, value })
}

/// Unlock collateral after settlement completes successfully
pub fn unlock_collateral_for_settlement(
    storage: &mut dyn Storage,
    solver_id: &str,
    locked: &LockedCollateral,
) -> Result<(), ContractError> {
    let mut pool =
        BOND_POOLS
            .load(storage, solver_id)
            .map_err(|_| ContractError::NoBondPool {
                solver_id: solver_id.to_string(),
            })?;

    let deposit = pool.find_by_asset_mut(&locked.asset).ok_or_else(|| {
        ContractError::InsufficientCollateral {
            required: locked.amount.to_string(),
            available: "deposit not found".to_string(),
        }
    })?;

    deposit.unlock(locked.amount)?;

    BOND_POOLS.save(storage, solver_id, &pool)?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// POOL INFO CACHING (FOR CROSS-CHAIN HYDRO VAULTS)
// ═══════════════════════════════════════════════════════════════════════════

/// Update cached pool info (called via ICQ callback)
pub fn execute_update_pool_info(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    share_denom: String,
    total_shares_issued: Uint128,
    total_pool_value: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin can update (would be called by ICQ relayer via admin)
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let pool_info = CachedPoolInfo {
        total_shares_issued,
        total_pool_value,
        updated_at: env.block.time.seconds(),
    };

    CACHED_POOL_INFO.save(deps.storage, &share_denom, &pool_info)?;

    Ok(Response::new()
        .add_attribute("action", "update_pool_info")
        .add_attribute("share_denom", share_denom)
        .add_attribute("total_shares_issued", total_shares_issued)
        .add_attribute("total_pool_value", total_pool_value))
}

// ═══════════════════════════════════════════════════════════════════════════
// LIQUIDATION HANDLERS
// ═══════════════════════════════════════════════════════════════════════════

/// Create a liquidation intent when slashing non-native collateral
pub fn create_liquidation_intent(
    storage: &mut dyn Storage,
    current_time: u64,
    settlement: &Settlement,
    locked: &LockedCollateral,
    slashed_solver: &str,
) -> Result<String, ContractError> {
    let liquidation_id = format!("liq-{}", settlement.id);

    // Check if already exists
    if LIQUIDATION_INTENTS.has(storage, &liquidation_id) {
        return Err(ContractError::LiquidationAlreadyExists { id: liquidation_id });
    }

    // Minimum output is the user's input amount (what they lost)
    let min_output_amount = settlement.user_input_amount;

    let intent = LiquidationIntent {
        id: liquidation_id.clone(),
        source_settlement_id: settlement.id.clone(),
        slashed_solver: slashed_solver.to_string(),
        collateral: locked.clone(),
        min_output_amount,
        beneficiary: settlement.user.clone(),
        timeout: current_time + 86400, // 24 hour timeout
        status: LiquidationStatus::Pending,
    };

    LIQUIDATION_INTENTS.save(storage, &liquidation_id, &intent)?;
    SETTLEMENT_LIQUIDATIONS.save(storage, &settlement.id, &liquidation_id)?;

    Ok(liquidation_id)
}

/// Submit a bid on a liquidation intent
pub fn execute_bid_on_liquidation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    liquidation_id: String,
    offered_output: Uint128,
) -> Result<Response, ContractError> {
    let mut intent = LIQUIDATION_INTENTS
        .load(deps.storage, &liquidation_id)
        .map_err(|_| ContractError::LiquidationNotFound {
            id: liquidation_id.clone(),
        })?;

    // Check not expired
    if env.block.time.seconds() > intent.timeout {
        return Err(ContractError::SettlementExpired {});
    }

    // Check not the slashed solver
    // Need to find solver by operator address
    let is_slashed = SOLVERS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .any(|(_, s)| s.id == intent.slashed_solver && s.operator == info.sender);

    if is_slashed {
        return Err(ContractError::SlashedSolverCannotBid {});
    }

    // Check bid meets minimum
    if offered_output < intent.min_output_amount {
        return Err(ContractError::BidBelowMinimum {
            offered: offered_output.to_string(),
            minimum: intent.min_output_amount.to_string(),
        });
    }

    // For now, use simple first-valid-bid-wins model
    // A more sophisticated auction could be added later
    match intent.status {
        LiquidationStatus::Pending => {
            intent.status = LiquidationStatus::Auctioning;
        }
        LiquidationStatus::Auctioning => {
            // Already in auction, new bids are accepted
        }
        _ => {
            return Err(ContractError::InvalidStateTransition {
                from: format!("{:?}", intent.status),
                to: "Auctioning".to_string(),
            });
        }
    }

    LIQUIDATION_INTENTS.save(deps.storage, &liquidation_id, &intent)?;

    Ok(Response::new()
        .add_attribute("action", "bid_on_liquidation")
        .add_attribute("liquidation_id", liquidation_id)
        .add_attribute("bidder", info.sender)
        .add_attribute("offered_output", offered_output))
}

/// Execute a liquidation (settle the winning bid)
pub fn execute_liquidation(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    liquidation_id: String,
) -> Result<Response, ContractError> {
    let mut intent = LIQUIDATION_INTENTS
        .load(deps.storage, &liquidation_id)
        .map_err(|_| ContractError::LiquidationNotFound {
            id: liquidation_id.clone(),
        })?;

    // Check status
    match intent.status {
        LiquidationStatus::Auctioning | LiquidationStatus::Pending => {}
        _ => {
            return Err(ContractError::InvalidStateTransition {
                from: format!("{:?}", intent.status),
                to: "Settled".to_string(),
            });
        }
    }

    // Verify the caller sent enough ATOM
    let atom_sent: Uint128 = info
        .funds
        .iter()
        .filter(|c| c.denom == "uatom")
        .map(|c| c.amount)
        .sum();

    if atom_sent < intent.min_output_amount {
        return Err(ContractError::InsufficientFunds {
            required: intent.min_output_amount.to_string(),
            provided: atom_sent.to_string(),
        });
    }

    // Transfer ATOM to beneficiary
    let send_to_beneficiary = BankMsg::Send {
        to_address: intent.beneficiary.to_string(),
        amount: vec![Coin {
            denom: "uatom".to_string(),
            amount: atom_sent,
        }],
    };

    // Transfer collateral to the liquidator
    let collateral_denom = intent.collateral.asset.denom().to_string();
    let send_collateral = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: collateral_denom.clone(),
            amount: intent.collateral.amount,
        }],
    };

    // Update intent status
    intent.status = LiquidationStatus::Settled {
        output_amount: atom_sent,
    };
    LIQUIDATION_INTENTS.save(deps.storage, &liquidation_id, &intent)?;

    Ok(Response::new()
        .add_message(send_to_beneficiary)
        .add_message(send_collateral)
        .add_attribute("action", "execute_liquidation")
        .add_attribute("liquidation_id", liquidation_id)
        .add_attribute("liquidator", info.sender)
        .add_attribute("atom_to_beneficiary", atom_sent)
        .add_attribute("collateral_to_liquidator", intent.collateral.amount)
        .add_attribute("collateral_denom", collateral_denom))
}

/// Cancel an expired liquidation intent (admin only)
pub fn execute_cancel_liquidation(
    deps: DepsMut,
    info: MessageInfo,
    liquidation_id: String,
    reason: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut intent = LIQUIDATION_INTENTS
        .load(deps.storage, &liquidation_id)
        .map_err(|_| ContractError::LiquidationNotFound {
            id: liquidation_id.clone(),
        })?;

    intent.status = LiquidationStatus::Failed {
        reason: reason.clone(),
    };
    LIQUIDATION_INTENTS.save(deps.storage, &liquidation_id, &intent)?;

    Ok(Response::new()
        .add_attribute("action", "cancel_liquidation")
        .add_attribute("liquidation_id", liquidation_id)
        .add_attribute("reason", reason))
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

/// Get the collateral value for a solver's bond pool (after haircuts)
pub fn get_solver_collateral_value(
    deps: &DepsMut,
    env: &Env,
    solver_id: &str,
) -> Result<Uint128, ContractError> {
    let pool = match BOND_POOLS.may_load(deps.storage, solver_id)? {
        Some(p) => p,
        None => return Ok(Uint128::zero()),
    };

    let mut total_value = Uint128::zero();

    for deposit in &pool.deposits {
        let raw_value = get_deposit_raw_value(deps, env, deposit)?;
        let value_after_haircut = apply_haircut(raw_value, deposit.asset.haircut_bps());
        total_value = total_value.checked_add(value_after_haircut).unwrap();
    }

    Ok(total_value)
}

/// Get raw value of a deposit (before haircut)
fn get_deposit_raw_value(
    deps: &DepsMut,
    env: &Env,
    deposit: &atom_intents_types::collateral::CollateralDeposit,
) -> Result<Uint128, ContractError> {
    match &deposit.asset {
        CollateralAsset::Native { denom } => {
            // Native ATOM is 1:1
            if denom == "uatom" {
                Ok(deposit.amount)
            } else {
                // Would need oracle for non-ATOM natives
                Err(ContractError::NoCachedPoolInfo {
                    share_denom: denom.clone(),
                })
            }
        }
        CollateralAsset::LsmShare { .. } => {
            // LSM shares are 1:1 with ATOM (haircut covers slashing risk)
            Ok(deposit.amount)
        }
        CollateralAsset::HydroVault { share_denom, .. } => {
            // Use cached pool info
            let pool_info = CACHED_POOL_INFO
                .load(deps.storage, share_denom)
                .map_err(|_| ContractError::NoCachedPoolInfo {
                    share_denom: share_denom.clone(),
                })?;

            // Check staleness
            if pool_info.is_stale(env.block.time.seconds()) {
                return Err(ContractError::StalePoolInfo {
                    share_denom: share_denom.clone(),
                    age_seconds: env.block.time.seconds() - pool_info.updated_at,
                    max_seconds: MAX_STALENESS_SECONDS,
                });
            }

            pool_info.calculate_share_value(deposit.amount).ok_or_else(|| {
                ContractError::InsufficientCollateral {
                    required: "calculation failed".to_string(),
                    available: "0".to_string(),
                }
            })
        }
    }
}
