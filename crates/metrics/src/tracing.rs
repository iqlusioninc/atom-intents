use std::sync::Arc;
use tracing::{field::Visit, span, Event, Level, Subscriber};
use tracing_subscriber::{
    fmt,
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

use crate::collector::MetricsCollector;

/// Initialize tracing with metrics integration
pub fn init_tracing_with_metrics(collector: Arc<MetricsCollector>) -> Result<(), TracingError> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,atom_intents=debug"));

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .json();

    let metrics_layer = MetricsLayer::new(collector);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(metrics_layer)
        .try_init()
        .map_err(|e| TracingError::InitError(e.to_string()))?;

    Ok(())
}

/// Tracing layer that records metrics from span events
pub struct MetricsLayer {
    collector: Arc<MetricsCollector>,
}

impl MetricsLayer {
    pub fn new(collector: Arc<MetricsCollector>) -> Self {
        Self { collector }
    }
}

impl<S> Layer<S> for MetricsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = MetricsVisitor::new(&self.collector);
        event.record(&mut visitor);

        // Record error events
        if *metadata.level() == Level::ERROR {
            if let Some(error_type) = visitor.error_type.as_ref() {
                if error_type.contains("ibc") {
                    self.collector.record_ibc_error(error_type);
                }
            }
        }
    }

    fn on_enter(&self, _id: &span::Id, _ctx: Context<'_, S>) {
        // Track span entry for duration measurement
    }

    fn on_exit(&self, _id: &span::Id, _ctx: Context<'_, S>) {
        // Record span duration
    }
}

/// Visitor to extract metrics-relevant fields from events
struct MetricsVisitor<'a> {
    #[allow(dead_code)]
    collector: &'a MetricsCollector,
    error_type: Option<String>,
}

impl<'a> MetricsVisitor<'a> {
    fn new(collector: &'a MetricsCollector) -> Self {
        Self {
            collector,
            error_type: None,
        }
    }
}

impl<'a> Visit for MetricsVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "error_type" {
            self.error_type = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "error_type" {
            self.error_type = Some(value.to_string());
        }
    }
}

/// Correlation ID for tracking requests across components
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(uuid::Uuid);

impl CorrelationId {
    /// Generate a new correlation ID
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Get the correlation ID as a string
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Span context for settlement flow tracking
#[derive(Debug, Clone)]
pub struct SettlementSpan {
    pub correlation_id: CorrelationId,
    pub intent_id: String,
    pub solver_id: String,
}

impl SettlementSpan {
    pub fn new(intent_id: String, solver_id: String) -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            intent_id,
            solver_id,
        }
    }

    /// Enter a tracing span for this settlement
    pub fn enter(&self) -> tracing::span::EnteredSpan {
        tracing::info_span!(
            "settlement",
            correlation_id = %self.correlation_id,
            intent_id = %self.intent_id,
            solver_id = %self.solver_id,
        )
        .entered()
    }
}

/// Error enrichment for adding context to errors
pub trait ErrorContext {
    /// Add correlation ID context to an error
    fn with_correlation_id(self, correlation_id: CorrelationId) -> Self;

    /// Add intent ID context to an error
    fn with_intent_id(self, intent_id: &str) -> Self;

    /// Add solver ID context to an error
    fn with_solver_id(self, solver_id: &str) -> Self;
}

impl<T, E> ErrorContext for Result<T, E>
where
    E: std::fmt::Display,
{
    fn with_correlation_id(self, correlation_id: CorrelationId) -> Self {
        self.map_err(|e| {
            tracing::error!(
                correlation_id = %correlation_id,
                error = %e,
                "error occurred"
            );
            e
        })
    }

    fn with_intent_id(self, intent_id: &str) -> Self {
        self.map_err(|e| {
            tracing::error!(
                intent_id = %intent_id,
                error = %e,
                "error occurred"
            );
            e
        })
    }

    fn with_solver_id(self, solver_id: &str) -> Self {
        self.map_err(|e| {
            tracing::error!(
                solver_id = %solver_id,
                error = %e,
                "error occurred"
            );
            e
        })
    }
}

/// Tracing error types
#[derive(Debug, thiserror::Error)]
pub enum TracingError {
    #[error("tracing initialization error: {0}")]
    InitError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlation_id_generation() {
        let id1 = CorrelationId::new();
        let id2 = CorrelationId::new();

        // IDs should be unique
        assert_ne!(id1, id2);

        // Should be valid UUID format
        assert!(id1.as_str().len() == 36); // UUID v4 format
    }

    #[test]
    fn test_settlement_span_creation() {
        let span = SettlementSpan::new("intent_123".to_string(), "solver_456".to_string());

        assert_eq!(span.intent_id, "intent_123");
        assert_eq!(span.solver_id, "solver_456");
    }
}
