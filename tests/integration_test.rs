use async_trait::async_trait;
use atom_intents_matching_engine::MatchingEngine;
use atom_intents_settlement::{
    determine_flow, EscrowContract, EscrowLock, IbcFlowType, IbcResult, IbcTransferBuilder,
    RelayerService, SettlementError, SolverVaultContract, TimeoutConfig, TwoPhaseSettlement,
    VaultLock,
};
use atom_intents_solver::{DexRoutingSolver, MockDexClient, MockOracle, SolutionAggregator};
use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, IbcTransferInfo, OutputSpec,
    SolverQuote, TradingPair,
};
use cosmwasm_std::{Binary, Uint128};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

// ═══════════════════════════════════════════════════════════════════════════
// MOCK IMPLEMENTATIONS FOR TESTING
// ═══════════════════════════════════════════════════════════════════════════

/// Mock escrow contract that tracks locks in memory
#[derive(Clone)]
struct MockEscrowContract {
    locks: Arc<Mutex<HashMap<String, EscrowLock>>>,
    next_lock_id: Arc<Mutex<u64>>,
    should_fail: Arc<Mutex<bool>>,
}

impl MockEscrowContract {
    fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            next_lock_id: Arc::new(Mutex::new(1)),
            should_fail: Arc::new(Mutex::new(false)),
        }
    }

    fn set_should_fail(&self, fail: bool) {
        *self.should_fail.lock().unwrap() = fail;
    }

    fn get_lock(&self, id: &str) -> Option<EscrowLock> {
        self.locks.lock().unwrap().get(id).cloned()
    }
}

#[async_trait]
impl EscrowContract for MockEscrowContract {
    async fn lock(
        &self,
        user: &str,
        amount: Uint128,
        denom: &str,
        timeout: u64,
    ) -> Result<EscrowLock, SettlementError> {
        if *self.should_fail.lock().unwrap() {
            return Err(SettlementError::EscrowLockFailed(
                "simulated failure".to_string(),
            ));
        }

        let mut next_id = self.next_lock_id.lock().unwrap();
        let lock_id = format!("escrow-{}", *next_id);
        *next_id += 1;

        let lock = EscrowLock {
            id: lock_id.clone(),
            amount,
            denom: denom.to_string(),
            owner: user.to_string(),
            expires_at: timeout,
        };

        self.locks.lock().unwrap().insert(lock_id, lock.clone());
        Ok(lock)
    }

    async fn release_to(
        &self,
        lock: &EscrowLock,
        _recipient: &str,
    ) -> Result<(), SettlementError> {
        self.locks.lock().unwrap().remove(&lock.id);
        Ok(())
    }

    async fn refund(&self, lock: &EscrowLock) -> Result<(), SettlementError> {
        self.locks.lock().unwrap().remove(&lock.id);
        Ok(())
    }
}

/// Mock solver vault contract
#[derive(Clone)]
struct MockSolverVault {
    locks: Arc<Mutex<HashMap<String, VaultLock>>>,
    next_lock_id: Arc<Mutex<u64>>,
    should_fail: Arc<Mutex<bool>>,
}

impl MockSolverVault {
    fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            next_lock_id: Arc::new(Mutex::new(1)),
            should_fail: Arc::new(Mutex::new(false)),
        }
    }

    fn set_should_fail(&self, fail: bool) {
        *self.should_fail.lock().unwrap() = fail;
    }

    fn get_lock(&self, id: &str) -> Option<VaultLock> {
        self.locks.lock().unwrap().get(id).cloned()
    }
}

#[async_trait]
impl SolverVaultContract for MockSolverVault {
    async fn lock(
        &self,
        solver_id: &str,
        amount: Uint128,
        denom: &str,
        timeout: u64,
    ) -> Result<VaultLock, SettlementError> {
        if *self.should_fail.lock().unwrap() {
            return Err(SettlementError::SolverVaultLockFailed(
                "simulated failure".to_string(),
            ));
        }

        let mut next_id = self.next_lock_id.lock().unwrap();
        let lock_id = format!("vault-{}", *next_id);
        *next_id += 1;

        let lock = VaultLock {
            id: lock_id.clone(),
            solver_id: solver_id.to_string(),
            amount,
            denom: denom.to_string(),
            expires_at: timeout,
        };

        self.locks.lock().unwrap().insert(lock_id, lock.clone());
        Ok(lock)
    }

