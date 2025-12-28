# Upgrade System Design: Zero-Downtime Intent Processing

## Problem Statement

During system upgrades (contract migrations, backend service restarts, protocol version changes), inflight intents can fail due to:

1. **State inconsistency**: Contracts upgraded mid-settlement lose context
2. **Service interruption**: Backend restarts drop in-memory state
3. **Version mismatch**: New code can't process old-format intents
4. **IBC packet loss**: Relayer restarts miss pending packets

This document describes a system that ensures **zero failed intents during upgrades**.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        UPGRADE COORDINATOR                               │
│  Orchestrates the entire upgrade process with safety guarantees         │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │  Drain Mode  │  │  Checkpoint  │  │   Version    │  │   Rollback   │ │
│  │   Manager    │──▶│   Service    │──▶│  Migrator    │──▶│   Handler    │ │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘ │
│         │                  │                  │                  │       │
│         ▼                  ▼                  ▼                  ▼       │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                     State Persistence Layer                       │   │
│  │              (SQLite + On-chain Contract State)                   │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### 1. Drain Mode Manager

Controls the flow of new intents during upgrade windows.

```rust
pub enum DrainMode {
    /// Normal operation - accepting new intents
    Active,

    /// Draining - reject new intents, process existing
    Draining { started_at: u64, deadline: u64 },

    /// Drained - no inflight intents, safe to upgrade
    Drained,

    /// Upgrading - system is being upgraded
    Upgrading { version: String },

    /// Resuming - accepting new intents after upgrade
    Resuming { new_version: String },
}

pub struct DrainModeManager {
    mode: Arc<RwLock<DrainMode>>,
    inflight_counter: Arc<AtomicU64>,
    settlement_store: Arc<dyn SettlementStore>,
}

impl DrainModeManager {
    /// Start draining - reject new intents
    pub async fn start_drain(&self, deadline_secs: u64) -> Result<(), DrainError>;

    /// Check if system is fully drained
    pub async fn is_drained(&self) -> bool;

    /// Wait for all inflight intents to complete or timeout
    pub async fn wait_for_drain(&self, timeout: Duration) -> Result<DrainResult, DrainError>;

    /// Resume normal operation
    pub async fn resume(&self, new_version: String) -> Result<(), DrainError>;
}
```

**Drain Process:**
1. Set mode to `Draining`
2. Return `503 Service Unavailable` for new intent submissions
3. Continue processing existing intents
4. Wait for all inflight intents to reach terminal state
5. Set mode to `Drained` when counter reaches zero

### 2. Checkpoint Service

Captures system state before upgrade for recovery.

```rust
pub struct SystemCheckpoint {
    /// Unique checkpoint identifier
    pub id: String,

    /// Protocol version at checkpoint time
    pub protocol_version: String,

    /// Timestamp of checkpoint creation
    pub created_at: u64,

    /// All inflight settlements at checkpoint time
    pub inflight_settlements: Vec<SettlementSnapshot>,

    /// Pending IBC packets
    pub pending_ibc_packets: Vec<IbcPacketSnapshot>,

    /// Order book state
    pub order_book_snapshot: OrderBookSnapshot,

    /// Solver states
    pub solver_states: Vec<SolverSnapshot>,
}

pub struct CheckpointService {
    store: Arc<dyn SettlementStore>,
    checkpoint_dir: PathBuf,
}

impl CheckpointService {
    /// Create a full system checkpoint
    pub async fn create_checkpoint(&self) -> Result<SystemCheckpoint, CheckpointError>;

    /// Restore from checkpoint after failed upgrade
    pub async fn restore_from_checkpoint(
        &self,
        checkpoint_id: &str
    ) -> Result<RestoreResult, CheckpointError>;

    /// Validate checkpoint integrity
    pub async fn validate_checkpoint(
        &self,
        checkpoint: &SystemCheckpoint
    ) -> Result<ValidationResult, CheckpointError>;
}
```

**Checkpoint Contents:**
- All settlements in non-terminal states
- IBC packet sequences and their status
- Order book entries with user signatures
- Solver bond states
- Configuration snapshots

