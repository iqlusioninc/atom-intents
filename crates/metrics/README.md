# ATOM Intents Metrics

Comprehensive metrics and monitoring for the ATOM Intent-Based Liquidity System.

## Features

- **Prometheus Metrics**: Full Prometheus metrics exposition with labeled counters, gauges, and histograms
- **HTTP Endpoint**: `/metrics` endpoint for Prometheus scraping
- **Tracing Integration**: Structured logging with correlation IDs and span tracking
- **Settlement Flow Tracking**: Track settlement phases with detailed metrics
- **Error Context Enrichment**: Add contextual information to errors for better debugging

## Metrics Categories

### Intent Metrics
- `atom_intents_received_total` - Total intents received
- `atom_intents_matched_total` - Total intents matched
- `atom_intents_failed_total` - Total intents failed
- `atom_intents_status_total{status}` - Intents by status
- `atom_intents_active` - Current active intents

### Settlement Metrics
- `atom_settlements_started_total` - Total settlements started
- `atom_settlements_completed_total` - Total settlements completed
- `atom_settlements_failed_total` - Total settlements failed
- `atom_settlement_duration_ms` - Settlement duration histogram
- `atom_settlement_phase_duration_ms{phase}` - Phase-specific duration
- `atom_settlements_active` - Current active settlements

### Solver Metrics
- `atom_solver_quotes_requested_total` - Total quote requests
- `atom_solver_quote_latency_ms` - Quote latency histogram
- `atom_solver_quote_latency_per_solver_ms{solver_id}` - Per-solver latency
- `atom_solvers_active` - Number of active solvers
- `atom_solver_health{solver_id}` - Solver health status
- `atom_solver_quote_failures_total{solver_id,reason}` - Quote failures

### IBC Metrics
- `atom_ibc_packets_sent_total` - Total IBC packets sent
- `atom_ibc_packets_received_total` - Total packets received
- `atom_ibc_packets_acked_total` - Total acknowledgments
- `atom_ibc_timeouts_total` - Total timeouts
- `atom_ibc_transfer_latency_ms` - Transfer latency histogram
- `atom_ibc_errors_total{error_type}` - IBC errors by type

### Oracle Metrics
- `atom_oracle_queries_total` - Total oracle queries
- `atom_oracle_failures_total` - Total query failures
- `atom_oracle_latency_ms` - Query latency histogram
- `atom_oracle_staleness_secs{source}` - Data staleness

### Matching Engine Metrics
- `atom_matching_attempts_total` - Total matching attempts
- `atom_matching_success_total` - Successful matches
- `atom_matching_latency_ms` - Matching latency
- `atom_matching_queue_size` - Queue size

## Usage

### Basic Setup

```rust
use atom_intents_metrics::{MetricsCollector, MetricsServer};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Create metrics collector
    let collector = Arc::new(MetricsCollector::new());

    // Start metrics HTTP server
    let server = MetricsServer::new(
        collector.clone(),
        "0.0.0.0:9090".to_string()
    );

    tokio::spawn(async move {
        server.serve().await.unwrap();
    });

    // Record metrics
    collector.record_intent_received();
}
```

### Recording Intent Metrics

```rust
use atom_intents_types::IntentStatus;

// Record intent received
collector.record_intent_received();

// Record status changes
collector.record_intent_status(IntentStatus::Pending);
collector.record_intent_status(IntentStatus::Filled);
```

### Recording Settlement Metrics

```rust
use std::time::Duration;
use atom_intents_types::SettlementStatus;

// Start settlement
collector.record_settlement_started();

// Record completion
let duration = Duration::from_secs(15);
collector.record_settlement(SettlementStatus::Complete, duration);

// Record phase durations
use atom_intents_metrics::SettlementPhase;
collector.record_settlement_phase_duration(
    SettlementPhase::IbcTransfer,
    Duration::from_secs(10)
);
```

### Recording Solver Metrics

```rust
use std::time::Duration;

// Record quote request
collector.record_solver_quote_requested();

// Record successful quote
let latency = Duration::from_millis(150);
collector.record_solver_quote("solver_1", latency);

// Record failure
collector.record_solver_quote_failure("solver_1", "timeout");

// Update health
collector.set_solver_health("solver_1", true);
```

### Tracing Integration

```rust
use atom_intents_metrics::{init_tracing_with_metrics, SettlementSpan};

// Initialize tracing
init_tracing_with_metrics(collector.clone()).unwrap();

// Create settlement span
let span = SettlementSpan::new(
    "intent_123".to_string(),
    "solver_456".to_string()
);

// Enter span for tracking
let _guard = span.enter();

// All logs within this span will include correlation_id, intent_id, solver_id
tracing::info!("Processing settlement");
```

### Error Context Enrichment

```rust
use atom_intents_metrics::ErrorContext;

fn process_intent() -> Result<(), MyError> {
    // Add context to errors
    some_operation()
        .with_intent_id("intent_123")
        .with_solver_id("solver_456")?;

    Ok(())
}
```

## Monitoring Setup

### Prometheus Configuration

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'atom-intents'
    scrape_interval: 15s
    static_configs:
      - targets: ['localhost:9090']
```

### Grafana Dashboard

Import the included Grafana dashboard:

```bash
# Load the dashboard
curl -X POST http://localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @crates/metrics/grafana-dashboard.json
```

The dashboard includes:
- Intent throughput and success rates
- Settlement duration percentiles
- Solver performance metrics
- IBC transfer latency
- Oracle query performance
- Active component counts

### Alert Rules

Load the alert rules into Prometheus:

```yaml
# prometheus.yml
rule_files:
  - "crates/metrics/alert-rules.yml"
```

Alert rules cover:
- High failure rates (intents, settlements, solvers)
- Performance degradation (slow settlements, transfers, queries)
- Capacity issues (backlogs, low solver count)
- Data quality (stale oracle data)

## Testing

Run the test suite:

```bash
cargo test -p atom-intents-metrics
```

## Architecture

### MetricsCollector

Central component for recording all metrics. Thread-safe and designed for high-performance concurrent access using atomic operations.

### MetricsServer

HTTP server exposing metrics on `/metrics` endpoint in Prometheus text format and `/health` endpoint for health checks.

### Tracing Integration

Uses `tracing-subscriber` for structured logging with:
- JSON formatting
- Correlation IDs for request tracking
- Span tracking for settlement flows
- Error context enrichment

## Performance Considerations

- All metrics use atomic operations for lock-free updates
- Histogram buckets are tuned for typical latencies
- Label cardinality is kept low to avoid memory issues
- Metrics are registered once at startup

## Best Practices

1. **Record metrics close to the source**: Add metrics calls where events occur
2. **Use correlation IDs**: Track requests across components
3. **Monitor success rates**: Track both successes and failures
4. **Set up alerts**: Use the provided alert rules as a starting point
5. **Review metrics regularly**: Ensure dashboards reflect current needs
