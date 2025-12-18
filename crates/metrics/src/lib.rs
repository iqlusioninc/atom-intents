//! Metrics and monitoring for the ATOM Intent-Based Liquidity System
//!
//! This crate provides comprehensive metrics collection and monitoring capabilities
//! for tracking intents, settlements, solvers, IBC operations, and oracle queries.
//!
//! # Features
//!
//! - Prometheus metrics exposition
//! - HTTP endpoint for metrics scraping
//! - Tracing integration with correlation IDs
//! - Span tracking for settlement flows
//! - Error context enrichment
//!
//! # Example
//!
//! ```no_run
//! use atom_intents_metrics::{MetricsCollector, MetricsServer};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create metrics collector
//!     let collector = Arc::new(MetricsCollector::new());
//!
//!     // Record some metrics
//!     collector.record_intent_received();
//!
//!     // Start metrics HTTP server
//!     let server = MetricsServer::new(collector.clone(), "0.0.0.0:9090".to_string());
//!     server.serve().await.unwrap();
//! }
//! ```

pub mod collector;
pub mod http;
pub mod metrics;
pub mod tracing;

pub use collector::{MetricsCollector, MetricsError, SettlementPhase};
pub use http::{MetricsServer, MetricsServerError};
pub use tracing::{
    init_tracing_with_metrics, CorrelationId, ErrorContext, SettlementSpan, TracingError,
};
