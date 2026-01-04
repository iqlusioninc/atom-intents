use cosmwasm_std::{Coin, StdError, StdResult, Storage, Uint128};

use crate::msg::{SettlementResponse, SolverReputationResponse, SolverResponse};
use crate::state::{
    extract_validator_from_lsm, is_lsm_share_denom, BondAsset, BondAssetType, FeeTier,
    LsmBondConfig, LstBondConfig, RegisteredSolver, Settlement, SettlementStatus, SolverReputation,
    LSM_CONFIG, LST_CONFIG,
};

/// Calculate reputation score based on solver performance metrics
///
/// Weighted formula:
/// - Success rate: 40% (0-4000 points)
/// - Volume: 20% (0-2000 points)
/// - Speed: 20% (0-2000 points)
/// - No slashing: 20% (0-2000 points)
pub fn calculate_reputation_score(rep: &SolverReputation) -> u64 {
    if rep.total_settlements == 0 {
        return 5000; // Default starting score for new solvers
    }

    // Success rate component (0-4000 points, 40%)
    let success_rate = if rep.total_settlements > 0 {
        (rep.successful_settlements * 4000) / rep.total_settlements
    } else {
        0
    };

    // Volume component (0-2000 points, 20%)
    // Scale: 0-10M = 0-2000 points (linear)
    let volume_score = {
        let volume_u64 = rep.total_volume.u128().min(10_000_000) as u64;
        (volume_u64 * 2000) / 10_000_000
    };

    // Speed component (0-2000 points, 20%)
    // Faster settlements get higher scores
    // Assume ideal settlement time is 60 seconds, max acceptable is 300 seconds
    let speed_score = if rep.average_settlement_time == 0 {
        2000
    } else if rep.average_settlement_time <= 60 {
        2000
    } else if rep.average_settlement_time >= 300 {
        0
    } else {
        2000 - ((rep.average_settlement_time - 60) * 2000) / 240
    };

    // No slashing component (0-2000 points, 20%)
    // Each slashing event reduces score
    let slash_penalty = rep.slashing_events.min(10) * 200; // -200 points per slash, max -2000
    let slash_score = 2000u64.saturating_sub(slash_penalty);

    // Total score
    let total = success_rate + volume_score + speed_score + slash_score;
    total.min(10000) // Cap at 10000
}

/// Get solver fee tier based on reputation score
pub fn get_solver_fee_tier(score: u64) -> FeeTier {
    match score {
        9000..=10000 => FeeTier::Premium,
        7000..=8999 => FeeTier::Standard,
        5000..=6999 => FeeTier::Basic,
        _ => FeeTier::New,
    }
}

/// Convert SolverReputation to response format
pub fn reputation_to_response(rep: SolverReputation) -> SolverReputationResponse {
    let fee_tier = match get_solver_fee_tier(rep.reputation_score) {
        FeeTier::Premium => "premium".to_string(),
        FeeTier::Standard => "standard".to_string(),
        FeeTier::Basic => "basic".to_string(),
        FeeTier::New => "new".to_string(),
    };

    SolverReputationResponse {
        solver_id: rep.solver_id,
        total_settlements: rep.total_settlements,
        successful_settlements: rep.successful_settlements,
        failed_settlements: rep.failed_settlements,
        total_volume: rep.total_volume,
        average_settlement_time: rep.average_settlement_time,
        slashing_events: rep.slashing_events,
        reputation_score: rep.reputation_score,
        fee_tier,
        last_updated: rep.last_updated,
    }
}

/// Convert RegisteredSolver to response format
pub fn solver_to_response(solver: RegisteredSolver) -> SolverResponse {
    SolverResponse {
        id: solver.id,
        operator: solver.operator.to_string(),
        bond_amount: solver.bond_amount,
        active: solver.active,
        total_settlements: solver.total_settlements,
        failed_settlements: solver.failed_settlements,
        registered_at: solver.registered_at,
    }
}

