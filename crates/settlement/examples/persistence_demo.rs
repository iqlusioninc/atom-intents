/// Example demonstrating the settlement persistence layer
///
/// This shows how to use InMemoryStore, SqliteStore, and SettlementManager
/// to persist settlement records and track their state transitions.
use atom_intents_settlement::{
    InMemoryStore, SettlementConfig, SettlementEvent, SettlementManager, SettlementRecord,
    SettlementResult, SettlementStore, SqliteStore,
};
use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, Intent, OutputSpec, SettlementStatus,
    SolverCapabilities, SolverInfo, PROTOCOL_VERSION,
};
use cosmwasm_std::{Binary, Uint128};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Settlement Persistence Layer Demo ===\n");

    // Example 1: In-Memory Store (for testing)
    println!("1. Using InMemoryStore:");
    demo_inmemory_store().await?;

    println!("\n");

    // Example 2: SQLite Store (for production)
    println!("2. Using SqliteStore:");
    demo_sqlite_store().await?;

    println!("\n");

    // Example 3: SettlementManager with full lifecycle
    println!("3. Full Settlement Lifecycle:");
    demo_settlement_lifecycle().await?;

    Ok(())
}

async fn demo_inmemory_store() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryStore::new();

    // Create a settlement record
    let settlement = SettlementRecord::new(
        "settlement-001".to_string(),
        "intent-001".to_string(),
        "cosmos1user...".to_string(),
        Asset::new("cosmoshub-4", "uatom", 1_000_000), // 1 ATOM
        Asset::new("osmosis-1", "uosmo", 5_000_000),   // 5 OSMO
        1700000000,                                    // expires_at
        1700000000,                                    // created_at
    );

    // Store it
    store.create(&settlement).await?;
    println!("  Created settlement: {}", settlement.id);

    // Update status
    store
        .update_status(
            "settlement-001",
            SettlementStatus::UserLocked,
            Some("User funds locked".to_string()),
        )
        .await?;
    println!("  Updated status to UserLocked");

    // Retrieve and verify
    let retrieved = store.get("settlement-001").await?.unwrap();
    println!("  Status: {:?}", retrieved.status);

    // Get history
    let history = store.get_history("settlement-001").await?;
    println!("  State transitions: {} recorded", history.len());

    Ok(())
}

async fn demo_sqlite_store() -> Result<(), Box<dyn std::error::Error>> {
    // Use in-memory SQLite for demo
    let store = SqliteStore::in_memory().await?;

    let settlement = SettlementRecord::new(
        "settlement-002".to_string(),
        "intent-002".to_string(),
        "cosmos1user...".to_string(),
        Asset::new("cosmoshub-4", "uatom", 2_000_000),
        Asset::new("osmosis-1", "uosmo", 10_000_000),
        1700000000,
        1700000000,
    );

    store.create(&settlement).await?;
    println!("  Created settlement in SQLite: {}", settlement.id);

    // Query by status
    let pending = store.list_by_status(SettlementStatus::Pending, 10).await?;
    println!("  Found {} pending settlements", pending.len());

    Ok(())
}

async fn demo_settlement_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    // Setup
    let store = Arc::new(InMemoryStore::new());
    let config = SettlementConfig::default();
    let manager = SettlementManager::new(store.clone(), config);

    // Create test intent and solver
    let intent = create_test_intent();
    let solver = create_test_solver();

    // Step 1: Start settlement
    let settlement = manager.start_settlement(&intent, &solver).await?;
    println!("  Started settlement: {}", settlement.id);
    println!("  Initial status: {:?}", settlement.status);

    // Step 2: User locks funds
    let updated = manager
        .advance_settlement(
            &settlement.id,
            SettlementEvent::UserLocked {
                escrow_id: "escrow-123".to_string(),
                tx_hash: Some("0xabc...".to_string()),
            },
        )
        .await?;
    println!("  Advanced to: {:?}", updated.status);
    println!("  Escrow ID: {:?}", updated.escrow_id);

    // Step 3: Solver locks funds
    let updated = manager
        .advance_settlement(
            &settlement.id,
            SettlementEvent::SolverLocked {
                bond_id: "bond-456".to_string(),
                tx_hash: Some("0xdef...".to_string()),
            },
        )
        .await?;
    println!("  Advanced to: {:?}", updated.status);
    println!("  Bond ID: {:?}", updated.solver_bond_id);

    // Step 4: Execute IBC transfer
    let updated = manager
        .advance_settlement(
            &settlement.id,
            SettlementEvent::IbcTransferStarted {
                sequence: 42,
                tx_hash: Some("0xghi...".to_string()),
            },
        )
        .await?;
    println!("  Advanced to: {:?}", updated.status);

    // Step 5: Complete settlement
    manager
        .complete_settlement(
            &settlement.id,
            SettlementResult::Success {
                output_delivered: Uint128::new(5_000_000),
                tx_hash: Some("0xjkl...".to_string()),
            },
        )
        .await?;
    println!("  Settlement completed!");

    // Get full history
    let history = manager.get_history(&settlement.id).await?;
    println!("\n  State Transition History:");
    for (i, transition) in history.iter().enumerate() {
        println!(
            "    {}. {:?} -> {:?} at {}",
            i + 1,
            transition.from_status,
            transition.to_status,
            transition.timestamp
        );
    }

    // Query by solver
    let solver_settlements = manager.list_by_solver(&solver.id, 10).await?;
    println!(
        "\n  Solver {} has {} settlements",
        solver.id,
        solver_settlements.len()
    );

    Ok(())
}

fn create_test_intent() -> Intent {
    Intent {
        id: "intent-demo".to_string(),
        version: PROTOCOL_VERSION.to_string(),
        nonce: 1,
        user: "cosmos1user123...".to_string(),
        input: Asset::new("cosmoshub-4", "uatom", 1_000_000),
        output: OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1recipient...".to_string(),
        },
        fill_config: FillConfig::default(),
        constraints: ExecutionConstraints::new(1700000000),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 1700000000,
        expires_at: 1700001000,
    }
}

fn create_test_solver() -> SolverInfo {
    SolverInfo {
        id: "solver-alpha".to_string(),
        name: "Alpha Solver".to_string(),
        operator: "cosmos1operator...".to_string(),
        capabilities: SolverCapabilities::default(),
        bond_amount: Uint128::new(100_000_000), // 100 ATOM bond
        registered_at: 1699000000,
        active: true,
    }
}