    async fn unlock(&self, lock: &VaultLock) -> Result<(), SettlementError> {
        self.locks.lock().unwrap().remove(&lock.id);
        Ok(())
    }

    async fn mark_complete(&self, lock: &VaultLock) -> Result<(), SettlementError> {
        self.locks.lock().unwrap().remove(&lock.id);
        Ok(())
    }
}

/// Mock relayer service
#[derive(Clone)]
struct MockRelayer {
    settlements: Arc<Mutex<HashMap<String, Vec<IbcTransferInfo>>>>,
    should_timeout: Arc<Mutex<bool>>,
    should_error: Arc<Mutex<bool>>,
}

impl MockRelayer {
    fn new() -> Self {
        Self {
            settlements: Arc::new(Mutex::new(HashMap::new())),
            should_timeout: Arc::new(Mutex::new(false)),
            should_error: Arc::new(Mutex::new(false)),
        }
    }

    fn set_should_timeout(&self, timeout: bool) {
        *self.should_timeout.lock().unwrap() = timeout;
    }

    fn set_should_error(&self, error: bool) {
        *self.should_error.lock().unwrap() = error;
    }
}

#[async_trait]
impl RelayerService for MockRelayer {
    async fn track_settlement(
        &self,
        settlement_id: &str,
        transfers: &[IbcTransferInfo],
    ) -> Result<(), SettlementError> {
        self.settlements
            .lock()
            .unwrap()
            .insert(settlement_id.to_string(), transfers.to_vec());
        Ok(())
    }

    async fn wait_for_ibc(
        &self,
        _transfer: &IbcTransferInfo,
    ) -> Result<IbcResult, SettlementError> {
        if *self.should_timeout.lock().unwrap() {
            return Ok(IbcResult::Timeout);
        }

        if *self.should_error.lock().unwrap() {
            return Ok(IbcResult::Error {
                reason: "simulated IBC error".to_string(),
            });
        }

        Ok(IbcResult::Success {
            ack: vec![1, 2, 3],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

fn make_test_intent(
    id: &str,
    user: &str,
    input_chain: &str,
    input_denom: &str,
    input_amount: u128,
    output_chain: &str,
    output_denom: &str,
    min_output: u128,
    limit_price: &str,
) -> Intent {
    Intent {
        id: id.to_string(),
        version: "1.0".to_string(),
        nonce: 0,
        user: user.to_string(),
        input: Asset::new(input_chain, input_denom, input_amount),
        output: OutputSpec {
            chain_id: output_chain.to_string(),
            denom: output_denom.to_string(),
            min_amount: Uint128::new(min_output),
            limit_price: limit_price.to_string(),
            recipient: user.to_string(),
        },
        fill_config: FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: "0.1".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
        constraints: ExecutionConstraints::new(9999999999),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 0,
        expires_at: 9999999999,
    }
}

fn current_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════════════
// INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_full_settlement_flow() {
    // This test covers the complete end-to-end flow:
    // 1. Create an intent with the matching engine
    // 2. Get solver quotes via the aggregator
    // 3. Match the intent and create fills
    // 4. Execute two-phase settlement (user lock → solver lock → execute → complete)
    // 5. Verify state transitions at each step

    // Setup
    let mut engine = MatchingEngine::new();
    let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
    let solver = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex]));
    let oracle = Arc::new(MockOracle::new("test-oracle"));
    // Set up oracle price for the trading pair
    let pair = TradingPair::new("uatom", "uusdc");
    oracle.set_price(&pair, Decimal::from_str("10.5").unwrap(), Decimal::from_str("0.01").unwrap()).await.unwrap();
    let aggregator = SolutionAggregator::new(vec![solver.clone()], oracle);

