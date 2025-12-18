# Settlement Persistence Layer

This document describes the persistence layer for the ATOM Intent-Based Liquidity System's settlement component.

## Overview

The persistence layer provides durable storage for settlement records, enabling:

- **State tracking**: Monitor settlements through their entire lifecycle
- **Recovery**: Identify and recover stuck or failed settlements
- **Auditing**: Complete history of state transitions
- **Analytics**: Query settlements by status, solver, or other criteria
- **Production deployment**: SQLite backend with proper migrations

## Architecture

### Core Components

1. **SettlementStore Trait**: Abstract storage interface
2. **InMemoryStore**: Fast in-memory implementation for testing
3. **SqliteStore**: Persistent SQLite implementation for production
4. **SettlementManager**: High-level settlement orchestration
5. **SettlementRecord**: Complete settlement state
6. **StateTransition**: Historical state change records

### Data Model

#### SettlementRecord

```rust
pub struct SettlementRecord {
    pub id: String,
    pub intent_id: String,
    pub solver_id: Option<String>,
    pub user_address: String,
    pub input_asset: Asset,
    pub output_asset: Asset,
    pub status: SettlementStatus,
    pub escrow_id: Option<String>,
    pub solver_bond_id: Option<String>,
    pub ibc_packet_sequence: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub completed_at: Option<u64>,
    pub error_message: Option<String>,
}
```

#### StateTransition

```rust
pub struct StateTransition {
    pub from_status: SettlementStatus,
    pub to_status: SettlementStatus,
    pub timestamp: u64,
    pub details: Option<String>,
    pub tx_hash: Option<String>,
}
```

## Usage Examples

### 1. Basic Storage Operations

```rust
use atom_intents_settlement::{InMemoryStore, SettlementStore, SettlementRecord};
use atom_intents_types::{Asset, SettlementStatus};

let store = InMemoryStore::new();

// Create a settlement
let settlement = SettlementRecord::new(
    "settlement-1".to_string(),
    "intent-1".to_string(),
    "user-address".to_string(),
    Asset::new("cosmoshub-4", "uatom", 1_000_000),
    Asset::new("osmosis-1", "uosmo", 5_000_000),
    expires_at,
    created_at,
);

store.create(&settlement).await?;

// Update status
store.update_status(
    "settlement-1",
    SettlementStatus::UserLocked,
    Some("Funds locked".to_string())
).await?;

// Query
let settlement = store.get("settlement-1").await?;
let history = store.get_history("settlement-1").await?;
```

### 2. Using SettlementManager

```rust
use atom_intents_settlement::{
    SettlementManager, SettlementConfig, SettlementEvent, SettlementResult
};
use std::sync::Arc;

let store = Arc::new(InMemoryStore::new());
let config = SettlementConfig::default();
let manager = SettlementManager::new(store, config);

// Start settlement
let settlement = manager.start_settlement(&intent, &solver).await?;

// Advance through lifecycle
manager.advance_settlement(
    &settlement.id,
    SettlementEvent::UserLocked {
        escrow_id: "escrow-123".to_string(),
        tx_hash: None,
    }
).await?;

manager.advance_settlement(
    &settlement.id,
    SettlementEvent::SolverLocked {
        bond_id: "bond-456".to_string(),
        tx_hash: None,
    }
).await?;

// Complete
manager.complete_settlement(
    &settlement.id,
    SettlementResult::Success {
        output_delivered: amount,
        tx_hash: Some("0xabc...".to_string()),
    }
).await?;
```

### 3. SQLite Production Setup

```rust
use atom_intents_settlement::SqliteStore;

// Connect to persistent database
let store = SqliteStore::new("./settlements.db").await?;

// Migrations are run automatically on connection

// Use the same interface as InMemoryStore
let settlement = store.get("settlement-1").await?;
```

### 4. Recovery Operations

```rust
// Find stuck settlements
let stuck = manager.find_stuck_settlements().await?;

for settlement in stuck {
    println!("Settlement {} stuck in status {:?}",
        settlement.id, settlement.status);

    // Attempt recovery or manual intervention
    manager.fail_settlement(
        &settlement.id,
        "Recovery timeout - manual intervention required"
    ).await?;
}
```

### 5. Analytics Queries

```rust
// List by status
let pending = store.list_by_status(SettlementStatus::Pending, 100).await?;
let completed = store.list_by_status(SettlementStatus::Complete, 100).await?;

// List by solver
let solver_settlements = store.list_by_solver("solver-1", 100).await?;

// Get by intent
let settlement = store.get_by_intent("intent-1").await?;

// List stuck (past timeout)
let threshold = current_time - 3600; // 1 hour ago
let stuck = store.list_stuck(threshold).await?;
```

