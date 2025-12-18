-- Settlement records table
CREATE TABLE IF NOT EXISTS settlements (
    id TEXT PRIMARY KEY NOT NULL,
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

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_settlements_intent_id ON settlements(intent_id);
CREATE INDEX IF NOT EXISTS idx_settlements_solver_id ON settlements(solver_id);
CREATE INDEX IF NOT EXISTS idx_settlements_status_created ON settlements(status, created_at);
CREATE INDEX IF NOT EXISTS idx_settlements_expires_at ON settlements(expires_at);
CREATE INDEX IF NOT EXISTS idx_settlements_user_address ON settlements(user_address);
