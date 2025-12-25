-- Initial database schema for Skip Select Simulator
-- PostgreSQL 15+

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Intents table
CREATE TABLE intents (
    id VARCHAR(64) PRIMARY KEY,
    user_address VARCHAR(128) NOT NULL,
    input_denom VARCHAR(32) NOT NULL,
    input_amount BIGINT NOT NULL,
    output_denom VARCHAR(32) NOT NULL,
    min_output_amount BIGINT NOT NULL,
    max_slippage_bps INTEGER NOT NULL DEFAULT 50,
    source_chain VARCHAR(64) DEFAULT 'cosmoshub-4',
    destination_chain VARCHAR(64),
    status VARCHAR(32) NOT NULL DEFAULT '"Pending"',
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_intents_status ON intents(status);
CREATE INDEX idx_intents_user ON intents(user_address);
CREATE INDEX idx_intents_created ON intents(created_at DESC);
CREATE INDEX idx_intents_expires ON intents(expires_at);

-- Auctions table
CREATE TABLE auctions (
    id VARCHAR(64) PRIMARY KEY,
    status VARCHAR(32) NOT NULL DEFAULT '"Open"',
    intent_ids JSONB NOT NULL DEFAULT '[]',
    quotes JSONB NOT NULL DEFAULT '[]',
    winning_quote_id VARCHAR(64),
    clearing_price DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX idx_auctions_status ON auctions(status);
CREATE INDEX idx_auctions_created ON auctions(created_at DESC);

-- Settlements table
CREATE TABLE settlements (
    id VARCHAR(64) PRIMARY KEY,
    auction_id VARCHAR(64) NOT NULL REFERENCES auctions(id),
    intent_ids JSONB NOT NULL DEFAULT '[]',
    solver_id VARCHAR(64) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT '"Pending"',
    phases JSONB NOT NULL DEFAULT '[]',
    tx_hashes JSONB DEFAULT '[]',
    gas_used BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    execution_time_ms BIGINT
);

CREATE INDEX idx_settlements_status ON settlements(status);
CREATE INDEX idx_settlements_auction ON settlements(auction_id);
CREATE INDEX idx_settlements_solver ON settlements(solver_id);
CREATE INDEX idx_settlements_created ON settlements(created_at DESC);

-- Solvers table
CREATE TABLE solvers (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    solver_type VARCHAR(32) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT '"Active"',
    reputation_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    total_volume BIGINT NOT NULL DEFAULT 0,
    success_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    avg_execution_time_ms BIGINT NOT NULL DEFAULT 0,
    supported_chains JSONB NOT NULL DEFAULT '[]',
    supported_denoms JSONB NOT NULL DEFAULT '[]',
    api_key_hash VARCHAR(256),
    connected_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_solvers_status ON solvers(status);
CREATE INDEX idx_solvers_type ON solvers(solver_type);

-- System stats table (singleton)
CREATE TABLE system_stats (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    total_intents BIGINT NOT NULL DEFAULT 0,
    pending_intents BIGINT NOT NULL DEFAULT 0,
    active_auctions BIGINT NOT NULL DEFAULT 0,
    completed_settlements BIGINT NOT NULL DEFAULT 0,
    total_volume_usd DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    active_solvers BIGINT NOT NULL DEFAULT 0,
    avg_execution_time_ms DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initialize stats row
INSERT INTO system_stats (id) VALUES (1);

-- API keys table for solver authentication
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    solver_id VARCHAR(64) REFERENCES solvers(id),
    key_hash VARCHAR(256) NOT NULL,
    name VARCHAR(128),
    scopes JSONB NOT NULL DEFAULT '["read", "quote"]',
    rate_limit INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX idx_api_keys_solver ON api_keys(solver_id);
CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);

-- Audit log table
CREATE TABLE audit_log (
    id BIGSERIAL PRIMARY KEY,
    event_type VARCHAR(64) NOT NULL,
    entity_type VARCHAR(32) NOT NULL,
    entity_id VARCHAR(64) NOT NULL,
    actor_id VARCHAR(128),
    actor_type VARCHAR(32),
    old_value JSONB,
    new_value JSONB,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_entity ON audit_log(entity_type, entity_id);
CREATE INDEX idx_audit_created ON audit_log(created_at DESC);
CREATE INDEX idx_audit_actor ON audit_log(actor_id);

-- Price history table (for analytics)
CREATE TABLE price_history (
    id BIGSERIAL PRIMARY KEY,
    denom VARCHAR(32) NOT NULL,
    price_usd DOUBLE PRECISION NOT NULL,
    volume_24h DOUBLE PRECISION,
    source VARCHAR(64),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_prices_denom ON price_history(denom, recorded_at DESC);

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply triggers
CREATE TRIGGER update_intents_updated_at
    BEFORE UPDATE ON intents
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_solvers_updated_at
    BEFORE UPDATE ON solvers
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
