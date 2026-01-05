use async_trait::async_trait;
use atom_intents_types::{
    AssetPreference, EthereumEscrowedIntent, EurekaDirection, ExecutionPlan,
    FrontingRiskAssessment, Intent, ProposedFill, SettlementRiskPricing, Solution,
    SolutionMetadata, SolveContext, SolverCapabilities, SolverCapacity, TradingPair,
};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::sync::Arc;

use crate::{SkipGoClient, SolveError, Solver};

/// Eureka Solver - Sources liquidity from Ethereum via IBC Eureka
///
/// This solver enables cross-ecosystem trading by bridging assets from Ethereum
/// to Cosmos using the IBC Eureka protocol. It integrates with Skip Go for
/// routing and quote generation.
pub struct EurekaSolver {
    id: String,
    skip_go: Arc<SkipGoClient>,
    supported_pairs: Vec<TradingPair>,
    capabilities: SolverCapabilities,
    /// Risk pricing configuration for fronting
    risk_pricing: SettlementRiskPricing,
    /// Whether this solver is willing to front settlements
    enable_fronting: bool,
}

impl EurekaSolver {
    /// Create a new EurekaSolver instance
    pub fn new(id: impl Into<String>, skip_go: Arc<SkipGoClient>) -> Self {
        Self {
            id: id.into(),
            skip_go,
            supported_pairs: vec![
                // Ethereum USDC to Cosmos USDC
                TradingPair::new(
                    "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", // Ethereum USDC
                    "uusdc",
                ),
                // Ethereum USDT to Cosmos USDT
                TradingPair::new(
                    "0xdAC17F958D2ee523a2206206994597C13D831ec7", // Ethereum USDT
                    "uusdt",
                ),
                // Cosmos USDC to Ethereum USDC
                TradingPair::new("uusdc", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
                // ATOM to Ethereum-bridged representations
                TradingPair::new("uatom", "eureka/atom"),
            ],
            capabilities: SolverCapabilities {
                dex_routing: false,
                intent_matching: false,
                cex_backstop: false,
                cross_ecosystem: true,
                max_fill_size_usd: 1_000_000, // Large cap for Ethereum liquidity
            },
            risk_pricing: SettlementRiskPricing::default_eureka(),
            enable_fronting: false, // Disabled by default
        }
    }

    /// Check if the user's asset preference allows bridged asset delivery
    pub fn can_deliver_bridged(constraints: &atom_intents_types::ExecutionConstraints) -> bool {
        match &constraints.asset_preference {
            AssetPreference::NativeOnly => false,
            AssetPreference::AcceptBridged { .. } => true,
            AssetPreference::AnyEquivalent => true,
        }
    }

    /// Check if the settlement time meets the user's constraints
    pub fn is_time_acceptable(constraints: &atom_intents_types::ExecutionConstraints) -> bool {
        // Eureka takes ~25 seconds for settlement
        const EUREKA_SETTLEMENT_TIME_SECS: u64 = 25;

        match constraints.max_settlement_secs {
            Some(max_secs) => max_secs >= EUREKA_SETTLEMENT_TIME_SECS,
            None => true, // No constraint means acceptable
        }
    }

    /// Determine Eureka direction based on denoms
    fn infer_direction(input_denom: &str, output_denom: &str) -> EurekaDirection {
        // Ethereum addresses start with "0x"
        if input_denom.starts_with("0x") {
            EurekaDirection::EthereumToCosmos
        } else if output_denom.starts_with("0x") {
            EurekaDirection::CosmosToEthereum
        } else {
            // Default to Ethereum->Cosmos for bridged denoms
            EurekaDirection::EthereumToCosmos
        }
    }

    /// Infer source chain ID from denom
    fn infer_source_chain(input_denom: &str) -> &str {
        if input_denom.starts_with("0x") {
            "1" // Ethereum mainnet
        } else if input_denom == "uatom" {
            "cosmoshub-4"
        } else if input_denom == "uusdc" {
            "noble-1"
        } else if input_denom == "uosmo" {
            "osmosis-1"
        } else {
            "cosmoshub-4" // Default
        }
    }

    /// Infer destination chain ID from denom
    fn infer_dest_chain(output_denom: &str) -> &str {
        if output_denom.starts_with("0x") {
            "1" // Ethereum mainnet
        } else if output_denom == "uatom" {
            "cosmoshub-4"
        } else if output_denom == "uusdc" {
            "noble-1"
        } else {
            // Default to cosmoshub-4 for eureka-bridged assets and unknown denoms
            "cosmoshub-4"
        }
    }

    /// Calculate bond requirement for Eureka fills
    fn calculate_bond(&self, fill_amount: Uint128) -> Uint128 {
        // Bond = 2x the fill amount (higher for cross-ecosystem)
        fill_amount * Uint128::new(2)
    }

    /// Check if this solver can front a settlement
    pub fn can_front_settlement(&self, escrowed_intent: &EthereumEscrowedIntent) -> bool {
        self.enable_fronting && escrowed_intent.is_ready_for_fronting()
    }

    /// Calculate fronting bond requirement
    pub fn calculate_fronting_bond(&self, amount: Uint128) -> Uint128 {
        self.risk_pricing.required_bond(amount)
    }

    /// Assess whether to front a specific settlement
    pub fn assess_fronting(
        &self,
        fronted_amount: Uint128,
        expected_output: Uint128,
    ) -> FrontingRiskAssessment {
        FrontingRiskAssessment::assess(&self.risk_pricing, fronted_amount, expected_output)
    }

    /// Get risk-adjusted quote for fronted settlement
    pub fn get_fronting_quote(&self, base_output: Uint128) -> Uint128 {
        self.risk_pricing.adjust_quote(base_output)
    }

    /// Create a new solver with fronting enabled
    pub fn with_fronting(mut self, enable: bool) -> Self {
        self.enable_fronting = enable;
        self
    }

    /// Set custom risk pricing
    pub fn with_risk_pricing(mut self, pricing: SettlementRiskPricing) -> Self {
        self.risk_pricing = pricing;
        self
    }
}

#[async_trait]
impl Solver for EurekaSolver {
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
        // Check if cross-ecosystem is allowed
        if !intent.constraints.allow_cross_ecosystem {
            return Err(SolveError::InvalidIntent {
                reason: "cross-ecosystem execution not allowed".to_string(),
            });
        }