### 3. Version Migrator

Handles data and protocol version migrations.

```rust
pub struct VersionMigrator {
    migrations: Vec<Box<dyn Migration>>,
}

pub trait Migration: Send + Sync {
    /// Source version this migration applies to
    fn from_version(&self) -> &str;

    /// Target version after migration
    fn to_version(&self) -> &str;

    /// Migrate intent format
    fn migrate_intent(&self, intent: &Intent) -> Result<Intent, MigrationError>;

    /// Migrate settlement format
    fn migrate_settlement(&self, settlement: &Settlement) -> Result<Settlement, MigrationError>;

    /// Check if migration is reversible
    fn is_reversible(&self) -> bool;

    /// Reverse migration (if supported)
    fn reverse(&self) -> Option<Box<dyn Migration>>;
}

impl VersionMigrator {
    /// Migrate all inflight intents to new version
    pub async fn migrate_inflight(
        &self,
        from: &str,
        to: &str,
        store: &dyn SettlementStore,
    ) -> Result<MigrationReport, MigrationError>;

    /// Check if migration path exists
    pub fn can_migrate(&self, from: &str, to: &str) -> bool;

    /// Get migration path
    pub fn get_migration_path(&self, from: &str, to: &str) -> Option<Vec<&dyn Migration>>;
}
```

**Version Compatibility:**
- Intent `version` field determines processing rules
- Old intents processed with original semantics
- New code can handle multiple versions simultaneously
- Graceful degradation for unsupported versions

### 4. Contract Migration Support

Add migration capability to CosmWasm contracts.

```rust
// contracts/settlement/src/msg.rs
#[cw_serde]
pub struct MigrateMsg {
    /// New protocol version
    pub new_version: String,

    /// Migration configuration
    pub config: Option<MigrationConfig>,
}

#[cw_serde]
pub struct MigrationConfig {
    /// Preserve inflight settlements during migration
    pub preserve_inflight: bool,

    /// Action for stuck settlements
    pub stuck_settlement_action: StuckSettlementAction,

    /// New configuration values (optional)
    pub new_config: Option<ConfigUpdate>,
}

#[cw_serde]
pub enum StuckSettlementAction {
    /// Keep as-is, process after migration
    Preserve,

    /// Refund users, mark as cancelled
    RefundAndCancel,

    /// Extend timeout to allow completion
    ExtendTimeout { additional_seconds: u64 },
}
```

```rust
// contracts/settlement/src/contract.rs
#[entry_point]
pub fn migrate(
    deps: DepsMut,
    env: Env,
    msg: MigrateMsg,
) -> Result<Response, ContractError> {
    // 1. Validate migration is authorized
    let config = CONFIG.load(deps.storage)?;

    // 2. Check for inflight settlements
    let inflight = query_inflight_settlements(deps.as_ref())?;
    if !inflight.is_empty() && !msg.config.map_or(true, |c| c.preserve_inflight) {
        return Err(ContractError::InflightSettlementsExist {
            count: inflight.len() as u64,
        });
    }

    // 3. Handle stuck settlements
    if let Some(config) = &msg.config {
        handle_stuck_settlements(deps.storage, &env, &config.stuck_settlement_action)?;
    }

    // 4. Update contract version
    set_contract_version(deps.storage, CONTRACT_NAME, &msg.new_version)?;

    // 5. Apply new configuration if provided
    if let Some(new_config) = msg.config.and_then(|c| c.new_config) {
        apply_config_update(deps.storage, new_config)?;
    }

    // 6. Emit migration event
    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("from_version", CONTRACT_VERSION)
        .add_attribute("to_version", &msg.new_version)
        .add_attribute("preserved_inflight", inflight.len().to_string()))
}
```

### 5. Upgrade Coordinator

Orchestrates the complete upgrade process.

