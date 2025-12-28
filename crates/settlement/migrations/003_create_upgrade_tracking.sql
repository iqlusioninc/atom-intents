-- Upgrade history tracking table
-- Records all upgrade events for audit and rollback purposes
CREATE TABLE IF NOT EXISTS upgrade_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    upgrade_id TEXT NOT NULL UNIQUE,
    version_from TEXT NOT NULL,
    version_to TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    status TEXT NOT NULL, -- 'in_progress', 'completed', 'rolled_back', 'failed'
    checkpoint_id TEXT,
    inflight_count_at_start INTEGER NOT NULL DEFAULT 0,
    settlements_preserved INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    initiated_by TEXT -- 'operator', 'automated', 'rollback'
);

-- Checkpoint snapshots for recovery
CREATE TABLE IF NOT EXISTS upgrade_checkpoints (
    id TEXT PRIMARY KEY NOT NULL,
    upgrade_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    checkpoint_type TEXT NOT NULL, -- 'pre_upgrade', 'post_upgrade', 'rollback'
    inflight_count INTEGER NOT NULL DEFAULT 0,
    pending_ibc_count INTEGER NOT NULL DEFAULT 0,
    data_hash TEXT, -- SHA256 of checkpoint data for integrity
    storage_path TEXT -- Path to checkpoint file if stored externally
);

-- Inflight intent snapshots during upgrade
CREATE TABLE IF NOT EXISTS upgrade_inflight_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    checkpoint_id TEXT NOT NULL,
    settlement_id TEXT NOT NULL,
    intent_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    user_funds_locked INTEGER NOT NULL DEFAULT 0,
    solver_funds_locked INTEGER NOT NULL DEFAULT 0,
    ibc_in_flight INTEGER NOT NULL DEFAULT 0,
    user_amount TEXT NOT NULL,
    solver_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (checkpoint_id) REFERENCES upgrade_checkpoints(id)
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_upgrade_history_status ON upgrade_history(status);
CREATE INDEX IF NOT EXISTS idx_upgrade_history_started ON upgrade_history(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_upgrade_checkpoints_upgrade ON upgrade_checkpoints(upgrade_id);
CREATE INDEX IF NOT EXISTS idx_inflight_snapshots_checkpoint ON upgrade_inflight_snapshots(checkpoint_id);
CREATE INDEX IF NOT EXISTS idx_inflight_snapshots_settlement ON upgrade_inflight_snapshots(settlement_id);