        // Check asset preference
        if !Self::can_deliver_bridged(&intent.constraints) {
            return Err(SolveError::InvalidIntent {
                reason: "user requires native-only assets, cannot deliver bridged".to_string(),
            });
        }

        // Check time constraints
        if !Self::is_time_acceptable(&intent.constraints) {
            return Err(SolveError::InvalidIntent {
                reason: "settlement time exceeds user's max_settlement_secs".to_string(),
            });
        }

        // Determine source and destination chains
        let source_chain = Self::infer_source_chain(&intent.input.denom);
        let dest_chain = Self::infer_dest_chain(&intent.output.denom);

        // Get Eureka quote from Skip Go
        let quote = self
            .skip_go
            .get_eureka_quote(
                &intent.input.denom,
                &intent.output.denom,
                ctx.remaining.u128(),
                source_chain,
                dest_chain,
            )
            .await
            .map_err(|e| SolveError::DexQueryFailed(format!("Eureka quote failed: {}", e)))?;

        // Check if output meets minimum requirement
        if Uint128::new(quote.output_amount) < intent.output.min_amount {
            return Err(SolveError::InsufficientLiquidity {
                needed: intent.output.min_amount.to_string(),
                available: quote.output_amount.to_string(),
            });
        }

        // Calculate price
        let remaining_dec = Decimal::from(ctx.remaining.u128());
        let output_dec = Decimal::from(quote.output_amount);
        let effective_price = output_dec / remaining_dec;

        // Determine direction and create metadata
        let direction = Self::infer_direction(&intent.input.denom, &intent.output.denom);
        let is_native = matches!(direction, EurekaDirection::EthereumToCosmos);

