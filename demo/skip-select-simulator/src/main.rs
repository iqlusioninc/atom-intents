//! Skip Select Simulator - Main entry point
//!
//! A simplified version of the Skip Select coordination layer for the ATOM Intents demo.
//! Provides REST API, WebSocket, and batch auction functionality.

mod api;
mod auction;
mod models;
mod oracle;
mod settlement;
mod solver;
mod state;
mod websocket;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    routing::{get, post},
    Router,
};
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "skip_select_simulator=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Skip Select Simulator");

    // Load configuration
    let config = load_config()?;
    info!("Configuration loaded: {:?}", config);

    // Initialize application state
    let state = Arc::new(RwLock::new(AppState::new(config.clone())));

    // Start mock solvers
    let solver_state = state.clone();
    tokio::spawn(async move {
        solver::run_mock_solvers(solver_state).await;
    });

    // Start auction engine
    let auction_state = state.clone();
    let auction_interval = Duration::from_millis(config.auction_interval_ms);
    tokio::spawn(async move {
        auction::run_auction_loop(auction_state, auction_interval).await;
    });

    // Start oracle price feed
    let oracle_state = state.clone();
    tokio::spawn(async move {
        oracle::run_price_feed(oracle_state).await;
    });

    // Start settlement processor
    let settlement_state = state.clone();
    tokio::spawn(async move {
        settlement::run_settlement_processor(settlement_state).await;
    });

    // Build router
    let app = build_router(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.api_port));
    info!("Skip Select Simulator listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn build_router(state: Arc<RwLock<AppState>>) -> Router {
    // CORS configuration for web UI
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health check
        .route("/health", get(api::health_check))
        // REST API routes
        .route("/api/v1/intents", post(api::submit_intent))
        .route("/api/v1/intents", get(api::list_intents))
        .route("/api/v1/intents/:id", get(api::get_intent))
        .route("/api/v1/auctions/current", get(api::get_current_auction))
        .route("/api/v1/auctions/:id", get(api::get_auction))
        .route("/api/v1/auctions/:id/quotes", get(api::get_auction_quotes))
        .route("/api/v1/settlements/:id", get(api::get_settlement))
        .route("/api/v1/prices", get(api::get_prices))
        .route("/api/v1/solvers", get(api::list_solvers))
        .route("/api/v1/stats", get(api::get_stats))
        // Demo endpoints
        .route("/api/v1/demo/generate-intent", post(api::generate_demo_intent))
        .route("/api/v1/demo/scenario/:name", post(api::run_scenario))
        // WebSocket
        .route("/ws", get(websocket::ws_handler))
        // Layers
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Debug, Clone)]
pub struct Config {
    pub api_port: u16,
    pub auction_interval_ms: u64,
    pub mock_latency_ms: u64,
    pub enable_analytics: bool,
    pub initial_prices: Vec<(String, f64)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_port: 8080,
            auction_interval_ms: 500,
            mock_latency_ms: 100,
            enable_analytics: true,
            initial_prices: vec![
                ("ATOM".to_string(), 9.50),
                ("OSMO".to_string(), 0.65),
                ("USDC".to_string(), 1.00),
                ("NTRN".to_string(), 0.45),
                ("STRD".to_string(), 1.20),
                ("TIA".to_string(), 5.25),  // Celestia
            ],
        }
    }
}

fn load_config() -> anyhow::Result<Config> {
    dotenvy::dotenv().ok();

    let config = Config {
        api_port: std::env::var("SKIP_SELECT_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()?,
        auction_interval_ms: std::env::var("AUCTION_INTERVAL_MS")
            .unwrap_or_else(|_| "500".to_string())
            .parse()?,
        mock_latency_ms: std::env::var("MOCK_LATENCY_MS")
            .unwrap_or_else(|_| "100".to_string())
            .parse()?,
        enable_analytics: std::env::var("ENABLE_ANALYTICS")
            .unwrap_or_else(|_| "true".to_string())
            .parse()?,
        initial_prices: Config::default().initial_prices,
    };

    Ok(config)
}