## Settlement Lifecycle

```
┌─────────┐
│ Pending │
└────┬────┘
     │
     ├─> UserLocked
     │        │
     │        ├─> SolverLocked
     │        │        │
     │        │        ├─> Executing
     │        │        │        │
     │        │        │        ├─> Complete ✓
     │        │        │        │
     │        │        │        └─> Failed ✗
     │        │        │
     │        │        └─> TimedOut ✗
     │        │
     │        └─> Failed ✗
     │
     └─> Failed ✗
```

## Database Schema

### settlements table

```sql
CREATE TABLE settlements (
    id TEXT PRIMARY KEY,
    intent_id TEXT NOT NULL,
    solver_id TEXT,
    user_address TEXT NOT NULL,
    input_chain_id TEXT NOT NULL,
    input_denom TEXT NOT NULL,
    input_amount TEXT NOT NULL,
    output_chain_id TEXT NOT NULL,
    output_denom TEXT NOT NULL,
    output_amount TEXT NOT NULL,
    status TEXT NOT NULL,
    escrow_id TEXT,
    solver_bond_id TEXT,
    ibc_packet_sequence INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    completed_at INTEGER,
    error_message TEXT
);

-- Indexes
CREATE INDEX idx_settlements_intent_id ON settlements(intent_id);
CREATE INDEX idx_settlements_solver_id ON settlements(solver_id);
CREATE INDEX idx_settlements_status_created ON settlements(status, created_at);
CREATE INDEX idx_settlements_expires_at ON settlements(expires_at);
CREATE INDEX idx_settlements_user_address ON settlements(user_address);
```

### settlement_transitions table

```sql
CREATE TABLE settlement_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    settlement_id TEXT NOT NULL,
    from_status TEXT NOT NULL,
    to_status TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    details TEXT,
    tx_hash TEXT,
    FOREIGN KEY (settlement_id) REFERENCES settlements(id) ON DELETE CASCADE
);

-- Index
CREATE INDEX idx_transitions_settlement_id ON settlement_transitions(settlement_id, timestamp);
```

## Configuration

### SettlementConfig

```rust
pub struct SettlementConfig {
    /// Default settlement timeout in seconds
    pub default_timeout_secs: u64,           // Default: 1800 (30 min)

    /// Maximum concurrent settlements per solver
    pub max_concurrent_per_solver: usize,    // Default: 10

    /// Enable automatic recovery of stuck settlements
    pub enable_auto_recovery: bool,          // Default: true

    /// Stuck settlement threshold in seconds
    pub stuck_threshold_secs: u64,           // Default: 3600 (1 hour)
}
```

## Error Handling

### StoreError

```rust
pub enum StoreError {
    NotFound(String),
    DuplicateId(String),
    DatabaseError(String),
    SerializationError(String),
    ConnectionError(String),
}
```

### SettlementManagerError

```rust
pub enum SettlementManagerError {
    NotFound(String),
    InvalidStateTransition(String),
    StoreError(StoreError),
    SettlementError(SettlementError),
    ConfigError(String),
}
```

## Testing

The persistence layer includes comprehensive tests:

```bash
# Run all settlement tests
cargo test -p atom-intents-settlement

# Run specific store tests
cargo test -p atom-intents-settlement store::tests
cargo test -p atom-intents-settlement sqlite_store::tests
cargo test -p atom-intents-settlement manager::tests
```

## Performance Considerations

### InMemoryStore
- **Use case**: Testing, development
- **Performance**: O(1) lookups, O(n) queries
- **Limitations**: No persistence, memory only
- **Thread-safe**: Yes (RwLock)

### SqliteStore
- **Use case**: Production, single-node deployment
- **Performance**: Indexed queries, ACID compliance
- **Limitations**: Single-writer (SQLite limitation)
- **Scalability**: Suitable for moderate throughput
- **Thread-safe**: Yes (connection pool)

## Migration Path

For high-volume production deployments, consider:

1. **PostgreSQL**: Implement `PostgresStore` for multi-writer support
2. **Redis**: Add `RedisStore` for high-throughput caching layer
3. **Composite**: Use Redis for hot data, Postgres for archival

The `SettlementStore` trait makes these extensions straightforward.

## Monitoring

Key metrics to track:

- Settlement duration by status
- Stuck settlement count
- Success/failure rates by solver
- Average state transition times
- Database query performance

## Example: Running the Demo

```bash
cargo run -p atom-intents-settlement --example persistence_demo
```

This demonstrates:
- InMemoryStore basic operations
- SqliteStore usage
- Complete settlement lifecycle
- State transition tracking
- Query operations

## API Reference

See the inline documentation:

```bash
cargo doc -p atom-intents-settlement --open
```
