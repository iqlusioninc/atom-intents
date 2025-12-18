use atom_intents_types::{Intent, OptimalFillPlan, Solution, SolveContext, TradingPair};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::{OracleError, PriceOracle, SolveError, Solver};

/// Oracle price requirement configuration
#[derive(Debug, Clone)]
pub enum OraclePriceRequirement {
    /// Fail if no oracle price is available (production mode)
    Required,
    /// Use fallback price only for testing (should never be used in production)
    Optional(Decimal),
    /// Use cached price if fresh enough, otherwise query oracle
    Cached(Duration),
}

/// Cached price data
#[derive(Debug, Clone)]
struct CachedPrice {
    price: Decimal,
    pair: TradingPair,
    fetched_at: Instant,
}

/// Aggregates solutions from multiple solvers to find optimal fill
pub struct SolutionAggregator {
    solvers: Vec<Arc<dyn Solver>>,
    oracle: Arc<dyn PriceOracle>,
    price_requirement: OraclePriceRequirement,
    price_cache: Arc<RwLock<HashMap<String, CachedPrice>>>,
    last_oracle_success: Arc<RwLock<Option<Instant>>>,
}

impl SolutionAggregator {
    pub fn new(solvers: Vec<Arc<dyn Solver>>, oracle: Arc<dyn PriceOracle>) -> Self {
        Self {
            solvers,
            oracle,
            price_requirement: OraclePriceRequirement::Required,
            price_cache: Arc::new(RwLock::new(HashMap::new())),
            last_oracle_success: Arc::new(RwLock::new(None)),
        }
    }

    /// Create aggregator with specific price requirement
    pub fn with_price_requirement(
        solvers: Vec<Arc<dyn Solver>>,
        oracle: Arc<dyn PriceOracle>,
        price_requirement: OraclePriceRequirement,
    ) -> Self {
        Self {
            solvers,
            oracle,
            price_requirement,
            price_cache: Arc::new(RwLock::new(HashMap::new())),
            last_oracle_success: Arc::new(RwLock::new(None)),
        }
    }

    pub fn add_solver(&mut self, solver: Arc<dyn Solver>) {
        self.solvers.push(solver);
    }

    pub fn solvers(&self) -> &[Arc<dyn Solver>] {
        &self.solvers
    }

    pub fn oracle(&self) -> &Arc<dyn PriceOracle> {
        &self.oracle
    }

    /// Get oracle price for a trading pair with caching and fallback handling
    pub async fn get_oracle_price(&self, pair: &TradingPair) -> Result<Decimal, OracleError> {
        let pair_symbol = pair.to_symbol();

        // Check if we should use cached price
        if let OraclePriceRequirement::Cached(ttl) = &self.price_requirement {
            let cache = self.price_cache.read().await;
            if let Some(cached) = cache.get(&pair_symbol) {
                if cached.fetched_at.elapsed() < *ttl {
                    return Ok(cached.price);
                }
            }
        }

        // Attempt to fetch from oracle
        let oracle_result = self.oracle.get_price(pair).await;

        match oracle_result {
            Ok(oracle_price) => {
                let price = oracle_price.price;

                // Update last success timestamp
                *self.last_oracle_success.write().await = Some(Instant::now());

                // Update cache
                self.price_cache.write().await.insert(
                    pair_symbol,
                    CachedPrice {
                        price,
                        pair: pair.clone(),
                        fetched_at: Instant::now(),
                    },
                );

                Ok(price)
            }
            Err(e) => {
                // Handle failure based on price requirement
                match &self.price_requirement {
                    OraclePriceRequirement::Required => {
                        // In production mode, propagate the error
                        Err(e)
                    }
                    OraclePriceRequirement::Optional(fallback) => {
                        // Only for testing: use fallback price
                        Ok(*fallback)
                    }
                    OraclePriceRequirement::Cached(ttl) => {
                        // Try to use cached price even if stale
                        let cache = self.price_cache.read().await;
                        if let Some(cached) = cache.get(&pair_symbol) {
                            // Only use stale cache if it's within reasonable bounds (e.g., 10x TTL)
                            if cached.fetched_at.elapsed() < *ttl * 10 {
                                return Ok(cached.price);
                            }
                        }
                        // No cache available, propagate error
                        Err(e)
                    }
                }
            }
        }
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
        let oracle_price = self
            .get_oracle_price(&pair)
            .await
            .map_err(|e| SolveError::Internal(format!("Oracle price fetch failed: {}", e)))?;

        let ctx = SolveContext {
            matched_amount,
            remaining,
            oracle_price: oracle_price.to_string(),
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
            let price_a = Decimal::from_str(&a.fill.price).unwrap_or(Decimal::ZERO);
            let price_b = Decimal::from_str(&b.fill.price).unwrap_or(Decimal::ZERO);
            price_b.cmp(&price_a)
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

    /// Check if oracle is healthy (has returned a valid price recently)
    pub async fn oracle_healthy(&self) -> bool {
        // First check if oracle itself is healthy
        if !self.oracle.health_check().await {
            return false;
        }

        // Check if we've had a successful price fetch recently (within last 5 minutes)
        let last_success = self.last_oracle_success.read().await;
        match *last_success {
            Some(instant) => instant.elapsed() < Duration::from_secs(300),
            None => {
                // Never had a successful fetch, try a quick health check
                // by checking if oracle supports at least one common pair
                let test_pair = TradingPair::new("ATOM", "USDC");
                self.oracle.supports_pair(&test_pair)
            }
        }
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
    use crate::{DexRoutingSolver, MockDexClient, MockOracle};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_aggregator_health_check() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let aggregator = SolutionAggregator::new(vec![solver], oracle);
        let health = aggregator.health_check().await;

        assert_eq!(health.len(), 1);
        assert!(health[0].1);
    }

    #[tokio::test]
    async fn test_aggregator_with_oracle_price() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregator = SolutionAggregator::new(vec![solver], oracle);

        let price = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(price, Decimal::from_str("12.34").unwrap());
    }

    #[tokio::test]
    async fn test_aggregator_oracle_failure_required_mode() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        // Don't set price - oracle will fail

        let aggregator = SolutionAggregator::new(vec![solver], oracle);

        // Should fail because we're in Required mode and oracle has no price
        let result = aggregator.get_oracle_price(&pair).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_aggregator_oracle_failure_optional_mode() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        // Don't set price - oracle will fail

        let fallback = Decimal::from_str("10.45").unwrap();
        let aggregator = SolutionAggregator::with_price_requirement(
            vec![solver],
            oracle,
            OraclePriceRequirement::Optional(fallback),
        );

        // Should succeed with fallback price
        let result = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(result, fallback);
    }