```rust
pub struct UpgradeCoordinator {
    drain_manager: Arc<DrainModeManager>,
    checkpoint_service: Arc<CheckpointService>,
    version_migrator: Arc<VersionMigrator>,
    health_checker: Arc<HealthChecker>,
    rollback_handler: Arc<RollbackHandler>,
}

pub struct UpgradeConfig {
    /// Maximum time to wait for drain
    pub drain_timeout: Duration,

    /// Whether to force upgrade after timeout
    pub force_after_timeout: bool,

    /// Target version
    pub target_version: String,

    /// Pre-upgrade validation checks
    pub pre_checks: Vec<UpgradeCheck>,

    /// Post-upgrade validation checks
    pub post_checks: Vec<UpgradeCheck>,
}

impl UpgradeCoordinator {
    /// Execute a coordinated upgrade
    pub async fn execute_upgrade(
        &self,
        config: UpgradeConfig,
    ) -> Result<UpgradeReport, UpgradeError> {
        // Phase 1: Pre-upgrade validation
        self.run_pre_checks(&config.pre_checks).await?;

        // Phase 2: Start drain mode
        self.drain_manager.start_drain(config.drain_timeout.as_secs()).await?;

        // Phase 3: Wait for drain (with timeout)
        let drain_result = self.drain_manager
            .wait_for_drain(config.drain_timeout)
            .await?;

        // Phase 4: Create checkpoint
        let checkpoint = self.checkpoint_service.create_checkpoint().await?;

        // Phase 5: Execute migration
        let migration_result = self.execute_migration(&config, &checkpoint).await;

        // Phase 6: Handle result
        match migration_result {
            Ok(report) => {
                // Run post-checks
                if let Err(e) = self.run_post_checks(&config.post_checks).await {
                    // Rollback on post-check failure
                    self.rollback_handler.rollback(&checkpoint).await?;
                    return Err(UpgradeError::PostCheckFailed(e));
                }

                // Resume normal operation
                self.drain_manager.resume(config.target_version).await?;
                Ok(report)
            }
            Err(e) => {
                // Rollback on migration failure
                self.rollback_handler.rollback(&checkpoint).await?;
                self.drain_manager.resume(checkpoint.protocol_version).await?;
                Err(e)
            }
        }
    }
}
```

---

## Upgrade Procedures

### Procedure 1: Backend Service Upgrade

For Rust backend service upgrades (orchestrator, relayer, etc.):

