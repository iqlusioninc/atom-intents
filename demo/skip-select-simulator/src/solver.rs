//! Mock solver implementations for the demo

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tokio::sync::RwLock;
use tracing::debug;

use crate::models::SolverStatus;
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

/// Run mock solver status updates
pub async fn run_mock_solvers(state: AppStateRef) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;
        update_solver_stats(&state).await;
    }
}

async fn update_solver_stats(state: &AppStateRef) {
    let mut rng = rand::thread_rng();
    let mut state = state.write().await;

    for solver in state.solvers.values_mut() {
        // Simulate occasional status changes
        if rng.gen_bool(0.05) {
            // 5% chance of status change
            solver.status = match rng.gen_range(0..10) {
                0 => SolverStatus::Idle,
                1 => SolverStatus::Suspended,
                _ => SolverStatus::Active,
            };
        }

        // Update performance metrics
        solver.total_volume += rng.gen_range(0..1_000_000_000);
        solver.success_rate = (solver.success_rate * 0.99 + rng.gen_range(0.95..1.0) * 0.01)
            .clamp(0.90, 1.0);
        solver.avg_execution_time_ms =
            ((solver.avg_execution_time_ms as f64 * 0.9 + rng.gen_range(1000.0..5000.0) * 0.1)
                as u64)
                .clamp(1000, 10000);

        // Update reputation based on performance
        solver.reputation_score = (solver.reputation_score * 0.95
            + solver.success_rate * 0.03
            + rng.gen_range(0.0..0.02))
        .clamp(0.5, 1.0);
    }

    debug!("Updated solver stats");
}

/// Simulate a solver quote for testing
pub fn generate_test_quote() -> crate::models::SolverQuote {
    use chrono::Utc;
    use uuid::Uuid;

    crate::models::SolverQuote {
        id: format!("quote_{}", Uuid::new_v4()),
        solver_id: "solver_test".to_string(),
        solver_name: "Test Solver".to_string(),
        solver_type: crate::models::SolverType::DexRouter,
        intent_ids: vec!["intent_test".to_string()],
        input_amount: 10_000_000,
        output_amount: 14_000_000,
        effective_price: 1.4,
        execution_plan: crate::models::ExecutionPlan {
            plan_type: crate::models::ExecutionPlanType::DexRoute,
            steps: vec![],
            estimated_duration_ms: 2000,
        },
        estimated_gas: 250_000,
        confidence: 0.95,
        submitted_at: Utc::now(),
    }
}
