use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::collector::MetricsCollector;

/// HTTP server for metrics endpoint
pub struct MetricsServer {
    collector: Arc<MetricsCollector>,
    addr: String,
}

impl MetricsServer {
    /// Create a new metrics server
    pub fn new(collector: Arc<MetricsCollector>, addr: String) -> Self {
        Self { collector, addr }
    }

    /// Start the metrics HTTP server
    pub async fn serve(self) -> Result<(), MetricsServerError> {
        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/health", get(health_handler))
            .with_state(self.collector);

        let listener = TcpListener::bind(&self.addr)
            .await
            .map_err(|e| MetricsServerError::BindError(e.to_string()))?;

        tracing::info!("Metrics server listening on {}", self.addr);

        axum::serve(listener, app)
            .await
            .map_err(|e| MetricsServerError::ServerError(e.to_string()))?;

        Ok(())
    }
}

/// Handler for /metrics endpoint
/// Returns Prometheus-formatted metrics
async fn metrics_handler(
    State(collector): State<Arc<MetricsCollector>>,
) -> Result<Response, MetricsHandlerError> {
    let metrics = collector
        .export_metrics()
        .map_err(|e| MetricsHandlerError::ExportError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4")],
        metrics,
    )
        .into_response())
}

/// Handler for /health endpoint
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Metrics server error types
#[derive(Debug, thiserror::Error)]
pub enum MetricsServerError {
    #[error("failed to bind to address: {0}")]
    BindError(String),
    #[error("server error: {0}")]
    ServerError(String),
}

/// Metrics handler error types
#[derive(Debug, thiserror::Error)]
pub enum MetricsHandlerError {
    #[error("failed to export metrics: {0}")]
    ExportError(String),
}

impl IntoResponse for MetricsHandlerError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            MetricsHandlerError::ExportError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let collector = Arc::new(MetricsCollector::new());
        let server = MetricsServer::new(collector, "127.0.0.1:0".to_string());

        // Just verify we can create the server
        assert_eq!(server.addr, "127.0.0.1:0");
    }
}