```
┌─────────────────────────────────────────────────────────────────────┐
│                    BACKEND UPGRADE SEQUENCE                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  1. ANNOUNCE UPGRADE                                                 │
│     └── Set maintenance window                                       │
│     └── Notify connected clients                                     │
│                                                                      │
│  2. ENTER DRAIN MODE                                                 │
│     └── Stop accepting new intents                                   │
│     └── Return 503 with Retry-After header                          │
│                                                                      │
│  3. WAIT FOR COMPLETION                                              │
│     └── Process remaining intents                                    │
│     └── Complete IBC transfers in progress                          │
│     └── Timeout: 30 minutes default                                 │
│                                                                      │
│  4. CHECKPOINT STATE                                                 │
│     └── Snapshot inflight settlements                               │
│     └── Export pending IBC packets                                  │
│     └── Save order book state                                       │
│                                                                      │
│  5. STOP OLD VERSION                                                 │
│     └── Graceful shutdown                                           │
│     └── Wait for connections to close                               │
│                                                                      │
│  6. DATABASE MIGRATION                                               │
│     └── Run schema migrations                                       │
│     └── Migrate data formats                                        │
│                                                                      │
│  7. START NEW VERSION                                                │
│     └── Load checkpoint state                                       │
│     └── Resume IBC packet processing                                │
│     └── Validate state integrity                                    │
│                                                                      │
│  8. RESUME OPERATION                                                 │
│     └── Accept new intents                                          │
│     └── Notify clients                                              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Procedure 2: Smart Contract Migration

For CosmWasm contract upgrades:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    CONTRACT MIGRATION SEQUENCE                       │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  1. DEPLOY NEW CODE                                                  │
│     └── Upload new WASM to chain                                    │
│     └── Get new code_id                                             │
│     └── Verify code hash matches expected                           │
│                                                                      │
│  2. BACKEND DRAIN                                                    │
│     └── Stop creating new settlements                               │
│     └── Wait for pending settlements                                │
│                                                                      │
│  3. QUERY INFLIGHT STATE                                             │
│     └── Get all non-terminal settlements                            │
│     └── Record escrow states                                        │
│     └── Record solver bond states                                   │
│                                                                      │
│  4. EXECUTE MIGRATION                                                │
│     └── governance/multisig triggers migrate                        │
│     └── MigrateMsg with preserve_inflight: true                     │
│     └── Contract migrates storage format                            │
│                                                                      │
│  5. VERIFY MIGRATION                                                 │
│     └── Query all inflight settlements                              │
│     └── Verify state preserved correctly                            │
│     └── Check config updated                                        │
│                                                                      │
│  6. UPDATE BACKEND CONFIG                                            │
│     └── Point to migrated contract                                  │
│     └── Resume settlement creation                                  │
│                                                                      │
│  7. COMPLETE INFLIGHT                                                │
│     └── Process remaining settlements                               │
│     └── Normal timeout/recovery handling                            │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Procedure 3: Protocol Version Upgrade

For changes to the Intent/Settlement data formats:

```
┌─────────────────────────────────────────────────────────────────────┐
│                   PROTOCOL VERSION UPGRADE                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  1. DEPLOY VERSION-AWARE CODE                                        │
│     └── New code handles both v1.0 and v2.0 intents                │
│     └── Migration logic for v1.0 -> v2.0                           │
│                                                                      │
│  2. PARALLEL PROCESSING PHASE                                        │
│     └── Accept both v1.0 and v2.0 intents                          │
│     └── Process with version-specific logic                        │
│     └── Duration: 1-2 weeks                                         │
│                                                                      │
│  3. MIGRATION PHASE                                                  │
│     └── Convert remaining v1.0 intents to v2.0                     │
│     └── Update stored settlements                                   │
│                                                                      │
│  4. DEPRECATION PHASE                                                │
│     └── Stop accepting v1.0 intents                                │
│     └── Return upgrade required error                               │
│     └── Duration: 1 month                                           │
│                                                                      │
│  5. REMOVAL PHASE                                                    │
│     └── Remove v1.0 processing code                                │
│     └── Clean up migration code                                     │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Safety Mechanisms

### 1. Inflight Intent Protection

```rust
/// Inflight intent tracker
pub struct InflightTracker {
    /// Active intents by ID
    active: Arc<RwLock<HashMap<String, InflightIntent>>>,

    /// Counter for quick drain check
    count: Arc<AtomicU64>,
}

pub struct InflightIntent {
    pub id: String,
    pub created_at: u64,
    pub phase: SettlementPhase,
    pub user_funds_locked: bool,
    pub solver_funds_locked: bool,
    pub ibc_in_flight: bool,
}

impl InflightTracker {
    /// Register new inflight intent
    pub fn register(&self, intent_id: &str) -> Result<(), InflightError> {
        // Fail if in drain mode
        if self.is_draining() {
            return Err(InflightError::DrainModeActive);
        }
        // Register and increment counter
        self.active.write().insert(intent_id.to_string(), InflightIntent::new(intent_id));
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Mark intent as complete
    pub fn complete(&self, intent_id: &str) {
        if self.active.write().remove(intent_id).is_some() {
            self.count.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Get all inflight intents that need preservation
    pub fn get_all_inflight(&self) -> Vec<InflightIntent> {
        self.active.read().values().cloned().collect()
    }
}
```

### 2. Timeout Extension During Upgrades

```rust
impl UpgradeCoordinator {
    /// Extend timeouts for inflight settlements during upgrade
    async fn extend_inflight_timeouts(
        &self,
        extension: Duration,
    ) -> Result<u64, UpgradeError> {
        let inflight = self.tracker.get_all_inflight();
        let mut extended = 0;

        for intent in inflight {
            // Extend on-chain timeout
            self.settlement_contract.extend_timeout(
                &intent.id,
                extension.as_secs(),
            ).await?;

            // Extend local tracking
            self.store.extend_expiry(&intent.id, extension.as_secs()).await?;

            extended += 1;
        }

        Ok(extended)
    }
}
```

