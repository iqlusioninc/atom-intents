use async_trait::async_trait;
use atom_intents_types::{
    DexSwapStep, ExecutionPlan, Intent, ProposedFill, Solution, SolveContext, SolverCapabilities,
    SolverCapacity, TradingPair,
};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

use crate::{DexClient, DexQuote, SolveError, Solver};

/// DEX Routing Solver - Zero capital required
/// Routes trades through existing Cosmos DEXes (Osmosis, Astroport, etc.)
pub struct DexRoutingSolver {
    id: String,
    supported_pairs: Vec<TradingPair>,
    capabilities: SolverCapabilities,
    dex_clients: Vec<Arc<dyn DexClient>>,
    surplus_capture_rate: Decimal, // e.g., 0.10 for 10%
    fee_estimator: crate::FeeEstimator,
}

impl DexRoutingSolver {
    pub fn new(id: impl Into<String>, dex_clients: Vec<Arc<dyn DexClient>>) -> Self {
        Self {
            id: id.into(),
            supported_pairs: vec![
                TradingPair::new("uatom", "uusdc"),
                TradingPair::new("uosmo", "uusdc"),
                TradingPair::new("uatom", "uosmo"),
            ],
            capabilities: SolverCapabilities {
                dex_routing: true,
                intent_matching: false,
                cex_backstop: false,
                cross_ecosystem: false,
                max_fill_size_usd: 500_000,
            },
            dex_clients,
            surplus_capture_rate: Decimal::from_str("0.10").unwrap(),
            fee_estimator: crate::FeeEstimator::new(),
        }
    }

    /// Enrich a quote with fee estimation
    fn enrich_quote_with_fees(&self, mut quote: DexQuote) -> DexQuote {
        // Try to estimate fees based on route
        if let Some(first_step) = quote.route.first() {
            let num_hops = quote.route.len() as u32;
            if let Ok(fee_estimate) = self.fee_estimator.estimate_swap_fee(
                &first_step.chain_id,
                num_hops,
                crate::FeePriority::Medium,
            ) {
                quote.estimated_fee = Some(fee_estimate);
            }
        }
        quote
    }

    async fn query_all_dexes(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Vec<DexQuote> {
        let futures: Vec<_> = self
            .dex_clients
            .iter()
            .map(|client| client.get_quote(input_denom, output_denom, amount))
            .collect();

        let results = futures::future::join_all(futures).await;

        results
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|quote| self.enrich_quote_with_fees(quote))
            .collect()
    }

    fn calculate_bond(&self, fill_amount: Uint128) -> Uint128 {
        // Bond = 1.5x the fill amount value
        fill_amount * Uint128::new(15) / Uint128::new(10)
    }
}

#[async_trait]
impl Solver for DexRoutingSolver {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_pairs(&self) -> &[TradingPair] {
        &self.supported_pairs
    }

    fn capabilities(&self) -> &SolverCapabilities {
        &self.capabilities
    }

    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError> {
        // Check if pair is supported
        let pair = intent.pair();
        if !self.supported_pairs.contains(&pair) {
            return Err(SolveError::NoViableRoute);
        }

        // Query all DEXes concurrently
        let quotes = self
            .query_all_dexes(
                &intent.input.denom,
                &intent.output.denom,
                ctx.remaining.u128(),
            )
            .await;

        if quotes.is_empty() {
            return Err(SolveError::NoViableRoute);
        }

        // Find best quote that meets minimum output
        let limit_price =
            intent
                .output
                .limit_price_decimal()
                .map_err(|e| SolveError::InvalidIntent {
                    reason: format!("invalid limit price: {}", e),
                })?;

        let best = quotes
            .into_iter()
            .filter(|q| Uint128::new(q.output_amount) >= intent.output.min_amount)
            .max_by_key(|q| q.output_amount)
            .ok_or(SolveError::NoViableRoute)?;

        // Calculate fee (10% of surplus over user's limit price)
        let remaining_dec = Decimal::from(ctx.remaining.u128());

        // SECURITY FIX (S-C1): Guard against division by zero when remaining is zero
        if remaining_dec.is_zero() {
            return Err(SolveError::InvalidIntent {
                reason: "zero remaining amount".into(),
            });
        }

        let user_min_output_dec = remaining_dec * limit_price;
        let output_amount_dec = Decimal::from(best.output_amount);
        let surplus_dec = (output_amount_dec - user_min_output_dec).max(Decimal::ZERO);
        let solver_fee_dec = surplus_dec * self.surplus_capture_rate;
        let solver_fee = solver_fee_dec
            .trunc()
            .to_string()
            .parse::<u128>()
            .unwrap_or(0);

        let output_to_user = best.output_amount.saturating_sub(solver_fee);
        let output_to_user_dec = Decimal::from(output_to_user);
        let effective_price = output_to_user_dec / remaining_dec;

        if let Some(max_fee_bps) = intent.constraints.max_solver_fee_bps {
            if !output_amount_dec.is_zero() {
                let fee_bps =
                    (solver_fee_dec / output_amount_dec) * Decimal::from(10_000u32);
                if fee_bps > Decimal::from(max_fee_bps) {
                    return Err(SolveError::NoViableRoute);
                }
            }
        }

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: ctx.remaining,
                output_amount: Uint128::new(output_to_user),
                price: format!("{:.6}", effective_price),
            },
            execution: ExecutionPlan::DexRoute { steps: best.route },
            valid_until: current_time + 5, // 5 second validity
            bond: self.calculate_bond(ctx.remaining),
        })
    }

    async fn capacity(&self, pair: &TradingPair) -> Result<SolverCapacity, SolveError> {
        if !self.supported_pairs.contains(pair) {
            return Err(SolveError::NoViableRoute);
        }

        // Query aggregate liquidity from all DEXes
        let mut total_liquidity: u128 = 0;
        for client in &self.dex_clients {
            if let Ok(pools) = client.get_pools(pair).await {
                for pool in pools {
                    total_liquidity += pool.liquidity_a.min(pool.liquidity_b);
                }
            }
        }

        Ok(SolverCapacity {
            max_immediate: Uint128::new(total_liquidity / 10), // 10% of liquidity
            available_liquidity: Uint128::new(total_liquidity),
            estimated_time_ms: 3000, // ~3 seconds for DEX route
        })
    }
}