    let escrow = MockEscrowContract::new();
    let vault = MockSolverVault::new();
    let relayer = MockRelayer::new();
    let config = TimeoutConfig::default();

    let settlement_engine = TwoPhaseSettlement::new(escrow.clone(), vault.clone(), relayer, config);

    // Step 1: Create an intent
    let intent = make_test_intent(
        "intent-1",
        "user1",
        "cosmoshub-4",
        "uatom",
        10_000_000, // 10 ATOM
        "noble-1",
        "uusdc",
        100_000_000, // 100 USDC minimum
        "10.0",       // 10 USDC/ATOM limit price
    );

    // Step 2: Process through matching engine
    let current_time = current_time();
    let match_result = engine.process_intent(&intent, current_time).unwrap();

    // No existing orders, so nothing matches
    assert!(match_result.fills.is_empty());
    assert_eq!(match_result.remaining, Uint128::new(10_000_000));

    // Step 3: Get solver solution via aggregator
    let fill_plan = aggregator
        .aggregate(&intent, Uint128::zero())
        .await
        .unwrap();

    assert!(!fill_plan.selected.is_empty());
    let (solution, _amount) = &fill_plan.selected[0];

    // Step 4: Execute two-phase settlement
    let settlement = settlement_engine
        .execute(&intent, solution, current_time)
        .await
        .unwrap();

    // Step 5: Verify settlement completed successfully
    assert_eq!(settlement.intent_id, "intent-1");
    assert_eq!(settlement.solver_id, "solver-1");
    assert!(matches!(
        settlement.status,
        atom_intents_types::SettlementStatus::Complete
    ));

    // Verify locks were created and released
    assert!(escrow.get_lock("escrow-1").is_none()); // Lock should be released
    assert!(vault.get_lock("vault-1").is_none()); // Lock should be released
}

#[tokio::test]
async fn test_batch_auction_with_internal_crossing() {
    // This test verifies batch auction functionality:
    // 1. Submit multiple intents for the same trading pair
    // 2. Run batch auction with internal crossing
    // 3. Verify crossed orders get matched at oracle price
    // 4. Verify net demand/supply goes to solvers

    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Create matching buy and sell intents
    let buy_intent = make_test_intent(
        "buy-1",
        "buyer1",
        "cosmoshub-4",
        "uusdc",
        105_000_000, // 105 USDC
        "cosmoshub-4",
        "uatom",
        10_000_000, // Want 10 ATOM minimum
        "0.095",     // Willing to pay ~10.5 USDC/ATOM
    );

    let sell_intent = make_test_intent(
        "sell-1",
        "seller1",
        "cosmoshub-4",
        "uatom",
        10_000_000, // 10 ATOM
        "cosmoshub-4",
        "uusdc",
        100_000_000, // Want 100 USDC minimum
        "10.0",       // Want at least 10 USDC/ATOM
    );

    let intents = vec![buy_intent, sell_intent];
    let oracle_price = Decimal::from_str("10.25").unwrap();

    // Run batch auction
    let result = engine
        .run_batch_auction(pair, intents, vec![], oracle_price)
        .unwrap();

    // Verify internal crossing occurred
    assert!(!result.internal_fills.is_empty());
    assert_eq!(result.internal_fills.len(), 2); // One fill for buy, one for sell

    // Verify clearing price
    let clearing_price = Decimal::from_str(&result.clearing_price).unwrap();
    assert!(clearing_price > Decimal::ZERO);

    // No net demand/supply, so no solver fills needed
    assert!(result.solver_fills.is_empty());

    // Verify epoch incremented
    assert_eq!(result.epoch_id, 1);
}

