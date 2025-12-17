use atom_intents_types::{Intent, OptimalFillPlan, Solution, SolveContext, TradingPair};
use cosmwasm_std::Uint128;
use std::sync::Arc;

use crate::{SolveError, Solver};

/// Aggregates solutions from multiple solvers to find optimal fill
pub struct SolutionAggregator {
    solvers: Vec<Arc<dyn Solver>>,
}

impl SolutionAggregator {
    pub fn new(solvers: Vec<Arc<dyn Solver>>) -> Self {
        Self { solvers }
    }

    pub fn add_solver(&mut self, solver: Arc<dyn Solver>) {
        self.solvers.push(solver);
    }

    pub fn solvers(&self) -> &[Arc<dyn Solver>] {
        &self.solvers
    }

    /// Get oracle price for a trading pair (mock implementation)
    pub async fn get_oracle_price(&self, _pair: &TradingPair) -> String {
        // In production, this would query price oracles
        "10.45".to_string()
    }

    /// Aggregate solutions for an intent
    pub async fn aggregate(
        &self,
        intent: &Intent,
        matched_amount: Uint128,
    ) -> Result<OptimalFillPlan, SolveError> {
        let remaining = intent
            .input
            .amount
            .checked_sub(matched_amount)
            .unwrap_or(Uint128::zero());

        if remaining.is_zero() {
            return Ok(OptimalFillPlan::fully_matched(&intent.id, matched_amount));
        }

        let pair = intent.pair();
        let oracle_price = self.get_oracle_price(&pair).await;

        let ctx = SolveContext {
            matched_amount,
            remaining,
            oracle_price,
        };

        // Collect solutions from all solvers concurrently
        let solve_futures: Vec<_> = self
            .solvers
            .iter()
            .filter(|s| s.supported_pairs().contains(&pair))
            .map(|s| s.solve(intent, &ctx))
            .collect();

        let results = futures::future::join_all(solve_futures).await;

        let mut solutions: Vec<Solution> = results.into_iter().filter_map(|r| r.ok()).collect();

        if solutions.is_empty() {
            return Err(SolveError::NoViableRoute);
        }

        // Sort by price (best prices first - higher output per input)
        solutions.sort_by(|a, b| {
            let price_a: f64 = a.fill.price.parse().unwrap_or(0.0);
            let price_b: f64 = b.fill.price.parse().unwrap_or(0.0);
            price_b.partial_cmp(&price_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Greedy selection: take best prices until filled
        let mut selected = Vec::new();
        let mut total_input = Uint128::zero();

        for solution in solutions {
            if total_input >= remaining {
                break;
            }

            let take_amount = std::cmp::min(
                remaining - total_input,
                solution.fill.input_amount,
            );

            selected.push((solution, take_amount));
            total_input += take_amount;
        }

        Ok(OptimalFillPlan {
            selected,
            total_input,
        })
    }

    /// Check health of all solvers
    pub async fn health_check(&self) -> Vec<(String, bool)> {
        let futures: Vec<_> = self
            .solvers
            .iter()
            .map(|s| async move { (s.id().to_string(), s.health_check().await) })
            .collect();

        futures::future::join_all(futures).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DexRoutingSolver, MockDexClient};

    #[tokio::test]
    async fn test_aggregator_health_check() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let aggregator = SolutionAggregator::new(vec![solver]);
        let health = aggregator.health_check().await;

        assert_eq!(health.len(), 1);
        assert!(health[0].1);
    }
}
