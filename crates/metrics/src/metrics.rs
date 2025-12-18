use lazy_static::lazy_static;
use prometheus::{
    register_histogram, register_histogram_vec, register_int_counter, register_int_counter_vec,
    register_int_gauge, register_int_gauge_vec, Histogram, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec,
};

lazy_static! {
    // ═══════════════════════════════════════════════════════════════════════════
    // INTENT METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of intents received by the system
    pub static ref INTENTS_RECEIVED: IntCounter = register_int_counter!(
        "atom_intents_intents_received_total",
        "Total number of intents received"
    )
    .unwrap();

    /// Total number of intents successfully matched
    pub static ref INTENTS_MATCHED: IntCounter = register_int_counter!(
        "atom_intents_intents_matched_total",
        "Total number of intents matched with solutions"
    )
    .unwrap();

    /// Total number of intents that failed
    pub static ref INTENTS_FAILED: IntCounter = register_int_counter!(
        "atom_intents_intents_failed_total",
        "Total number of intents that failed"
    )
    .unwrap();

    /// Intent status counter by status type
    pub static ref INTENT_STATUS_COUNT: IntCounterVec = register_int_counter_vec!(
        "atom_intents_intent_status_total",
        "Total intents by status",
        &["status"]
    )
    .unwrap();

    /// Current number of active intents
    pub static ref ACTIVE_INTENTS: IntGauge = register_int_gauge!(
        "atom_intents_intents_active",
        "Current number of active intents"
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // SETTLEMENT METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of settlements started
    pub static ref SETTLEMENTS_STARTED: IntCounter = register_int_counter!(
        "atom_intents_settlements_started_total",
        "Total number of settlements initiated"
    )
    .unwrap();

    /// Total number of settlements completed successfully
    pub static ref SETTLEMENTS_COMPLETED: IntCounter = register_int_counter!(
        "atom_intents_settlements_completed_total",
        "Total number of settlements completed"
    )
    .unwrap();

    /// Total number of settlements that failed
    pub static ref SETTLEMENTS_FAILED: IntCounter = register_int_counter!(
        "atom_intents_settlements_failed_total",
        "Total number of settlements that failed"
    )
    .unwrap();

    /// Settlement status counter by status type
    pub static ref SETTLEMENT_STATUS_COUNT: IntCounterVec = register_int_counter_vec!(
        "atom_intents_settlement_status_total",
        "Total settlements by status",
        &["status"]
    )
    .unwrap();

    /// Current number of active settlements
    pub static ref ACTIVE_SETTLEMENTS: IntGauge = register_int_gauge!(
        "atom_intents_settlements_active",
        "Current number of active settlements"
    )
    .unwrap();

    /// Settlement duration histogram (in milliseconds)
    pub static ref SETTLEMENT_DURATION: Histogram = register_histogram!(
        "atom_intents_settlement_duration_ms",
        "Settlement duration in milliseconds",
        vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 30000.0, 60000.0]
    )
    .unwrap();

    /// Settlement duration by phase
    pub static ref SETTLEMENT_PHASE_DURATION: HistogramVec = register_histogram_vec!(
        "atom_intents_settlement_phase_duration_ms",
        "Settlement phase duration in milliseconds",
        &["phase"],
        vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 30000.0]
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // SOLVER METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of solver quotes requested
    pub static ref SOLVER_QUOTES_REQUESTED: IntCounter = register_int_counter!(
        "atom_intents_solver_quotes_requested_total",
        "Total number of solver quotes requested"
    )
    .unwrap();

    /// Total number of solver quotes received
    pub static ref SOLVER_QUOTES_RECEIVED: IntCounter = register_int_counter!(
        "atom_intents_solver_quotes_received_total",
        "Total number of solver quotes received"
    )
    .unwrap();

    /// Solver quote request latency histogram (in milliseconds)
    pub static ref SOLVER_QUOTE_LATENCY: Histogram = register_histogram!(
        "atom_intents_solver_quote_latency_ms",
        "Solver quote request latency in milliseconds",
        vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0]
    )
    .unwrap();

    /// Solver quote latency per solver
    pub static ref SOLVER_QUOTE_LATENCY_PER_SOLVER: HistogramVec = register_histogram_vec!(
        "atom_intents_solver_quote_latency_per_solver_ms",
        "Solver quote latency per solver in milliseconds",
        &["solver_id"],
        vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0]
    )
    .unwrap();

    /// Number of currently active solvers
    pub static ref ACTIVE_SOLVERS: IntGauge = register_int_gauge!(
        "atom_intents_solvers_active",
        "Number of currently active solvers"
    )
    .unwrap();

    /// Solver health status
    pub static ref SOLVER_HEALTH: IntGaugeVec = register_int_gauge_vec!(
        "atom_intents_solver_health",
        "Solver health status (1=healthy, 0=unhealthy)",
        &["solver_id"]
    )
    .unwrap();

    /// Solver quote success rate
    pub static ref SOLVER_QUOTE_SUCCESS: IntCounterVec = register_int_counter_vec!(
        "atom_intents_solver_quote_success_total",
        "Total successful solver quotes",
        &["solver_id"]
    )
    .unwrap();

    /// Solver quote failures
    pub static ref SOLVER_QUOTE_FAILURES: IntCounterVec = register_int_counter_vec!(
        "atom_intents_solver_quote_failures_total",
        "Total solver quote failures",
        &["solver_id", "reason"]
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // IBC METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of IBC packets sent
    pub static ref IBC_PACKETS_SENT: IntCounter = register_int_counter!(
        "atom_intents_ibc_packets_sent_total",
        "Total number of IBC packets sent"
    )
    .unwrap();

    /// Total number of IBC packets received
    pub static ref IBC_PACKETS_RECEIVED: IntCounter = register_int_counter!(
        "atom_intents_ibc_packets_received_total",
        "Total number of IBC packets received"
    )
    .unwrap();

    /// Total number of IBC packet acknowledgments
    pub static ref IBC_PACKETS_ACKED: IntCounter = register_int_counter!(
        "atom_intents_ibc_packets_acked_total",
        "Total number of IBC packets acknowledged"
    )
    .unwrap();

    /// Total number of IBC packet timeouts
    pub static ref IBC_TIMEOUTS: IntCounter = register_int_counter!(
        "atom_intents_ibc_timeouts_total",
        "Total number of IBC packet timeouts"
    )
    .unwrap();

    /// IBC transfer latency histogram (in milliseconds)
    pub static ref IBC_TRANSFER_LATENCY: Histogram = register_histogram!(
        "atom_intents_ibc_transfer_latency_ms",
        "IBC transfer latency in milliseconds",
        vec![1000.0, 5000.0, 10000.0, 30000.0, 60000.0, 120000.0, 300000.0]
    )
    .unwrap();

    /// IBC transfer latency per channel
    pub static ref IBC_TRANSFER_LATENCY_PER_CHANNEL: HistogramVec = register_histogram_vec!(
        "atom_intents_ibc_transfer_latency_per_channel_ms",
        "IBC transfer latency per channel in milliseconds",
        &["channel_id"],
        vec![1000.0, 5000.0, 10000.0, 30000.0, 60000.0, 120000.0]
    )
    .unwrap();

    /// IBC errors by type
    pub static ref IBC_ERRORS: IntCounterVec = register_int_counter_vec!(
        "atom_intents_ibc_errors_total",
        "Total IBC errors by type",
        &["error_type"]
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // ORACLE METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of oracle queries
    pub static ref ORACLE_QUERIES: IntCounter = register_int_counter!(
        "atom_intents_oracle_queries_total",
        "Total number of oracle queries"
    )
    .unwrap();

    /// Total number of oracle failures
    pub static ref ORACLE_FAILURES: IntCounter = register_int_counter!(
        "atom_intents_oracle_failures_total",
        "Total number of oracle query failures"
    )
    .unwrap();

    /// Oracle query latency histogram (in milliseconds)
    pub static ref ORACLE_LATENCY: Histogram = register_histogram!(
        "atom_intents_oracle_latency_ms",
        "Oracle query latency in milliseconds",
        vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0]
    )
    .unwrap();

    /// Oracle query latency per source
    pub static ref ORACLE_LATENCY_PER_SOURCE: HistogramVec = register_histogram_vec!(
        "atom_intents_oracle_latency_per_source_ms",
        "Oracle query latency per source in milliseconds",
        &["source"],
        vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0]
    )
    .unwrap();

    /// Oracle data staleness (in seconds)
    pub static ref ORACLE_STALENESS: HistogramVec = register_histogram_vec!(
        "atom_intents_oracle_staleness_secs",
        "Oracle data staleness in seconds",
        &["source"],
        vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0]
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // MATCHING ENGINE METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Total number of matching attempts
    pub static ref MATCHING_ATTEMPTS: IntCounter = register_int_counter!(
        "atom_intents_matching_attempts_total",
        "Total number of matching attempts"
    )
    .unwrap();

    /// Total number of successful matches
    pub static ref MATCHING_SUCCESS: IntCounter = register_int_counter!(
        "atom_intents_matching_success_total",
        "Total number of successful matches"
    )
    .unwrap();

    /// Matching engine latency histogram (in milliseconds)
    pub static ref MATCHING_LATENCY: Histogram = register_histogram!(
        "atom_intents_matching_latency_ms",
        "Matching engine latency in milliseconds",
        vec![1.0, 5.0, 10.0, 50.0, 100.0, 250.0, 500.0]
    )
    .unwrap();

    /// Number of intents in matching queue
    pub static ref MATCHING_QUEUE_SIZE: IntGauge = register_int_gauge!(
        "atom_intents_matching_queue_size",
        "Number of intents in matching queue"
    )
    .unwrap();

    // ═══════════════════════════════════════════════════════════════════════════
    // SYSTEM METRICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// System uptime in seconds
    pub static ref SYSTEM_UPTIME: IntCounter = register_int_counter!(
        "atom_intents_system_uptime_seconds",
        "System uptime in seconds"
    )
    .unwrap();

    /// Total value locked in the system (in smallest denomination)
    pub static ref TOTAL_VALUE_LOCKED: IntGaugeVec = register_int_gauge_vec!(
        "atom_intents_total_value_locked",
        "Total value locked in the system",
        &["denom"]
    )
    .unwrap();
}