/// Convert Settlement to response format
pub fn settlement_to_response(settlement: Settlement) -> SettlementResponse {
    let status = match settlement.status {
        SettlementStatus::Pending => "pending".to_string(),
        SettlementStatus::UserLocked => "user_locked".to_string(),
        SettlementStatus::SolverLocked => "solver_locked".to_string(),
        SettlementStatus::Executing => "executing".to_string(),
        SettlementStatus::Completed => "completed".to_string(),
        SettlementStatus::Failed { reason } => format!("failed: {}", reason),
        SettlementStatus::Slashed { amount } => format!("slashed: {}", amount),
    };

    SettlementResponse {
        id: settlement.id,
        intent_id: settlement.intent_id,
        solver_id: settlement.solver_id,
        user: settlement.user.to_string(),
        user_input_amount: settlement.user_input_amount,
        user_input_denom: settlement.user_input_denom,
        solver_output_amount: settlement.solver_output_amount,
        solver_output_denom: settlement.solver_output_denom,
        status,
        created_at: settlement.created_at,
        expires_at: settlement.expires_at,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LSM & LST BOND VALUATION HELPERS
// ═══════════════════════════════════════════════════════════════════════════

/// Result of validating and valuing a bond coin
pub struct BondValuation {
    pub asset: BondAsset,
    pub is_valid: bool,
    pub error: Option<String>,
}

/// Validate and calculate ATOM-equivalent value for a coin being used as bond
///
/// Supports:
/// - Native ATOM (uatom): 1:1 value
/// - LSM shares (cosmosvaloperXXX/YYY): Valued with discount per LSM config
/// - LST tokens (stuatom, uqatom, stk/uatom): Valued per LST config exchange rates
pub fn validate_and_value_bond_coin(
    storage: &dyn Storage,
    coin: &Coin,
) -> StdResult<BondValuation> {
    // Check if it's native ATOM
    if coin.denom == "uatom" {
        return Ok(BondValuation {
            asset: BondAsset {
                denom: coin.denom.clone(),
                amount: coin.amount,
                asset_type: BondAssetType::NativeAtom,
                atom_value: coin.amount, // 1:1 for native ATOM
            },
            is_valid: true,
            error: None,
        });
    }

    // Check if it's an LSM share
    if is_lsm_share_denom(&coin.denom) {
        return validate_lsm_share(storage, coin);
    }

    // Check if it's a supported LST
    return validate_lst(storage, coin);
}

/// Validate and value an LSM share
fn validate_lsm_share(storage: &dyn Storage, coin: &Coin) -> StdResult<BondValuation> {
    // Load LSM config
    let lsm_config = LSM_CONFIG
        .may_load(storage)?
        .unwrap_or_else(LsmBondConfig::default);

    // Check if LSM bonding is enabled
    if !lsm_config.enabled {
        return Ok(BondValuation {
            asset: BondAsset {
                denom: coin.denom.clone(),
                amount: coin.amount,
                asset_type: BondAssetType::LsmShare {
                    validator: String::new(),
                },
                atom_value: Uint128::zero(),
            },
            is_valid: false,
            error: Some("LSM share bonding is not enabled".to_string()),
        });
    }

    // Extract validator address
    let validator = extract_validator_from_lsm(&coin.denom).ok_or_else(|| {
        StdError::generic_err(format!("Invalid LSM share denom: {}", coin.denom))
    })?;

    // Check if validator is blocked
    if lsm_config.blocked_validators.contains(&validator) {
        return Ok(BondValuation {
            asset: BondAsset {
                denom: coin.denom.clone(),
                amount: coin.amount,
                asset_type: BondAssetType::LsmShare {
                    validator: validator.clone(),
                },
                atom_value: Uint128::zero(),
            },
            is_valid: false,
            error: Some(format!("Validator {} is blocked", validator)),
        });
    }

    // Calculate ATOM-equivalent value with discount
    // LSM shares are 1:1 with staked ATOM, but we apply a liquidity discount
    let atom_value =
        coin.amount * Uint128::from(lsm_config.valuation_discount_bps) / Uint128::from(10000u64);

    Ok(BondValuation {
        asset: BondAsset {
            denom: coin.denom.clone(),
            amount: coin.amount,
            asset_type: BondAssetType::LsmShare { validator },
            atom_value,
        },
        is_valid: true,
        error: None,
    })
}

/// Validate and value an LST token
fn validate_lst(storage: &dyn Storage, coin: &Coin) -> StdResult<BondValuation> {
    // Load LST config
    let lst_config = LST_CONFIG
        .may_load(storage)?
        .unwrap_or_else(LstBondConfig::default);

    // Check if LST bonding is enabled globally
    if !lst_config.enabled {
        return Ok(BondValuation {
            asset: BondAsset {
                denom: coin.denom.clone(),
                amount: coin.amount,
                asset_type: BondAssetType::Lst {
                    protocol: String::new(),
                },
                atom_value: Uint128::zero(),
            },
            is_valid: false,
            error: Some("LST bonding is not enabled".to_string()),
        });
    }

    // Find token config
    let token_config = lst_config
        .accepted_tokens
        .iter()
        .find(|t| t.denom == coin.denom);

    match token_config {
        Some(config) => {
            if !config.enabled {
                return Ok(BondValuation {
                    asset: BondAsset {
                        denom: coin.denom.clone(),
                        amount: coin.amount,
                        asset_type: BondAssetType::Lst {
                            protocol: config.protocol.clone(),
                        },
                        atom_value: Uint128::zero(),
                    },
                    is_valid: false,
                    error: Some(format!("LST {} is not enabled for bonding", coin.denom)),
                });
            }

            // Calculate ATOM-equivalent value using exchange rate
            let atom_value =
                coin.amount * Uint128::from(config.exchange_rate_bps) / Uint128::from(10000u64);

            Ok(BondValuation {
                asset: BondAsset {
                    denom: coin.denom.clone(),
                    amount: coin.amount,
                    asset_type: BondAssetType::Lst {
                        protocol: config.protocol.clone(),
                    },
                    atom_value,
                },
                is_valid: true,
                error: None,
            })
        }
        None => Ok(BondValuation {
            asset: BondAsset {
                denom: coin.denom.clone(),
                amount: coin.amount,
                asset_type: BondAssetType::Lst {
                    protocol: String::new(),
                },
                atom_value: Uint128::zero(),
            },
            is_valid: false,
            error: Some(format!("Token {} is not accepted for bonding", coin.denom)),
        }),
    }
}

/// Validate all coins sent for bonding and calculate total ATOM value
pub fn validate_and_value_bond_funds(
    storage: &dyn Storage,
    funds: &[Coin],
) -> StdResult<(Vec<BondAsset>, Uint128, Vec<String>)> {
    let mut assets = Vec::new();
    let mut total_value = Uint128::zero();
    let mut errors = Vec::new();

    for coin in funds {
        let valuation = validate_and_value_bond_coin(storage, coin)?;

        if valuation.is_valid {
            total_value += valuation.asset.atom_value;
            assets.push(valuation.asset);
        } else if let Some(error) = valuation.error {
            errors.push(error);
        }
    }

    Ok((assets, total_value, errors))
}

/// Check if a solver's LSM bond would exceed limits after adding new LSM shares
pub fn check_lsm_limits(
    storage: &dyn Storage,
    current_lsm_value: Uint128,
    new_lsm_value: Uint128,
) -> StdResult<bool> {
    let lsm_config = LSM_CONFIG
        .may_load(storage)?
        .unwrap_or_else(LsmBondConfig::default);

    if let Some(max_per_solver) = lsm_config.max_lsm_per_solver {
        if current_lsm_value + new_lsm_value > max_per_solver {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Check if a solver's LST bond would exceed limits after adding new LSTs
pub fn check_lst_limits(
    storage: &dyn Storage,
    solver_bond: &crate::state::SolverBond,
    new_assets: &[BondAsset],
) -> StdResult<Vec<String>> {
    let lst_config = LST_CONFIG
        .may_load(storage)?
        .unwrap_or_else(LstBondConfig::default);

    let mut errors = Vec::new();

    // Check per-token limits
    for new_asset in new_assets {
        if let BondAssetType::Lst { .. } = &new_asset.asset_type {
            if let Some(token_config) = lst_config
                .accepted_tokens
                .iter()
                .find(|t| t.denom == new_asset.denom)
            {
                if let Some(max_per_solver) = token_config.max_per_solver {
                    // Calculate current amount of this token
                    let current_amount: Uint128 = solver_bond
                        .assets
                        .iter()
                        .filter(|a| a.denom == new_asset.denom)
                        .map(|a| a.amount)
                        .sum();

                    if current_amount + new_asset.amount > max_per_solver {
                        errors.push(format!(
                            "Adding {} {} would exceed max limit of {} for this token",
                            new_asset.amount, new_asset.denom, max_per_solver
                        ));
                    }
                }
            }
        }
    }

    // Check total LST limit
    if let Some(max_total) = lst_config.max_lst_per_solver {
        let current_lst_value: Uint128 = solver_bond
            .assets
            .iter()
            .filter(|a| matches!(a.asset_type, BondAssetType::Lst { .. }))
            .map(|a| a.atom_value)
            .sum();

        let new_lst_value: Uint128 = new_assets
            .iter()
            .filter(|a| matches!(a.asset_type, BondAssetType::Lst { .. }))
            .map(|a| a.atom_value)
            .sum();

        if current_lst_value + new_lst_value > max_total {
            errors.push(format!(
                "Total LST value would exceed max limit of {} ATOM equivalent",
                max_total
            ));
        }
    }

    Ok(errors)
}
