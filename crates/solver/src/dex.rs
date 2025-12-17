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
        }
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

        results.into_iter().filter_map(|r| r.ok()).collect()
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
        let limit_price = intent
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
        let user_min_output = ctx.remaining.u128() as f64 * limit_price.to_string().parse::<f64>().unwrap_or(0.0);
        let surplus = (best.output_amount as f64 - user_min_output).max(0.0);
        let solver_fee = (surplus * self.surplus_capture_rate.to_string().parse::<f64>().unwrap_or(0.1)) as u128;

        let output_to_user = best.output_amount.saturating_sub(solver_fee);
        let effective_price = output_to_user as f64 / ctx.remaining.u128() as f64;

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
    fee_rate: f64,
}

impl MockDexClient {
    pub fn new(venue: impl Into<String>, liquidity: u128, fee_rate: f64) -> Self {
        Self {
            venue: venue.into(),
            liquidity,
            fee_rate,
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
        let output = (amount as f64 * (1.0 - self.fee_rate) * 10.5) as u128; // Mock price ~10.5
        let price_impact = amount as f64 / self.liquidity as f64;

        Ok(DexQuote {
            venue: self.venue.clone(),
            input_amount: amount,
            output_amount: output,
            price_impact: format!("{:.4}", price_impact),
            route: vec![DexSwapStep {
                venue: self.venue.clone(),
                pool_id: "pool-1".to_string(),
                input_denom: input_denom.to_string(),
                output_denom: output_denom.to_string(),
                chain_id: "osmosis-1".to_string(),
            }],
        })
    }

    async fn get_pools(&self, _pair: &TradingPair) -> Result<Vec<crate::PoolInfo>, crate::DexError> {
        Ok(vec![crate::PoolInfo {
            pool_id: "pool-1".to_string(),
            token_a: "uatom".to_string(),
            token_b: "uusdc".to_string(),
            liquidity_a: self.liquidity,
            liquidity_b: self.liquidity * 10,
            fee_rate: format!("{:.4}", self.fee_rate),
        }])
    }
}