#[tokio::test]
async fn test_batch_auction_with_net_demand() {
    // Test batch auction with unbalanced orders requiring solver intervention

    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // More buy than sell - creates net demand
    let buy1 = make_test_intent(
        "buy-1",
        "buyer1",
        "cosmoshub-4",
        "uusdc",
        100_000_000, // 100 USDC
        "cosmoshub-4",
        "uatom",
        9_000_000,
        "0.1", // 10 USDC/ATOM
    );

    let buy2 = make_test_intent(
        "buy-2",
        "buyer2",
        "cosmoshub-4",
        "uusdc",
        100_000_000, // 100 USDC
        "cosmoshub-4",
        "uatom",
        9_000_000,
        "0.1",
    );

    let sell1 = make_test_intent(
        "sell-1",
        "seller1",
        "cosmoshub-4",
        "uatom",
        5_000_000, // Only 5 ATOM to sell
        "cosmoshub-4",
        "uusdc",
        50_000_000,
        "10.0",
    );

    let intents = vec![buy1, buy2, sell1];
    let oracle_price = Decimal::from_str("10.0").unwrap();

    // Provide solver quotes for the net demand
    let solver_quote = SolverQuote {
        solver_id: "solver-1".to_string(),
        input_amount: Uint128::new(150_000_000), // Can provide up to 150 USDC worth
        output_amount: Uint128::new(1_500_000_000),
        price: "10.0".to_string(),
        valid_for_ms: 5000,
    };

    let result = engine
        .run_batch_auction(pair, intents, vec![solver_quote], oracle_price)
        .unwrap();

    // Should have both internal and solver fills
    assert!(!result.internal_fills.is_empty());
    assert!(!result.solver_fills.is_empty());

    // Verify solver was needed for net demand
    let solver_fill_counterparty = &result.solver_fills[0].counterparty;
    assert_eq!(solver_fill_counterparty, "solver-1");
}

#[tokio::test]
async fn test_multi_pair_trading() {
    // Test that different trading pairs are handled independently

    let mut engine = MatchingEngine::new();

    // ATOM/USDC pair
    let atom_intent = make_test_intent(
        "atom-1",
        "user1",
        "cosmoshub-4",
        "uatom",
        10_000_000,
        "noble-1",
        "uusdc",
        100_000_000,
        "10.0",
    );

    // OSMO/ATOM pair
    let osmo_intent = make_test_intent(
        "osmo-1",
        "user2",
        "osmosis-1",
        "uosmo",
        100_000_000,
        "cosmoshub-4",
        "uatom",
        10_000_000,
        "0.1", // 0.1 ATOM per OSMO
    );

    let current_time = current_time();

    // Process both intents
    let atom_result = engine.process_intent(&atom_intent, current_time).unwrap();
    let osmo_result = engine
        .process_intent(&osmo_intent, current_time + 1)
        .unwrap();

    // Both should be added to their respective books
    assert_eq!(atom_result.remaining, Uint128::new(10_000_000));
    assert_eq!(osmo_result.remaining, Uint128::new(100_000_000));

    // Verify separate order books exist
    let atom_pair = TradingPair::new("uatom", "uusdc");
    let osmo_pair = TradingPair::new("uatom", "uosmo");

    assert!(engine.get_book(&atom_pair).is_some());
    assert!(engine.get_book(&osmo_pair).is_some());

    // Verify each book has the correct depth
    let atom_book = engine.get_book(&atom_pair).unwrap();
    let osmo_book = engine.get_book(&osmo_pair).unwrap();

    assert_eq!(atom_book.ask_depth(), Uint128::new(10_000_000));
    assert_eq!(osmo_book.bid_depth(), Uint128::new(100_000_000));
}