/// Mock DEX client for testing
pub struct MockDexClient {
    venue: String,
    liquidity: u128,
    fee_rate: Decimal,
}

impl MockDexClient {
    pub fn new(venue: impl Into<String>, liquidity: u128, fee_rate: f64) -> Self {
        Self {
            venue: venue.into(),
            liquidity,
            fee_rate: Decimal::from_f64_retain(fee_rate).unwrap_or(Decimal::ZERO),
        }
    }
}

#[async_trait]
impl DexClient for MockDexClient {
    async fn get_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Result<DexQuote, crate::DexError> {
        if amount > self.liquidity {
            return Err(crate::DexError::InsufficientLiquidity);
        }

        // Simple constant product AMM simulation
        let amount_dec = Decimal::from(amount);
        let liquidity_dec = Decimal::from(self.liquidity);
        let price = Decimal::from_str("10.5").unwrap(); // Mock price ~10.5
        let one = Decimal::ONE;
        let output_dec = amount_dec * (one - self.fee_rate) * price;
        let output = output_dec.trunc().to_string().parse::<u128>().unwrap_or(0);
        let price_impact_dec = amount_dec / liquidity_dec;
        let price_impact = price_impact_dec.to_string();

        Ok(DexQuote {
            venue: self.venue.clone(),
            input_amount: amount,
            output_amount: output,
            price_impact,
            route: vec![DexSwapStep {
                venue: self.venue.clone(),
                pool_id: "pool-1".to_string(),
                input_denom: input_denom.to_string(),
                output_denom: output_denom.to_string(),
                chain_id: "osmosis-1".to_string(),
            }],
            estimated_fee: None,
        })
    }