    #[tokio::test]
    async fn test_aggregator_price_caching() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregator = SolutionAggregator::with_price_requirement(
            vec![solver],
            oracle.clone(),
            OraclePriceRequirement::Cached(Duration::from_secs(10)),
        );

        // First call should fetch from oracle
        let price1 = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(price1, Decimal::from_str("12.34").unwrap());

        // Update oracle price
        oracle
            .set_price(
                &pair,
                Decimal::from_str("15.00").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        // Second call should use cache (still old price)
        let price2 = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(price2, Decimal::from_str("12.34").unwrap());
    }

    #[tokio::test]
    async fn test_aggregator_cache_expiration() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        // Very short cache TTL for testing
        let aggregator = SolutionAggregator::with_price_requirement(
            vec![solver],
            oracle.clone(),
            OraclePriceRequirement::Cached(Duration::from_millis(50)),
        );

        // First call
        let price1 = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(price1, Decimal::from_str("12.34").unwrap());

        // Wait for cache to expire
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Update oracle price
        oracle
            .set_price(
                &pair,
                Decimal::from_str("15.00").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        // Should fetch new price from oracle
        let price2 = aggregator.get_oracle_price(&pair).await.unwrap();
        assert_eq!(price2, Decimal::from_str("15.00").unwrap());
    }

    #[tokio::test]
    async fn test_oracle_healthy_check() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregator = SolutionAggregator::new(vec![solver], oracle);

        // Before any price fetch, should be healthy if oracle supports the pair
        let healthy = aggregator.oracle_healthy().await;
        assert!(healthy);

        // After successful fetch, should be healthy
        let _ = aggregator.get_oracle_price(&pair).await.unwrap();
        let healthy = aggregator.oracle_healthy().await;
        assert!(healthy);
    }

    #[tokio::test]
    async fn test_oracle_healthy_check_after_failure() {
        let mock_client = Arc::new(MockDexClient::new("osmosis", 1_000_000_000_000, 0.003));
        let solver = Arc::new(DexRoutingSolver::new("dex-solver-1", vec![mock_client]));

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("ATOM", "USDC");
        // Set price first
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregator = SolutionAggregator::new(vec![solver], oracle.clone());

        // First successful fetch
        let _ = aggregator.get_oracle_price(&pair).await.unwrap();
        assert!(aggregator.oracle_healthy().await);

        // Now remove the price to simulate oracle failure
        oracle.clear_prices().await;

        // Try to fetch - will fail
        let fallback_aggregator = SolutionAggregator::with_price_requirement(
            vec![],
            oracle,
            OraclePriceRequirement::Optional(Decimal::from_str("10.00").unwrap()),
        );

        let _ = fallback_aggregator.get_oracle_price(&pair).await;

        // Health check should still show healthy for a while due to last success time
        // (This test would need time manipulation for proper testing of staleness)
    }
}