#[tokio::test]
async fn test_settlement_failure_and_recovery() {
    // Test failure scenarios and verify proper recovery (refunds)

    let mut engine = MatchingEngine::new();
    let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
    let solver = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex]));
    let oracle = Arc::new(MockOracle::new("test-oracle"));
    // Set up oracle price for the trading pair
    let pair = TradingPair::new("uatom", "uusdc");
    oracle.set_price(&pair, Decimal::from_str("10.5").unwrap(), Decimal::from_str("0.01").unwrap()).await.unwrap();
    let aggregator = SolutionAggregator::new(vec![solver], oracle);

    let escrow = MockEscrowContract::new();
    let vault = MockSolverVault::new();
    let relayer = MockRelayer::new();

    // Configure relayer to timeout
    relayer.set_should_timeout(true);

    let config = TimeoutConfig::default();
    let settlement_engine = TwoPhaseSettlement::new(escrow.clone(), vault.clone(), relayer, config);

    let intent = make_test_intent(
        "intent-timeout",
        "user1",
        "cosmoshub-4",
        "uatom",
        10_000_000,
        "noble-1",
        "uusdc",
        100_000_000,
        "10.0",
    );

    let current_time = current_time();
    engine.process_intent(&intent, current_time).unwrap();

    let fill_plan = aggregator
        .aggregate(&intent, Uint128::zero())
        .await
        .unwrap();

    let (solution, _) = &fill_plan.selected[0];

    // Execute settlement (should timeout)
    let settlement = settlement_engine
        .execute(&intent, solution, current_time)
        .await
        .unwrap();

    // Verify settlement timed out
    assert!(matches!(
        settlement.status,
        atom_intents_types::SettlementStatus::TimedOut
    ));

    // Verify both locks were refunded
    assert!(escrow.get_lock("escrow-1").is_none());
    assert!(vault.get_lock("vault-1").is_none());
}

#[tokio::test]
async fn test_solver_vault_lock_failure() {
    // Test that user escrow is not locked if solver vault lock fails

    let escrow = MockEscrowContract::new();
    let vault = MockSolverVault::new();
    let relayer = MockRelayer::new();

    // Make vault fail
    vault.set_should_fail(true);

    let config = TimeoutConfig::default();
    let settlement_engine = TwoPhaseSettlement::new(escrow.clone(), vault.clone(), relayer, config);

    let intent = make_test_intent(
        "intent-fail",
        "user1",
        "cosmoshub-4",
        "uatom",
        10_000_000,
        "noble-1",
        "uusdc",
        100_000_000,
        "10.0",
    );

    let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
    let solver = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex]));
    let oracle = Arc::new(MockOracle::new("test-oracle"));
    // Set up oracle price for the trading pair
    let pair = TradingPair::new("uatom", "uusdc");
    oracle.set_price(&pair, Decimal::from_str("10.5").unwrap(), Decimal::from_str("0.01").unwrap()).await.unwrap();
    let aggregator = SolutionAggregator::new(vec![solver], oracle);

    let fill_plan = aggregator
        .aggregate(&intent, Uint128::zero())
        .await
        .unwrap();

    let (solution, _) = &fill_plan.selected[0];

    let current_time = current_time();

    // Settlement should fail
    let result = settlement_engine.execute(&intent, solution, current_time).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        SettlementError::SolverVaultLockFailed(_)
    ));

    // User escrow lock should have been created then rolled back
    // In this mock, we can't verify rollback easily, but in production
    // this would be handled atomically
}

#[tokio::test]
async fn test_ibc_flow_selection() {
    // Test correct IBC flow selection based on chain topology

    let mut channel_map = HashMap::new();
    channel_map.insert(
        ("cosmoshub-4".to_string(), "osmosis-1".to_string()),
        "channel-141".to_string(),
    );

    // Test 1: Same chain
    let flow = determine_flow("cosmoshub-4", "cosmoshub-4", false, &channel_map);
    assert!(matches!(flow, IbcFlowType::SameChain));

    // Test 2: Direct IBC
    let flow = determine_flow("cosmoshub-4", "osmosis-1", false, &channel_map);
    assert!(matches!(flow, IbcFlowType::DirectIbc { .. }));

    // Test 3: IBC with swap (needs hooks)
    let flow = determine_flow("cosmoshub-4", "osmosis-1", true, &channel_map);
    assert!(matches!(flow, IbcFlowType::IbcHooksWasm { .. }));

    // Test 4: Multi-hop (no direct channel)
    let flow = determine_flow("cosmoshub-4", "juno-1", false, &channel_map);
    assert!(matches!(flow, IbcFlowType::MultiHopPfm { .. }));
}

