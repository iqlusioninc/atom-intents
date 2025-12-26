//! Settlement simulation for the demo
//!
//! Simulates the settlement flow including:
//! - Local escrow (for Hub-native users)
//! - Cross-chain escrow via IBC Hooks (for chains like Celestia without smart contracts)

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

/// Chains that don't have smart contracts and require Hub escrow via IBC Hooks
const NON_WASM_CHAINS: &[&str] = &["celestia", "noble-1"];

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
    // Generate random values before await
    let (delay_ms, ibc_packet_id, success, execution_txid, escrow_ibc_packet) = {
        let mut rng = rand::thread_rng();
        (
            rng.gen_range(100..300),
            format!("ibc_{}_{}", rng.gen::<u32>(), rng.gen::<u32>()),
            rng.gen_bool(0.95),
            format!("tx_{}", Uuid::new_v4()),
            format!("ibc_escrow_{}_{}", rng.gen::<u32>(), rng.gen::<u32>()),
        )
    };

    // Simulate processing delay
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;

    let mut state = state.write().await;

    // First, get the current phase and necessary data from settlement and intent
    let (current_phase, intent_ids, solver_id, input_amount, source_chain, input_denom) = {
        let settlement = match state.settlements.get(settlement_id) {
            Some(s) => s,
            None => return,
        };

        // Get source chain from first intent
        let source_chain = settlement.intent_ids.first()
            .and_then(|id| state.intents.get(id))
            .map(|i| i.input.chain_id.clone())
            .unwrap_or_else(|| "cosmoshub-4".to_string());

        let input_denom = settlement.intent_ids.first()
            .and_then(|id| state.intents.get(id))
            .map(|i| i.input.denom.clone())
            .unwrap_or_else(|| "ATOM".to_string());

        (
            settlement.phase.clone(),
            settlement.intent_ids.clone(),
            settlement.solver_id.clone(),
            settlement.input_amount,
            source_chain,
            input_denom,
        )
    };

    // Determine if this is a cross-chain escrow (via IBC Hooks)
    let is_cross_chain = NON_WASM_CHAINS.contains(&source_chain.as_str());

    let now = Utc::now();

    // Now update the settlement
    let settlement = match state.settlements.get_mut(settlement_id) {
        Some(s) => s,
        None => return,
    };
    settlement.updated_at = now;

    // Progress through settlement phases
    match current_phase {
        SettlementPhase::Init => {
            // Start escrow lock - different flow for cross-chain vs local
            settlement.phase = SettlementPhase::EscrowLocked;
            settlement.status = SettlementStatus::Committing;
            settlement.escrow_txid = Some(format!("tx_{}", Uuid::new_v4()));

            if is_cross_chain {
                // Cross-chain escrow via IBC Hooks (LockFromIbc)
                settlement.events.push(SettlementEvent {
                    event_type: "ibc_escrow_initiated".to_string(),
                    timestamp: now,
                    description: format!(
                        "{} sent from {} to Hub escrow via IBC Hooks (LockFromIbc)",
                        input_denom, source_chain
                    ),
                    metadata: serde_json::json!({
                        "ibc_packet": escrow_ibc_packet,
                        "source_chain": source_chain,
                        "source_channel": format!("channel-{}-hub", source_chain),
                        "amount": settlement.input_amount,
                        "escrow_type": "cross_chain_ibc_hooks",
                    }),
                });
                settlement.events.push(SettlementEvent {
                    event_type: "escrow_locked".to_string(),
                    timestamp: now,
                    description: "User funds locked in Hub escrow contract via IBC Hooks".to_string(),
                    metadata: serde_json::json!({
                        "txid": settlement.escrow_txid,
                        "amount": settlement.input_amount,
                        "escrow_contract": "cosmos1escrow...",
                        "owner_source_address": format!("{}1user...", source_chain.split('-').next().unwrap_or("celestia")),
                        "source_channel": format!("channel-{}-hub", source_chain),
                    }),
                });

                info!(
                    "Settlement {} - cross-chain escrow locked via IBC Hooks from {}",
                    settlement.id, source_chain
                );
            } else {
                // Local escrow (direct Lock call)
                settlement.events.push(SettlementEvent {
                    event_type: "escrow_locked".to_string(),
                    timestamp: now,
                    description: "User funds locked in escrow contract".to_string(),
                    metadata: serde_json::json!({
                        "txid": settlement.escrow_txid,
                        "amount": settlement.input_amount,
                        "escrow_type": "local",
                    }),
                });

                info!(
                    "Settlement {} - local escrow locked",
                    settlement.id
                );
            }
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
            settlement.ibc_packet_id = Some(ibc_packet_id);
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
            if success {
                settlement.phase = SettlementPhase::Finalized;
                settlement.status = SettlementStatus::Completed;
                settlement.completed_at = Some(now);
                settlement.execution_txid = Some(execution_txid);
                settlement.events.push(SettlementEvent {
                    event_type: "completed".to_string(),
                    timestamp: now,
                    description: "Settlement completed successfully".to_string(),
                    metadata: serde_json::json!({
                        "execution_txid": settlement.execution_txid,
                        "output_amount": settlement.output_amount,
                    }),
                });

                info!(
                    "Settlement {} - completed successfully",
                    settlement_id
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

                // Mark for refund - different flow for cross-chain vs local
                settlement.status = SettlementStatus::Refunded;

                if is_cross_chain {
                    // Cross-chain refund via IBC (back to source chain)
                    settlement.events.push(SettlementEvent {
                        event_type: "ibc_refund_initiated".to_string(),
                        timestamp: now,
                        description: format!(
                            "Initiating IBC refund of {} {} back to {}",
                            settlement.input_amount, input_denom, source_chain
                        ),
                        metadata: serde_json::json!({
                            "refund_amount": settlement.input_amount,
                            "refund_denom": input_denom,
                            "destination_chain": source_chain,
                            "destination_channel": format!("channel-hub-{}", source_chain),
                            "refund_type": "ibc_transfer",
                        }),
                    });
                    settlement.events.push(SettlementEvent {
                        event_type: "refunded".to_string(),
                        timestamp: now,
                        description: format!(
                            "User funds refunded via IBC to {} on {}",
                            format!("{}1user...", source_chain.split('-').next().unwrap_or("celestia")),
                            source_chain
                        ),
                        metadata: serde_json::json!({
                            "refund_amount": settlement.input_amount,
                            "refund_type": "cross_chain_ibc",
                        }),
                    });

                    info!(
                        "Settlement {} - failed and refunded via IBC to {}",
                        settlement_id, source_chain
                    );
                } else {
                    // Local refund via bank send
                    settlement.events.push(SettlementEvent {
                        event_type: "refunded".to_string(),
                        timestamp: now,
                        description: "User funds refunded from escrow".to_string(),
                        metadata: serde_json::json!({
                            "refund_amount": settlement.input_amount,
                            "refund_type": "local_bank_send",
                        }),
                    });

                    info!(
                        "Settlement {} - failed and refunded locally",
                        settlement_id
                    );
                }
            }
        }
        SettlementPhase::Finalized => {
            // Already complete, nothing to do
        }
    }

    // Get settlement data for broadcast before dropping the mutable borrow
    let (settlement_clone, phase, status) = {
        let settlement = state.settlements.get(settlement_id).unwrap();
        (settlement.clone(), settlement.phase.clone(), settlement.status.clone())
    };

    // Now update related state based on current phase
    if current_phase == SettlementPhase::IbcInFlight {
        if success {
            // Update intent status
            for intent_id in &intent_ids {
                if let Some(intent) = state.intents.get_mut(intent_id) {
                    intent.status = IntentStatus::Completed;
                }
            }

            // Update solver stats
            if let Some(solver) = state.solvers.get_mut(&solver_id) {
                solver.total_volume += input_amount;
            }

            // Update system stats
            state.stats.pending_intents = state.stats.pending_intents.saturating_sub(
                intent_ids.len() as u64
            );
        } else {
            // Update intent status for failure
            for intent_id in &intent_ids {
                if let Some(intent) = state.intents.get_mut(intent_id) {
                    intent.status = IntentStatus::Failed;
                }
            }
        }

        // Recalculate all stats (including success rate) after settlement completion
        state.update_stats();
    }

    // Broadcast settlement update
    state.broadcast(WsMessage::SettlementUpdate(settlement_clone));

    debug!(
        "Processed settlement {} - phase: {:?}, status: {:?}",
        settlement_id, phase, status
    );
}
