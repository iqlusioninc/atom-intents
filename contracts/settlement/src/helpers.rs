use crate::msg::{SettlementResponse, SolverReputationResponse, SolverResponse};
use crate::state::{FeeTier, RegisteredSolver, Settlement, SettlementStatus, SolverReputation};

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