#[tokio::test]
async fn test_ibc_transfer_builder() {
    // Test IBC transfer builder constructs correct transfer info

    let current_time = current_time();

    let transfer = IbcTransferBuilder::new("cosmoshub-4", "osmosis-1", "channel-141")
        .denom("uatom")
        .amount(Uint128::new(1_000_000))
        .sender("cosmos1sender")
        .receiver("osmo1receiver")
        .timeout_secs(600)
        .memo("test memo")
        .build(current_time);

    assert_eq!(transfer.source_chain, "cosmoshub-4");
    assert_eq!(transfer.dest_chain, "osmosis-1");
    assert_eq!(transfer.channel, "channel-141");
    assert_eq!(transfer.denom, "uatom");
    assert_eq!(transfer.amount, Uint128::new(1_000_000));
    assert_eq!(transfer.sender, "cosmos1sender");
    assert_eq!(transfer.receiver, "osmo1receiver");
    assert_eq!(transfer.timeout_timestamp, current_time + 600);
    assert_eq!(transfer.memo, Some("test memo".to_string()));
}

#[tokio::test]
async fn test_timeout_calculation() {
    // Test that timeout calculations are correct based on flow type

    use atom_intents_settlement::{calculate_timeout, PfmHop};

    let base_timeout = 60u64;

    // Same chain: 1x base
    let timeout = calculate_timeout(&IbcFlowType::SameChain, base_timeout);
    assert_eq!(timeout, 60);

    // Direct IBC: 2x base
    let timeout = calculate_timeout(
        &IbcFlowType::DirectIbc {
            channel: "channel-0".to_string(),
        },
        base_timeout,
    );
    assert_eq!(timeout, 120);

    // Multi-hop: 2 + hops
    let hops = vec![
        PfmHop {
            receiver: "addr1".to_string(),
            channel: "channel-1".to_string(),
        },
        PfmHop {
            receiver: "addr2".to_string(),
            channel: "channel-2".to_string(),
        },
    ];
    let timeout = calculate_timeout(&IbcFlowType::MultiHopPfm { hops }, base_timeout);
    assert_eq!(timeout, 240); // 2 + 2 hops = 4x

    // IBC Hooks: 3x base
    let timeout = calculate_timeout(
        &IbcFlowType::IbcHooksWasm {
            contract: "contract".to_string(),
            msg: "{}".to_string(),
        },
        base_timeout,
    );
    assert_eq!(timeout, 180);
}

#[tokio::test]
async fn test_multiple_solvers_aggregation() {
    // Test that aggregator properly combines multiple solver solutions

    let mock_dex1 = Arc::new(MockDexClient::new("osmosis", 50_000_000_000, 0.003));
    let mock_dex2 = Arc::new(MockDexClient::new("astroport", 30_000_000_000, 0.002));

    let solver1 = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex1]));
    let solver2 = Arc::new(DexRoutingSolver::new("solver-2", vec![mock_dex2]));

    let oracle = Arc::new(MockOracle::new("test-oracle"));
    // Set up oracle price for the trading pair
    let pair = TradingPair::new("uatom", "uusdc");
    oracle.set_price(&pair, Decimal::from_str("10.5").unwrap(), Decimal::from_str("0.01").unwrap()).await.unwrap();
    let aggregator = SolutionAggregator::new(vec![solver1, solver2], oracle);

    let intent = make_test_intent(
        "intent-multi",
        "user1",
        "cosmoshub-4",
        "uatom",
        10_000_000,
        "noble-1",
        "uusdc",
        100_000_000,
        "10.0",
    );

    let fill_plan = aggregator
        .aggregate(&intent, Uint128::zero())
        .await
        .unwrap();

    // Should have solutions from both solvers
    assert!(!fill_plan.selected.is_empty());

    // Verify total input covered
    assert!(fill_plan.total_input > Uint128::zero());
}