### 3. Rollback Mechanism

```rust
pub struct RollbackHandler {
    checkpoint_service: Arc<CheckpointService>,
    contract_client: Arc<dyn ContractClient>,
}

impl RollbackHandler {
    /// Rollback to previous checkpoint
    pub async fn rollback(
        &self,
        checkpoint: &SystemCheckpoint,
    ) -> Result<RollbackReport, RollbackError> {
        info!(
            checkpoint_id = %checkpoint.id,
            "Starting rollback to checkpoint"
        );

        // 1. Restore settlement store state
        self.restore_settlements(&checkpoint.inflight_settlements).await?;

        // 2. Restore order book
        self.restore_order_book(&checkpoint.order_book_snapshot).await?;

        // 3. Verify IBC packet state
        self.verify_ibc_packets(&checkpoint.pending_ibc_packets).await?;

        // 4. Contract rollback (if supported)
        if let Some(contract_checkpoint) = &checkpoint.contract_state {
            self.rollback_contract(contract_checkpoint).await?;
        }

        Ok(RollbackReport {
            checkpoint_id: checkpoint.id.clone(),
            settlements_restored: checkpoint.inflight_settlements.len(),
            success: true,
        })
    }
}
```

### 4. Health Checks

```rust
pub struct HealthChecker {
    components: Vec<Box<dyn HealthCheck>>,
}

#[async_trait]
pub trait HealthCheck: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self) -> HealthStatus;
}

pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

impl HealthChecker {
    /// Run all health checks
    pub async fn check_all(&self) -> Vec<(String, HealthStatus)> {
        let mut results = Vec::new();
        for check in &self.components {
            results.push((check.name().to_string(), check.check().await));
        }
        results
    }

    /// Check if system is healthy enough for upgrade
    pub async fn is_upgrade_safe(&self) -> bool {
        self.check_all().await.iter().all(|(_, status)| {
            matches!(status, HealthStatus::Healthy)
        })
    }
}

// Example health checks
pub struct SettlementStoreHealth { /* ... */ }
pub struct IbcRelayerHealth { /* ... */ }
pub struct SolverAvailabilityHealth { /* ... */ }
pub struct ContractStateHealth { /* ... */ }
```

---

## API Endpoints

### Upgrade Control API

```rust
/// POST /admin/upgrade/start
/// Start an upgrade process
pub async fn start_upgrade(
    State(coordinator): State<Arc<UpgradeCoordinator>>,
    Json(config): Json<UpgradeConfig>,
) -> Result<Json<UpgradeHandle>, ApiError>;

/// GET /admin/upgrade/status
/// Get current upgrade status
pub async fn upgrade_status(
    State(coordinator): State<Arc<UpgradeCoordinator>>,
) -> Result<Json<UpgradeStatus>, ApiError>;

/// POST /admin/upgrade/abort
/// Abort an in-progress upgrade
pub async fn abort_upgrade(
    State(coordinator): State<Arc<UpgradeCoordinator>>,
) -> Result<Json<AbortResult>, ApiError>;

/// GET /admin/drain/status
/// Get drain mode status
pub async fn drain_status(
    State(drain_manager): State<Arc<DrainModeManager>>,
) -> Result<Json<DrainStatus>, ApiError>;

/// GET /admin/inflight
/// Get all inflight intents
pub async fn list_inflight(
    State(tracker): State<Arc<InflightTracker>>,
) -> Result<Json<Vec<InflightIntent>>, ApiError>;
```

### Response Types

