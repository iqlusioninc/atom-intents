//! Settlement simulation for the demo

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rand::Rng;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use crate::models::*;
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

/// Run the settlement processor loop
pub async fn run_settlement_processor(state: AppStateRef) {
    let mut interval = tokio::time::interval(Duration::from_millis(500));

    loop {
        interval.tick().await;
        process_settlements(&state).await;
    }
}

async fn process_settlements(state: &AppStateRef) {
    // Get settlements that need processing
    let pending_settlements: Vec<String> = {
        let state = state.read().await;
        state
            .settlements
            .values()
            .filter(|s| s.status != SettlementStatus::Completed && s.status != SettlementStatus::Failed)
            .map(|s| s.id.clone())
            .collect()
    };

    for settlement_id in pending_settlements {
        process_single_settlement(state, &settlement_id).await;
    }
}

async fn process_single_settlement(state: &AppStateRef, settlement_id: &str) {
    let mut rng = rand::thread_rng();

    // Simulate processing delay
    tokio::time::sleep(Duration::from_millis(rng.gen_range(100..300))).await;

    let mut state = state.write().await;
    let settlement = match state.settlements.get_mut(settlement_id) {
        Some(s) => s,
        None => return,
    };

    let now = Utc::now();
    settlement.updated_at = now;

    // Progress through settlement phases
    match settlement.phase {
        SettlementPhase::Init => {
            // Start escrow lock
            settlement.phase = SettlementPhase::EscrowLocked;
            settlement.status = SettlementStatus::Committing;
            settlement.escrow_txid = Some(format!("tx_{}", Uuid::new_v4()));
            settlement.events.push(SettlementEvent {
                event_type: "escrow_locked".to_string(),
                timestamp: now,
                description: "User funds locked in escrow contract".to_string(),
                metadata: serde_json::json!({
                    "txid": settlement.escrow_txid,
                    "amount": settlement.input_amount,
                }),
            });

            info!(
                "Settlement {} - escrow locked",
                settlement.id
            );
        }
        SettlementPhase::EscrowLocked => {
            // Solver commits output
            settlement.phase = SettlementPhase::SolverCommitted;
            settlement.events.push(SettlementEvent {
                event_type: "solver_committed".to_string(),
                timestamp: now,
                description: "Solver committed output funds".to_string(),
                metadata: serde_json::json!({
                    "solver_id": settlement.solver_id,
                    "output_amount": settlement.output_amount,
                }),
            });

            info!(
                "Settlement {} - solver committed",
                settlement.id
            );
        }
        SettlementPhase::SolverCommitted => {
            // Initiate IBC transfer (if cross-chain)
            settlement.phase = SettlementPhase::IbcInFlight;
            settlement.status = SettlementStatus::Executing;
            settlement.ibc_packet_id = Some(format!("ibc_{}_{}",
                rng.gen::<u32>(),
                rng.gen::<u32>()
            ));
            settlement.events.push(SettlementEvent {
                event_type: "ibc_initiated".to_string(),
                timestamp: now,
                description: "IBC packet submitted".to_string(),
                metadata: serde_json::json!({
                    "packet_id": settlement.ibc_packet_id,
                    "estimated_completion": "3-5 seconds",
                }),
            });

            info!(
                "Settlement {} - IBC in flight",
                settlement.id
            );
        }
        SettlementPhase::IbcInFlight => {
            // Simulate IBC completion (with small chance of failure for realism)
            let success = rng.gen_bool(0.95); // 95% success rate

            if success {
                settlement.phase = SettlementPhase::Finalized;
                settlement.status = SettlementStatus::Completed;
                settlement.completed_at = Some(now);
                settlement.execution_txid = Some(format!("tx_{}", Uuid::new_v4()));
                settlement.events.push(SettlementEvent {
                    event_type: "completed".to_string(),
                    timestamp: now,
                    description: "Settlement completed successfully".to_string(),
                    metadata: serde_json::json!({
                        "execution_txid": settlement.execution_txid,
                        "output_amount": settlement.output_amount,
                    }),
                });

                // Update intent status
                for intent_id in &settlement.intent_ids {
                    if let Some(intent) = state.intents.get_mut(intent_id) {
                        intent.status = IntentStatus::Completed;
                    }
                }

                // Update solver stats
                if let Some(solver) = state.solvers.get_mut(&settlement.solver_id) {
                    solver.total_volume += settlement.input_amount;
                }

                // Update system stats
                state.stats.pending_intents = state.stats.pending_intents.saturating_sub(
                    settlement.intent_ids.len() as u64
                );

                info!(
                    "Settlement {} - completed successfully",
                    settlement.id
                );
            } else {
                // Simulate failure and refund
                settlement.status = SettlementStatus::Failed;
                settlement.events.push(SettlementEvent {
                    event_type: "failed".to_string(),
                    timestamp: now,
                    description: "Settlement failed - initiating refund".to_string(),
                    metadata: serde_json::json!({
                        "reason": "IBC timeout",
                    }),
                });

                // Mark for refund
                settlement.status = SettlementStatus::Refunded;
                settlement.events.push(SettlementEvent {
                    event_type: "refunded".to_string(),
                    timestamp: now,
                    description: "User funds refunded from escrow".to_string(),
                    metadata: serde_json::json!({
                        "refund_amount": settlement.input_amount,
                    }),
                });

                // Update intent status
                for intent_id in &settlement.intent_ids {
                    if let Some(intent) = state.intents.get_mut(intent_id) {
                        intent.status = IntentStatus::Failed;
                    }
                }

                info!(
                    "Settlement {} - failed and refunded",
                    settlement.id
                );
            }
        }
        SettlementPhase::Finalized => {
            // Already complete, nothing to do
        }
    }

    // Broadcast settlement update
    let settlement_clone = settlement.clone();
    state.broadcast(WsMessage::SettlementUpdate(settlement_clone));

    debug!(
        "Processed settlement {} - phase: {:?}, status: {:?}",
        settlement_id, settlement.phase, settlement.status
    );
}
