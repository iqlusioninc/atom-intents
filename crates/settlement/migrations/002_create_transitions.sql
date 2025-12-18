-- Settlement state transition history
CREATE TABLE IF NOT EXISTS settlement_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    settlement_id TEXT NOT NULL,
    from_status TEXT NOT NULL,
    to_status TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    details TEXT,
    tx_hash TEXT,
    FOREIGN KEY (settlement_id) REFERENCES settlements(id) ON DELETE CASCADE
);

-- Index for efficient history lookup
CREATE INDEX IF NOT EXISTS idx_transitions_settlement_id ON settlement_transitions(settlement_id, timestamp);