```rust
#[derive(Serialize)]
pub struct UpgradeStatus {
    pub state: UpgradeState,
    pub target_version: String,
    pub started_at: Option<u64>,
    pub drain_status: DrainStatus,
    pub inflight_count: u64,
    pub checkpoint_id: Option<String>,
    pub progress_pct: u8,
    pub estimated_completion: Option<u64>,
}

#[derive(Serialize)]
pub struct DrainStatus {
    pub mode: DrainMode,
    pub inflight_settlements: u64,
    pub pending_ibc_packets: u64,
    pub oldest_inflight_age_secs: Option<u64>,
}
```

---

## Database Migrations

### Migration Strategy

```rust
pub struct MigrationRunner {
    migrations: Vec<Box<dyn DatabaseMigration>>,
    store: Arc<SqliteStore>,
}

pub trait DatabaseMigration: Send + Sync {
    /// Migration version (monotonically increasing)
    fn version(&self) -> u32;

    /// Human-readable description
    fn description(&self) -> &str;

    /// SQL to apply migration
    fn up(&self) -> &str;

    /// SQL to reverse migration (if possible)
    fn down(&self) -> Option<&str>;

    /// Whether this migration requires exclusive access
    fn requires_drain(&self) -> bool;
}

impl MigrationRunner {
    /// Apply pending migrations
    pub async fn apply_pending(&self) -> Result<MigrationReport, MigrationError> {
        let current = self.get_current_version().await?;
        let pending: Vec<_> = self.migrations
            .iter()
            .filter(|m| m.version() > current)
            .collect();

        for migration in pending {
            if migration.requires_drain() {
                // Verify system is drained before applying
                self.verify_drained().await?;
            }

            self.apply_migration(migration.as_ref()).await?;
        }

        Ok(MigrationReport { /* ... */ })
    }
}
```

### Migration Files

```sql
-- migrations/003_add_upgrade_tracking.sql
CREATE TABLE IF NOT EXISTS upgrade_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version_from TEXT NOT NULL,
    version_to TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    status TEXT NOT NULL, -- 'in_progress', 'completed', 'rolled_back'
    checkpoint_id TEXT,
    settlements_migrated INTEGER DEFAULT 0,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS inflight_checkpoints (
    id TEXT PRIMARY KEY,
    settlement_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    user_funds_locked INTEGER NOT NULL,
    solver_funds_locked INTEGER NOT NULL,
    ibc_sequence INTEGER,
    checkpoint_time INTEGER NOT NULL,
    FOREIGN KEY (settlement_id) REFERENCES settlements(id)
);

CREATE INDEX idx_upgrade_history_status ON upgrade_history(status);
CREATE INDEX idx_inflight_checkpoints_time ON inflight_checkpoints(checkpoint_time);
```

---

## Monitoring and Alerts

### Metrics

```rust
// Upgrade metrics
pub static UPGRADE_STATE: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new("upgrade_state", "Current upgrade state (0=none, 1=draining, 2=upgrading)")
        .expect("metric creation failed")
});

pub static INFLIGHT_SETTLEMENTS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new("inflight_settlements", "Number of inflight settlements")
        .expect("metric creation failed")
});

pub static DRAIN_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    Histogram::with_opts(
        HistogramOpts::new("drain_duration_seconds", "Time to drain system")
            .buckets(vec![10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0])
    ).expect("metric creation failed")
});

pub static UPGRADE_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    Histogram::with_opts(
        HistogramOpts::new("upgrade_duration_seconds", "Total upgrade duration")
    ).expect("metric creation failed")
});

pub static UPGRADE_ROLLBACKS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new("upgrade_rollbacks_total", "Total number of upgrade rollbacks")
        .expect("metric creation failed")
});
```

### Alert Rules

```yaml
groups:
  - name: upgrade_alerts
    rules:
      - alert: UpgradeDrainTooLong
        expr: upgrade_state == 1 and (time() - upgrade_start_time) > 1800
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Upgrade drain taking too long"
          description: "System has been in drain mode for over 30 minutes"

      - alert: UpgradeStuckSettlements
        expr: upgrade_state == 1 and inflight_settlements > 0 and rate(inflight_settlements[10m]) == 0
        for: 10m
        labels:
          severity: critical
        annotations:
          summary: "Settlements stuck during upgrade drain"
          description: "Inflight settlements not decreasing during drain"

      - alert: UpgradeRollbackOccurred
        expr: increase(upgrade_rollbacks_total[1h]) > 0
        labels:
          severity: critical
        annotations:
          summary: "Upgrade rollback occurred"
          description: "An upgrade was rolled back in the last hour"
```