    async fn get_pools(
        &self,
        _pair: &TradingPair,
    ) -> Result<Vec<crate::PoolInfo>, crate::DexError> {
        Ok(vec![crate::PoolInfo {
            pool_id: "pool-1".to_string(),
            token_a: "uatom".to_string(),
            token_b: "uusdc".to_string(),
            liquidity_a: self.liquidity,
            liquidity_b: self.liquidity * 10,
            fee_rate: self.fee_rate.to_string(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{
        Asset, ExecutionConstraints, FillConfig, OutputSpec,
    };
    use cosmwasm_std::Binary;

    fn make_intent(
        input_denom: &str,
        output_denom: &str,
        input_amount: u128,
        min_output: u128,
        limit_price: &str,
    ) -> Intent {
        Intent {
            id: "test-1".to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset::new("osmosis-1", input_denom, input_amount),
            output: OutputSpec {
                chain_id: "osmosis-1".to_string(),
                denom: output_denom.to_string(),
                min_amount: Uint128::new(min_output),
                limit_price: limit_price.to_string(),
                recipient: "osmo1recipient".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::from(vec![1, 2, 3]),
            public_key: Binary::from(vec![4, 5, 6]),
            created_at: 1000,
            expires_at: 9999999999,
        }
    }

    fn make_context(remaining: u128) -> SolveContext {
        SolveContext {
            matched_amount: Uint128::zero(),
            remaining: Uint128::new(remaining),
            oracle_price: "10.5".to_string(),
        }
    }

    fn make_solver(liquidity: u128, fee_rate: f64) -> DexRoutingSolver {
        let client = Arc::new(MockDexClient::new("osmosis", liquidity, fee_rate));
        DexRoutingSolver::new("test-solver", vec![client])
    }

    // ═══════════════════════════════════════════════════════════════════
    // SOLVER TRAIT BASICS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_solver_id() {
        let solver = make_solver(10_000_000, 0.003);
        assert_eq!(solver.id(), "test-solver");
    }

    #[test]
    fn test_supported_pairs() {
        let solver = make_solver(10_000_000, 0.003);
        let pairs = solver.supported_pairs();
        assert_eq!(pairs.len(), 3);
        assert!(pairs.contains(&TradingPair::new("uatom", "uusdc")));
        assert!(pairs.contains(&TradingPair::new("uosmo", "uusdc")));
        assert!(pairs.contains(&TradingPair::new("uatom", "uosmo")));
    }

    #[test]
    fn test_capabilities() {
        let solver = make_solver(10_000_000, 0.003);
        let caps = solver.capabilities();
        assert!(caps.dex_routing);
        assert!(!caps.intent_matching);
        assert!(!caps.cex_backstop);
        assert!(!caps.cross_ecosystem);
        assert_eq!(caps.max_fill_size_usd, 500_000);
    }

    // ═══════════════════════════════════════════════════════════════════
    // SOLVE TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_solve_success() {
        let solver = make_solver(10_000_000, 0.003);
        // uatom -> uusdc: supported pair
        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_ok());

        let solution = result.unwrap();
        assert_eq!(solution.solver_id, "test-solver");
        assert_eq!(solution.intent_id, "test-1");
        assert_eq!(solution.fill.input_amount, Uint128::new(1_000_000));
        // Output should be > 0 and <= mock output (1_000_000 * 0.997 * 10.5 = 10_468_500)
        assert!(solution.fill.output_amount > Uint128::zero());
        assert!(solution.fill.output_amount <= Uint128::new(10_468_500));
    }

    #[tokio::test]
    async fn test_solve_unsupported_pair() {
        let solver = make_solver(10_000_000, 0.003);
        let intent = make_intent("ufoo", "ubar", 1_000_000, 100, "1.0");
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SolveError::NoViableRoute));
    }

    #[tokio::test]
    async fn test_solve_insufficient_liquidity_no_quote() {
        // Liquidity of 100 but requesting 1_000_000
        let solver = make_solver(100, 0.003);
        let intent = make_intent("uatom", "uusdc", 1_000_000, 100, "10.0");
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SolveError::NoViableRoute));
    }

    #[tokio::test]
    async fn test_solve_output_below_min_amount() {
        let solver = make_solver(10_000_000, 0.003);
        // Set min_amount very high so the mock quote can't satisfy it
        let intent = make_intent("uatom", "uusdc", 1_000_000, 999_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SolveError::NoViableRoute));
    }

    // ═══════════════════════════════════════════════════════════════════
    // FEE CALCULATION TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_solve_fee_is_ten_pct_of_surplus() {
        let solver = make_solver(10_000_000, 0.003);
        // limit_price = 10.0, so user expects 10_000_000 output for 1_000_000 input
        // Mock gives ~10_468_500, surplus ~468_500, fee ~46_850
        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let solution = solver.solve(&intent, &ctx).await.unwrap();

        // Raw output from DEX: 1_000_000 * 0.997 * 10.5 = 10_468_500
        let raw_output = 10_468_500u128;
        let user_min = 10_000_000u128; // 1_000_000 * 10.0
        let expected_surplus = raw_output - user_min; // 468_500
        let expected_fee = expected_surplus / 10; // 46_850
        let expected_output = raw_output - expected_fee; // 10_421_650

        assert_eq!(solution.fill.output_amount, Uint128::new(expected_output));
    }

    #[tokio::test]
    async fn test_solve_max_solver_fee_bps_rejection() {
        let solver = make_solver(10_000_000, 0.003);
        let mut intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        // Set extremely restrictive fee limit (1 bps = 0.01%)
        intent.constraints.max_solver_fee_bps = Some(1);
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SolveError::NoViableRoute));
    }

    #[tokio::test]
    async fn test_solve_fee_bps_within_limit() {
        let solver = make_solver(10_000_000, 0.003);
        let mut intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        // Fee is ~44.75 bps, set limit to 100 bps
        intent.constraints.max_solver_fee_bps = Some(100);
        let ctx = make_context(1_000_000);

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_ok());
    }

    // ═══════════════════════════════════════════════════════════════════
    // BOND CALCULATION TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_calculate_bond() {
        let solver = make_solver(10_000_000, 0.003);
        let bond = solver.calculate_bond(Uint128::new(1_000_000));
        assert_eq!(bond, Uint128::new(1_500_000)); // 1.5x
    }

    #[test]
    fn test_calculate_bond_rounding() {
        let solver = make_solver(10_000_000, 0.003);
        // 7 * 15 / 10 = 10 (integer division: 105/10 = 10)
        let bond = solver.calculate_bond(Uint128::new(7));
        assert_eq!(bond, Uint128::new(10));
    }

    #[tokio::test]
    async fn test_solve_bond_is_1_5x_input() {
        let solver = make_solver(10_000_000, 0.003);
        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let solution = solver.solve(&intent, &ctx).await.unwrap();
        assert_eq!(solution.bond, Uint128::new(1_500_000));
    }

    // ═══════════════════════════════════════════════════════════════════
    // CAPACITY TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_capacity_supported_pair() {
        let solver = make_solver(10_000_000, 0.003);
        let pair = TradingPair::new("uatom", "uusdc");

        let result = solver.capacity(&pair).await;
        assert!(result.is_ok());

        let capacity = result.unwrap();
        // max_immediate = liquidity / 10 = 1_000_000
        assert_eq!(capacity.max_immediate, Uint128::new(1_000_000));
        // available_liquidity = min(liquidity_a, liquidity_b) = min(10M, 100M) = 10M
        assert_eq!(capacity.available_liquidity, Uint128::new(10_000_000));
        assert_eq!(capacity.estimated_time_ms, 3000);
    }

    #[tokio::test]
    async fn test_capacity_unsupported_pair() {
        let solver = make_solver(10_000_000, 0.003);
        let pair = TradingPair::new("ufoo", "ubar");

        let result = solver.capacity(&pair).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SolveError::NoViableRoute));
    }

    // ═══════════════════════════════════════════════════════════════════
    // MULTI-DEX TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_solve_picks_best_quote_from_multiple_dexes() {
        let client1 = Arc::new(MockDexClient::new("osmosis", 10_000_000, 0.003));
        let client2 = Arc::new(MockDexClient::new("astroport", 10_000_000, 0.001)); // lower fee = better quote
        let solver = DexRoutingSolver::new("multi-solver", vec![client1, client2]);

        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let solution = solver.solve(&intent, &ctx).await.unwrap();

        // Astroport (0.1% fee) gives better output than Osmosis (0.3% fee)
        // Osmosis: 1M * 0.997 * 10.5 = 10,468,500
        // Astroport: 1M * 0.999 * 10.5 = 10,489,500
        // Should pick astroport's higher output
        assert!(solution.fill.output_amount > Uint128::zero());

        match &solution.execution {
            ExecutionPlan::DexRoute { steps } => {
                assert_eq!(steps.len(), 1);
                assert_eq!(steps[0].venue, "astroport");
            }
            _ => panic!("Expected DexRoute execution plan"),
        }
    }

    #[tokio::test]
    async fn test_capacity_aggregates_multiple_dexes() {
        let client1 = Arc::new(MockDexClient::new("osmosis", 5_000_000, 0.003));
        let client2 = Arc::new(MockDexClient::new("astroport", 3_000_000, 0.001));
        let solver = DexRoutingSolver::new("multi-solver", vec![client1, client2]);

        let pair = TradingPair::new("uatom", "uusdc");
        let capacity = solver.capacity(&pair).await.unwrap();

        // Total liquidity from pool min(a, b) for each: 5M + 3M = 8M
        assert_eq!(capacity.available_liquidity, Uint128::new(8_000_000));
        // Max immediate = 10% of total = 800_000
        assert_eq!(capacity.max_immediate, Uint128::new(800_000));
    }

    // ═══════════════════════════════════════════════════════════════════
    // EXECUTION PLAN TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_solve_produces_dex_route_execution_plan() {
        let solver = make_solver(10_000_000, 0.003);
        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let solution = solver.solve(&intent, &ctx).await.unwrap();
        match solution.execution {
            ExecutionPlan::DexRoute { steps } => {
                assert!(!steps.is_empty());
                assert_eq!(steps[0].venue, "osmosis");
                assert_eq!(steps[0].input_denom, "uatom");
                assert_eq!(steps[0].output_denom, "uusdc");
            }
            _ => panic!("Expected DexRoute execution plan"),
        }
    }

    #[tokio::test]
    async fn test_solve_validity_is_short() {
        let solver = make_solver(10_000_000, 0.003);
        let intent = make_intent("uatom", "uusdc", 1_000_000, 9_000_000, "10.0");
        let ctx = make_context(1_000_000);

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let solution = solver.solve(&intent, &ctx).await.unwrap();

        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // valid_until should be ~5 seconds from now
        assert!(solution.valid_until >= before + 5);
        assert!(solution.valid_until <= after + 6);
    }
}