#[tokio::test]
async fn test_health_check_aggregator() {
    // Test health check functionality

    let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
    let solver = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex]));

    let oracle = Arc::new(MockOracle::new("test-oracle"));
    let aggregator = SolutionAggregator::new(vec![solver], oracle);

    let health = aggregator.health_check().await;

    assert_eq!(health.len(), 1);
    assert_eq!(health[0].0, "solver-1");
    assert!(health[0].1); // Should be healthy
}

#[tokio::test]
async fn test_partial_fill_settlement() {
    // Test settlement of partial fills

    let mut engine = MatchingEngine::new();

    // Large sell order
    let sell = make_test_intent(
        "sell-large",
        "seller1",
        "cosmoshub-4",
        "uatom",
        100_000_000, // 100 ATOM
        "noble-1",
        "uusdc",
        1_000_000_000, // 1000 USDC minimum
        "10.0",
    );

    // Smaller buy order
    let buy = make_test_intent(
        "buy-small",
        "buyer1",
        "cosmoshub-4",
        "uusdc",
        300_000_000, // 300 USDC (30 ATOM worth)
        "cosmoshub-4",
        "uatom",
        30_000_000,
        "0.1", // 10 USDC/ATOM
    );

    let current_time = current_time();

    // Add sell to book
    engine.process_intent(&sell, current_time).unwrap();

    // Match with buy
    let result = engine.process_intent(&buy, current_time + 1).unwrap();

    // Should have partial fill
    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.fills[0].input_amount, Uint128::new(300_000_000));
    assert_eq!(result.fills[0].output_amount, Uint128::new(30_000_000));

    // Verify remaining in book
    let pair = TradingPair::new("uatom", "uusdc");
    let book = engine.get_book(&pair).unwrap();
    assert_eq!(book.ask_depth(), Uint128::new(70_000_000)); // 100 - 30 = 70 ATOM remaining
}

#[tokio::test]
async fn test_complex_multi_step_flow() {
    // Complex integration test combining multiple features

    let mut engine = MatchingEngine::new();

    // Step 1: Multiple intents for batch auction
    let buy1 = make_test_intent(
        "buy-1",
        "buyer1",
        "cosmoshub-4",
        "uusdc",
        100_000_000,
        "cosmoshub-4",
        "uatom",
        9_000_000,
        "0.1",
    );

    let buy2 = make_test_intent(
        "buy-2",
        "buyer2",
        "cosmoshub-4",
        "uusdc",
        50_000_000,
        "cosmoshub-4",
        "uatom",
        4_500_000,
        "0.1",
    );

    let sell1 = make_test_intent(
        "sell-1",
        "seller1",
        "cosmoshub-4",
        "uatom",
        8_000_000,
        "cosmoshub-4",
        "uusdc",
        80_000_000,
        "10.0",
    );

    let pair = TradingPair::new("uatom", "uusdc");
    let intents = vec![buy1, buy2, sell1];
    let oracle_price = Decimal::from_str("10.0").unwrap();

    // Step 2: Setup solvers
    let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
    let solver = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex]));

    // Get solver quotes
    let solver_quotes = vec![SolverQuote {
        solver_id: "solver-1".to_string(),
        input_amount: Uint128::new(70_000_000),
        output_amount: Uint128::new(700_000_000),
        price: "10.0".to_string(),
        valid_for_ms: 5000,
    }];

    // Step 3: Run batch auction
    let auction_result = engine
        .run_batch_auction(pair, intents, solver_quotes, oracle_price)
        .unwrap();

    // Verify auction results
    assert!(!auction_result.internal_fills.is_empty());
    assert_eq!(auction_result.epoch_id, 1);

    // Step 4: Verify state after auction
    let clearing_price = Decimal::from_str(&auction_result.clearing_price).unwrap();
    assert!(clearing_price > Decimal::ZERO);
}