---

## Configuration

```toml
# config/upgrade.toml

[upgrade]
# Maximum time to wait for drain before forcing
drain_timeout_secs = 1800

# Whether to force upgrade after timeout (dangerous)
force_after_timeout = false

# Extend inflight settlement timeouts during upgrade
extend_timeout_secs = 3600

# Minimum time between upgrades
min_upgrade_interval_secs = 86400

[checkpoint]
# Directory to store checkpoints
checkpoint_dir = "/var/lib/atom-intents/checkpoints"

# How long to retain checkpoints
retention_days = 30

# Maximum checkpoint size before compression
compress_threshold_mb = 100

[rollback]
# Automatic rollback on post-check failure
auto_rollback = true

# Maximum rollback attempts
max_rollback_attempts = 3

# Delay between rollback attempts
rollback_retry_delay_secs = 30

[health_checks]
# Pre-upgrade checks
pre_checks = ["settlement_store", "ibc_relayer", "contract_state"]

# Post-upgrade checks
post_checks = ["settlement_store", "ibc_relayer", "contract_state", "api_endpoints"]

# Health check timeout
check_timeout_secs = 30
```

---

## Testing

### Integration Test Scenarios

1. **Happy Path Upgrade**: Full upgrade with no inflight intents
2. **Drain with Inflight**: Upgrade with active settlements that complete during drain
3. **Timeout During Drain**: Test behavior when settlements don't complete
4. **Rollback on Failure**: Verify rollback restores previous state
5. **Contract Migration**: Test on-chain state preservation
6. **Version Compatibility**: Process old-version intents with new code
7. **IBC Packet Handling**: Verify pending IBC packets processed after upgrade

### Test Utilities

```rust
#[cfg(test)]
pub mod test_utils {
    /// Create test settlements in various states
    pub async fn setup_inflight_settlements(
        store: &dyn SettlementStore,
        count: usize,
    ) -> Vec<String>;

    /// Simulate upgrade process
    pub async fn simulate_upgrade(
        coordinator: &UpgradeCoordinator,
        config: UpgradeConfig,
    ) -> UpgradeReport;

    /// Verify state preserved after upgrade
    pub async fn verify_state_preserved(
        before: &SystemCheckpoint,
        after: &SystemCheckpoint,
    ) -> bool;
}
```

---

## Implementation Phases

### Phase 1: Core Infrastructure
- Implement DrainModeManager
- Add inflight tracking to existing settlement flow
- Create checkpoint service with basic persistence

### Phase 2: Contract Migration
- Add MigrateMsg to settlement contract
- Add MigrateMsg to escrow contract
- Implement state preservation during migration

### Phase 3: Upgrade Coordinator
- Implement UpgradeCoordinator service
- Add version migration framework
- Implement rollback handling

### Phase 4: Monitoring & Tooling
- Add Prometheus metrics
- Create admin API endpoints
- Build CLI tools for upgrade management

### Phase 5: Testing & Documentation
- Comprehensive integration tests
- Upgrade runbooks
- Operator documentation

---

## Conclusion

This upgrade system ensures zero failed intents during upgrades through:

1. **Drain Mode**: Gracefully stop accepting new work while completing existing
2. **Checkpointing**: Full system state snapshot before any changes
3. **Version Compatibility**: Multi-version processing during transitions
4. **Contract Migration**: On-chain state preservation
5. **Rollback**: Automatic recovery on upgrade failure
6. **Health Checks**: Validation before and after upgrades

The system prioritizes **user fund safety** above all else - if any upgrade step fails, the system rolls back to the previous known-good state, ensuring no user funds are lost or locked.
