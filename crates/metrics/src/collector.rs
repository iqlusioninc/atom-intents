use std::time::Duration;

use atom_intents_types::{IntentStatus, SettlementStatus};
use prometheus::{Encoder, Registry, TextEncoder};

use crate::metrics::*;

/// Metrics collector for the ATOM Intent-Based Liquidity System
pub struct MetricsCollector {
    registry: Registry,
}

impl MetricsCollector {
    /// Create a new metrics collector with default registry
    pub fn new() -> Self {
        let registry = Registry::new();
        Self { registry }
    }

    /// Create a new metrics collector with custom registry
    pub fn with_registry(registry: Registry) -> Self {
        Self { registry }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTENT METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record a new intent being received
    pub fn record_intent_received(&self) {
        INTENTS_RECEIVED.inc();
        ACTIVE_INTENTS.inc();
    }

    /// Record an intent status change
    pub fn record_intent_status(&self, status: IntentStatus) {
        let status_str = match status {
            IntentStatus::Pending => "pending",
            IntentStatus::PartiallyFilled { .. } => "partially_filled",
            IntentStatus::Filled => "filled",
            IntentStatus::Finalized => "finalized",
            IntentStatus::Cancelled => "cancelled",
            IntentStatus::Settled => "settled",
            IntentStatus::Expired => "expired",
        };

        INTENT_STATUS_COUNT.with_label_values(&[status_str]).inc();

        // Update active intents counter
        match status {
            IntentStatus::Filled
            | IntentStatus::Finalized
            | IntentStatus::Cancelled
            | IntentStatus::Settled
            | IntentStatus::Expired => {
                ACTIVE_INTENTS.dec();
            }
            _ => {}
        }

        // Track matched intents
        if matches!(status, IntentStatus::Filled | IntentStatus::PartiallyFilled { .. }) {
            INTENTS_MATCHED.inc();
        }

        // Track failed intents
        if matches!(status, IntentStatus::Cancelled | IntentStatus::Expired) {
            INTENTS_FAILED.inc();
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SETTLEMENT METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record a new settlement being started
    pub fn record_settlement_started(&self) {
        SETTLEMENTS_STARTED.inc();
        ACTIVE_SETTLEMENTS.inc();
    }

    /// Record a settlement status change
    pub fn record_settlement_status(&self, status: SettlementStatus) {
        let status_str = match &status {
            SettlementStatus::Pending => "pending",
            SettlementStatus::UserLocked => "user_locked",
            SettlementStatus::SolverLocked => "solver_locked",
            SettlementStatus::Executing => "executing",
            SettlementStatus::Complete => "complete",
            SettlementStatus::Failed { .. } => "failed",
            SettlementStatus::TimedOut => "timed_out",
        };

        SETTLEMENT_STATUS_COUNT.with_label_values(&[status_str]).inc();

        // Update counters and active settlements
        match status {
            SettlementStatus::Complete => {
                SETTLEMENTS_COMPLETED.inc();
                ACTIVE_SETTLEMENTS.dec();
            }
            SettlementStatus::Failed { .. } | SettlementStatus::TimedOut => {
                SETTLEMENTS_FAILED.inc();
                ACTIVE_SETTLEMENTS.dec();
            }
            _ => {}
        }
    }

    /// Record settlement duration
    pub fn record_settlement_duration(&self, duration: Duration) {
        SETTLEMENT_DURATION.observe(duration.as_millis() as f64);
    }

    /// Record settlement with status and duration
    pub fn record_settlement(&self, status: SettlementStatus, duration: Duration) {
        self.record_settlement_status(status);
        self.record_settlement_duration(duration);
    }

    /// Record settlement phase duration
    pub fn record_settlement_phase_duration(&self, phase: SettlementPhase, duration: Duration) {
        let phase_str = match phase {
            SettlementPhase::UserLock => "user_lock",
            SettlementPhase::SolverLock => "solver_lock",
            SettlementPhase::IbcTransfer => "ibc_transfer",
            SettlementPhase::Completion => "completion",
        };

        SETTLEMENT_PHASE_DURATION
            .with_label_values(&[phase_str])
            .observe(duration.as_millis() as f64);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SOLVER METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record a solver quote request
    pub fn record_solver_quote_requested(&self) {
        SOLVER_QUOTES_REQUESTED.inc();
    }

    /// Record a solver quote received
    pub fn record_solver_quote_received(&self) {
        SOLVER_QUOTES_RECEIVED.inc();
    }

    /// Record solver quote latency
    pub fn record_solver_quote(&self, solver_id: &str, latency: Duration) {
        SOLVER_QUOTE_LATENCY.observe(latency.as_millis() as f64);
        SOLVER_QUOTE_LATENCY_PER_SOLVER
            .with_label_values(&[solver_id])
            .observe(latency.as_millis() as f64);
        SOLVER_QUOTE_SUCCESS.with_label_values(&[solver_id]).inc();
    }

    /// Record solver quote failure
    pub fn record_solver_quote_failure(&self, solver_id: &str, reason: &str) {
        SOLVER_QUOTE_FAILURES
            .with_label_values(&[solver_id, reason])
            .inc();
    }

    /// Set solver health status
    pub fn set_solver_health(&self, solver_id: &str, healthy: bool) {
        SOLVER_HEALTH
            .with_label_values(&[solver_id])
            .set(if healthy { 1 } else { 0 });
    }

    /// Update active solvers count
    pub fn set_active_solvers(&self, count: i64) {
        ACTIVE_SOLVERS.set(count);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // IBC METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record an IBC packet sent
    pub fn record_ibc_packet_sent(&self) {
        IBC_PACKETS_SENT.inc();
    }

    /// Record an IBC packet received
    pub fn record_ibc_packet_received(&self) {
        IBC_PACKETS_RECEIVED.inc();
    }

    /// Record an IBC packet acknowledgment
    pub fn record_ibc_packet_acked(&self) {
        IBC_PACKETS_ACKED.inc();
    }

    /// Record an IBC timeout
    pub fn record_ibc_timeout(&self) {
        IBC_TIMEOUTS.inc();
    }

    /// Record IBC transfer latency
    pub fn record_ibc_transfer(&self, channel_id: Option<&str>, latency: Duration) {
        IBC_TRANSFER_LATENCY.observe(latency.as_millis() as f64);

        if let Some(channel) = channel_id {
            IBC_TRANSFER_LATENCY_PER_CHANNEL
                .with_label_values(&[channel])
                .observe(latency.as_millis() as f64);
        }
    }

    /// Record an IBC error
    pub fn record_ibc_error(&self, error_type: &str) {
        IBC_ERRORS.with_label_values(&[error_type]).inc();
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // ORACLE METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record an oracle query
    pub fn record_oracle_query(&self, success: bool, latency: Duration) {
        ORACLE_QUERIES.inc();
        ORACLE_LATENCY.observe(latency.as_millis() as f64);

        if !success {
            ORACLE_FAILURES.inc();
        }
    }

    /// Record oracle query with source
    pub fn record_oracle_query_with_source(
        &self,
        source: &str,
        success: bool,
        latency: Duration,
        staleness_secs: Option<f64>,
    ) {
        self.record_oracle_query(success, latency);

        ORACLE_LATENCY_PER_SOURCE
            .with_label_values(&[source])
            .observe(latency.as_millis() as f64);

        if let Some(staleness) = staleness_secs {
            ORACLE_STALENESS
                .with_label_values(&[source])
                .observe(staleness);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // MATCHING ENGINE METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Record a matching attempt
    pub fn record_matching_attempt(&self) {
        MATCHING_ATTEMPTS.inc();
    }

    /// Record a successful match
    pub fn record_matching_success(&self, latency: Duration) {
        MATCHING_SUCCESS.inc();
        MATCHING_LATENCY.observe(latency.as_millis() as f64);
    }

    /// Update matching queue size
    pub fn set_matching_queue_size(&self, size: i64) {
        MATCHING_QUEUE_SIZE.set(size);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SYSTEM METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Increment system uptime counter
    pub fn increment_uptime(&self) {
        SYSTEM_UPTIME.inc();
    }

    /// Update total value locked for a denomination
    pub fn set_total_value_locked(&self, denom: &str, amount: i64) {
        TOTAL_VALUE_LOCKED.with_label_values(&[denom]).set(amount);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // EXPORT
    // ═══════════════════════════════════════════════════════════════════════════

    /// Export metrics in Prometheus text format
    pub fn export_metrics(&self) -> Result<String, MetricsError> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| MetricsError::EncodingError(e.to_string()))?;

        String::from_utf8(buffer).map_err(|e| MetricsError::EncodingError(e.to_string()))
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Settlement phase for tracking phase-specific metrics
#[derive(Debug, Clone, Copy)]
pub enum SettlementPhase {
    UserLock,
    SolverLock,
    IbcTransfer,
    Completion,
}

/// Metrics error types
#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    #[error("encoding error: {0}")]
    EncodingError(String),
    #[error("registry error: {0}")]
    RegistryError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert!(collector.export_metrics().is_ok());
    }

    #[test]
    fn test_record_intent_metrics() {
        let collector = MetricsCollector::new();

        // Record intent received
        collector.record_intent_received();

        // Record various intent statuses
        collector.record_intent_status(IntentStatus::Pending);
        collector.record_intent_status(IntentStatus::Filled);
        collector.record_intent_status(IntentStatus::Expired);

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_intents_received_total"));
        assert!(metrics.contains("atom_intents_intents_matched_total"));
        assert!(metrics.contains("atom_intents_intents_failed_total"));
    }

    #[test]
    fn test_record_settlement_metrics() {
        let collector = MetricsCollector::new();

        // Record settlement started
        collector.record_settlement_started();

        // Record settlement completion
        let duration = Duration::from_secs(10);
        collector.record_settlement(SettlementStatus::Complete, duration);

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_settlements_started_total"));
        assert!(metrics.contains("atom_intents_settlements_completed_total"));
        assert!(metrics.contains("atom_intents_settlement_duration_ms"));
    }

    #[test]
    fn test_record_solver_metrics() {
        let collector = MetricsCollector::new();

        // Record solver quote
        let latency = Duration::from_millis(150);
        collector.record_solver_quote("solver_1", latency);

        // Record solver failure
        collector.record_solver_quote_failure("solver_1", "timeout");

        // Set solver health
        collector.set_solver_health("solver_1", true);

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_solver_quote_latency_ms"));
        assert!(metrics.contains("atom_intents_solver_health"));
    }

    #[test]
    fn test_record_ibc_metrics() {
        let collector = MetricsCollector::new();

        // Record IBC packet sent
        collector.record_ibc_packet_sent();

        // Record IBC transfer
        let latency = Duration::from_secs(30);
        collector.record_ibc_transfer(Some("channel-0"), latency);

        // Record IBC timeout
        collector.record_ibc_timeout();

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_ibc_packets_sent_total"));
        assert!(metrics.contains("atom_intents_ibc_transfer_latency_ms"));
        assert!(metrics.contains("atom_intents_ibc_timeouts_total"));
    }

    #[test]
    fn test_record_oracle_metrics() {
        let collector = MetricsCollector::new();

        // Record successful oracle query
        let latency = Duration::from_millis(100);
        collector.record_oracle_query(true, latency);

        // Record failed oracle query
        collector.record_oracle_query(false, Duration::from_millis(50));

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_oracle_queries_total"));
        assert!(metrics.contains("atom_intents_oracle_failures_total"));
        assert!(metrics.contains("atom_intents_oracle_latency_ms"));
    }

    #[test]
    fn test_settlement_phase_tracking() {
        let collector = MetricsCollector::new();

        // Record different phases
        collector.record_settlement_phase_duration(
            SettlementPhase::UserLock,
            Duration::from_millis(500),
        );
        collector.record_settlement_phase_duration(
            SettlementPhase::IbcTransfer,
            Duration::from_secs(15),
        );

        let metrics = collector.export_metrics().unwrap();
        assert!(metrics.contains("atom_intents_settlement_phase_duration_ms"));
        assert!(metrics.contains("user_lock"));
        assert!(metrics.contains("ibc_transfer"));
    }
}