        let metadata = SolutionMetadata::eureka(
            direction,
            None, // eth_address filled by orchestrator
            &intent.output.denom,
            is_native,
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: ctx.remaining,
                output_amount: Uint128::new(quote.output_amount),
                price: format!("{:.6}", effective_price),
            },
            execution: ExecutionPlan::CrossEcosystem {
                bridge: "eureka".to_string(),
                target: dest_chain.to_string(),
            },
            valid_until: current_time + 10, // 10 second validity for cross-ecosystem
            bond: self.calculate_bond(ctx.remaining),
            metadata: Some(metadata),
        })
    }

    async fn capacity(&self, pair: &TradingPair) -> Result<SolverCapacity, SolveError> {
        if !self.supported_pairs.contains(pair) {
            return Err(SolveError::NoViableRoute);
        }

        // Ethereum has deep liquidity - return large capacity
        // In production, this would query actual bridge limits and liquidity
        Ok(SolverCapacity {
            max_immediate: Uint128::new(10_000_000_000_000), // 10M USDC (6 decimals)
            available_liquidity: Uint128::new(100_000_000_000_000), // 100M USDC
            estimated_time_ms: 25_000,                       // 25 seconds for Eureka
        })
    }

    async fn health_check(&self) -> bool {
        // For now, always return true
        // In production, would check Skip Go API health and Eureka bridge status
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, EscrowStatus, EthereumEscrowedIntent, ExecutionConstraints, FillConfig, OutputSpec};
    use cosmwasm_std::Binary;

    fn create_test_intent_with_constraints(constraints: ExecutionConstraints) -> Intent {
        Intent {
            id: "test-intent-eureka".to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1test".to_string(),
            input: Asset {
                chain_id: "1".to_string(), // Ethereum
                denom: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
                amount: Uint128::new(1_000_000_000), // 1000 USDC
            },
            output: OutputSpec {
                chain_id: "noble-1".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(990_000_000), // 990 USDC min
                limit_price: "0.99".to_string(),
                recipient: "cosmos1recipient".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints,
            signature: Binary::default(),
            public_key: Binary::default(),
            created_at: 0,
            expires_at: 3600,
        }
    }

    #[test]
    fn test_eureka_solver_id() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver-1", skip_go);

        assert_eq!(solver.id(), "eureka-solver-1");
    }

    #[test]
    fn test_eureka_solver_capabilities() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        let caps = solver.capabilities();
        assert!(caps.cross_ecosystem);
        assert!(!caps.dex_routing);
        assert!(!caps.cex_backstop);
        assert!(!caps.intent_matching);
        assert_eq!(caps.max_fill_size_usd, 1_000_000);
    }

    #[test]
    fn test_eureka_solver_can_deliver_native() {
        // NativeOnly should reject bridged delivery
        let native_only_constraints = ExecutionConstraints::default();
        assert!(!EurekaSolver::can_deliver_bridged(&native_only_constraints));

        // AcceptBridged should allow
        let bridged_constraints =
            ExecutionConstraints::new(1000).with_asset_preference(AssetPreference::AcceptBridged {
                allowed_denoms: vec!["eureka/usdc".to_string()],
            });
        assert!(EurekaSolver::can_deliver_bridged(&bridged_constraints));

        // AnyEquivalent should allow
        let any_constraints =
            ExecutionConstraints::new(1000).with_asset_preference(AssetPreference::AnyEquivalent);
        assert!(EurekaSolver::can_deliver_bridged(&any_constraints));
    }

    #[test]
    fn test_time_constraint_check() {
        // No constraint - should be acceptable
        let no_constraint = ExecutionConstraints::default();
        assert!(EurekaSolver::is_time_acceptable(&no_constraint));

        // 30 seconds - should be acceptable (>= 25)
        let thirty_secs = ExecutionConstraints::new(1000).with_max_settlement_secs(30);
        assert!(EurekaSolver::is_time_acceptable(&thirty_secs));

        // 25 seconds - should be acceptable (== 25)
        let twenty_five = ExecutionConstraints::new(1000).with_max_settlement_secs(25);
        assert!(EurekaSolver::is_time_acceptable(&twenty_five));

        // 20 seconds - should NOT be acceptable (< 25)
        let twenty_secs = ExecutionConstraints::new(1000).with_max_settlement_secs(20);
        assert!(!EurekaSolver::is_time_acceptable(&twenty_secs));

        // 10 seconds - should NOT be acceptable (< 25)
        let ten_secs = ExecutionConstraints::new(1000).with_max_settlement_secs(10);
        assert!(!EurekaSolver::is_time_acceptable(&ten_secs));
    }

    #[test]
    fn test_infer_direction() {
        // Ethereum address as input -> EthereumToCosmos
        let direction =
            EurekaSolver::infer_direction("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "uusdc");
        assert!(matches!(direction, EurekaDirection::EthereumToCosmos));

        // Cosmos denom as input, Ethereum as output -> CosmosToEthereum
        let direction =
            EurekaSolver::infer_direction("uusdc", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        assert!(matches!(direction, EurekaDirection::CosmosToEthereum));

        // Both Cosmos denoms -> defaults to EthereumToCosmos
        let direction = EurekaSolver::infer_direction("uatom", "eureka/atom");
        assert!(matches!(direction, EurekaDirection::EthereumToCosmos));
    }

    #[test]
    fn test_infer_source_chain() {
        assert_eq!(
            EurekaSolver::infer_source_chain("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            "1"
        );
        assert_eq!(EurekaSolver::infer_source_chain("uatom"), "cosmoshub-4");
        assert_eq!(EurekaSolver::infer_source_chain("uusdc"), "noble-1");
        assert_eq!(EurekaSolver::infer_source_chain("uosmo"), "osmosis-1");
    }

    #[test]
    fn test_infer_dest_chain() {
        assert_eq!(
            EurekaSolver::infer_dest_chain("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            "1"
        );
        assert_eq!(EurekaSolver::infer_dest_chain("uatom"), "cosmoshub-4");
        assert_eq!(EurekaSolver::infer_dest_chain("uusdc"), "noble-1");
        assert_eq!(EurekaSolver::infer_dest_chain("eureka/atom"), "cosmoshub-4");
    }

    #[tokio::test]
    async fn test_eureka_solver_rejects_native_only() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        // Create intent with NativeOnly preference but cross-ecosystem allowed
        let constraints = ExecutionConstraints::new(1000)
            .with_cross_ecosystem(true)
            .with_asset_preference(AssetPreference::NativeOnly);

        let intent = create_test_intent_with_constraints(constraints);
        let ctx = SolveContext {
            matched_amount: Uint128::zero(),
            remaining: Uint128::new(1_000_000_000),
            oracle_price: "1.0".to_string(),
        };

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());

        if let Err(SolveError::InvalidIntent { reason }) = result {
            assert!(reason.contains("native-only"));
        } else {
            panic!("Expected InvalidIntent error for native-only preference");
        }
    }

    #[tokio::test]
    async fn test_eureka_solver_rejects_cross_ecosystem_disabled() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        // Create intent with cross_ecosystem disabled (default)
        let constraints =
            ExecutionConstraints::new(1000).with_asset_preference(AssetPreference::AnyEquivalent);

        let intent = create_test_intent_with_constraints(constraints);
        let ctx = SolveContext {
            matched_amount: Uint128::zero(),
            remaining: Uint128::new(1_000_000_000),
            oracle_price: "1.0".to_string(),
        };

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());

        if let Err(SolveError::InvalidIntent { reason }) = result {
            assert!(reason.contains("cross-ecosystem"));
        } else {
            panic!("Expected InvalidIntent error for cross-ecosystem disabled");
        }
    }

    #[tokio::test]
    async fn test_eureka_solver_rejects_time_constraint() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        // Create intent with time constraint less than 25 seconds
        let constraints = ExecutionConstraints::new(1000)
            .with_cross_ecosystem(true)
            .with_asset_preference(AssetPreference::AnyEquivalent)
            .with_max_settlement_secs(10); // Too short for Eureka

        let intent = create_test_intent_with_constraints(constraints);
        let ctx = SolveContext {
            matched_amount: Uint128::zero(),
            remaining: Uint128::new(1_000_000_000),
            oracle_price: "1.0".to_string(),
        };

        let result = solver.solve(&intent, &ctx).await;
        assert!(result.is_err());

        if let Err(SolveError::InvalidIntent { reason }) = result {
            assert!(reason.contains("max_settlement_secs"));
        } else {
            panic!("Expected InvalidIntent error for time constraint");
        }
    }

    #[tokio::test]
    async fn test_eureka_solver_capacity() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        let pair = TradingPair::new("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "uusdc");

        let capacity = solver.capacity(&pair).await.unwrap();

        // Should have large capacity for Ethereum liquidity
        assert!(capacity.available_liquidity.u128() > 0);
        assert!(capacity.max_immediate.u128() > 0);
        assert_eq!(capacity.estimated_time_ms, 25_000);
    }

    #[tokio::test]
    async fn test_eureka_solver_capacity_unsupported_pair() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        let unsupported_pair = TradingPair::new("uatom", "uosmo"); // Not an Eureka pair

        let result = solver.capacity(&unsupported_pair).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SolveError::NoViableRoute)));
    }

    #[tokio::test]
    async fn test_eureka_solver_health_check() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        assert!(solver.health_check().await);
    }

    #[test]
    fn test_supported_pairs() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("eureka-solver", skip_go);

        let pairs = solver.supported_pairs();
        assert!(!pairs.is_empty());

        // Should have Ethereum USDC to Cosmos USDC
        let eth_usdc_pair = TradingPair::new("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "uusdc");
        assert!(pairs.contains(&eth_usdc_pair));
    }

    // ==================== Fronting Tests ====================

    fn create_mock_escrowed_intent(escrow_status: EscrowStatus) -> EthereumEscrowedIntent {
        let intent = create_test_intent_with_constraints(
            ExecutionConstraints::new(9999999999)
                .with_cross_ecosystem(true)
                .with_asset_preference(AssetPreference::AnyEquivalent)
        );
        let mut escrowed = EthereumEscrowedIntent::new(
            intent,
            "0x1234567890abcdef".to_string(),
            300,
        );
        escrowed.escrow_status = escrow_status;
        escrowed
    }

    #[test]
    fn test_fronting_disabled_by_default() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go);
        assert!(!solver.enable_fronting);
    }

    #[test]
    fn test_enable_fronting() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go)
            .with_fronting(true);
        assert!(solver.enable_fronting);
    }

    #[test]
    fn test_calculate_fronting_bond() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go);
        let bond = solver.calculate_fronting_bond(Uint128::new(1_000_000));
        assert_eq!(bond, Uint128::new(2_000_000)); // 2x default multiplier
    }

    #[test]
    fn test_fronting_quote_adjustment() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go);
        let adjusted = solver.get_fronting_quote(Uint128::new(1_000_000));
        // Default 50 bps premium = 0.5% = 5000 deducted
        assert_eq!(adjusted, Uint128::new(995_000));
    }

    #[test]
    fn test_assess_fronting_positive() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go);
        let assessment = solver.assess_fronting(
            Uint128::new(1_000_000),
            Uint128::new(1_100_000), // 10% profit
        );
        assert!(assessment.should_front);
    }

    #[test]
    fn test_can_front_settlement_when_enabled_and_ready() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go)
            .with_fronting(true);

        // Create an escrowed intent in "Received" status (ready for fronting)
        let escrowed = create_mock_escrowed_intent(EscrowStatus::Received {
            packet_id: "pkt-123".to_string(),
            received_at: 1704067200,
        });

        assert!(solver.can_front_settlement(&escrowed));
    }

    #[test]
    fn test_cannot_front_when_disabled() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go); // fronting disabled by default

        let escrowed = create_mock_escrowed_intent(EscrowStatus::Received {
            packet_id: "pkt-123".to_string(),
            received_at: 1704067200,
        });

        assert!(!solver.can_front_settlement(&escrowed));
    }

    #[test]
    fn test_cannot_front_when_pending() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go)
            .with_fronting(true);

        let escrowed = create_mock_escrowed_intent(EscrowStatus::Pending);

        assert!(!solver.can_front_settlement(&escrowed));
    }

    #[test]
    fn test_cannot_front_when_finalized() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let solver = EurekaSolver::new("test", skip_go)
            .with_fronting(true);

        let escrowed = create_mock_escrowed_intent(EscrowStatus::Finalized {
            packet_id: "pkt-123".to_string(),
            finalized_at: 1704067230,
        });

        // When finalized, no need to front - just settle normally
        assert!(!solver.can_front_settlement(&escrowed));
    }

    #[test]
    fn test_with_risk_pricing() {
        let skip_go = Arc::new(SkipGoClient::mainnet());
        let custom_pricing = SettlementRiskPricing::conservative();
        let solver = EurekaSolver::new("test", skip_go)
            .with_risk_pricing(custom_pricing);

        // Conservative pricing uses 3x multiplier
        let bond = solver.calculate_fronting_bond(Uint128::new(1_000_000));
        assert_eq!(bond, Uint128::new(3_000_000)); // 3x conservative multiplier
    }
}
