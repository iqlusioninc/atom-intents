//! Skip Select Simulator - Main entry point
//!
//! A simplified version of the Skip Select coordination layer for the ATOM Intents demo.
//! Provides REST API, WebSocket, and batch auction functionality.
//!
//! Supports three execution modes:
//! - `simulated` (default): Pure in-memory simulation, no blockchain
//! - `testnet`: Connected to real Cosmos testnets
//! - `localnet`: Connected to local docker chains

mod api;
mod auction;
mod backend;
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
use clap::Parser;
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::backend::{BackendMode, ExecutionBackend};
use crate::backend::simulated::SimulatedBackend;
use crate::backend::testnet::TestnetBackend;
use crate::state::AppState;

/// Skip Select Simulator CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Execution mode: simulated, testnet, or localnet
    #[arg(long, default_value = "simulated")]
    mode: String,

    /// Path to testnet/localnet config file (required for non-simulated modes)
    #[arg(long)]
    config: Option<String>,

    /// API server port
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Auction interval in milliseconds
    #[arg(long, default_value = "500")]
    auction_interval: u64,

    /// Mock latency in milliseconds (simulated mode only)
    #[arg(long, default_value = "100")]
    mock_latency: u64,
}

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

    // Parse CLI arguments
    let args = Args::parse();

    info!("Starting Skip Select Simulator");
    info!("  Mode: {}", args.mode);
    info!("  Port: {}", args.port);

    // Initialize execution backend based on mode
    let backend: Arc<dyn ExecutionBackend> = match args.mode.as_str() {
        "simulated" => {
            info!("🎭 Running in SIMULATED mode (no blockchain)");
            Arc::new(SimulatedBackend::with_settings(
                (args.mock_latency, args.mock_latency * 3),
                0.95,
            ))
        }
        "testnet" => {
            let config_path = args.config.as_ref().ok_or_else(|| {
                anyhow::anyhow!("--config is required for testnet mode")
            })?;

            info!("🌐 Running in TESTNET mode");
            info!("  Config: {}", config_path);

            match TestnetBackend::from_config_file(config_path).await {
                Ok(backend) => {
                    if let Some(addrs) = backend.contract_addresses() {
                        info!("  Settlement contract: {}", addrs.settlement);
                        info!("  Escrow contract: {}", addrs.escrow);
                    }
                    Arc::new(backend)
                }
                Err(e) => {
                    error!("Failed to initialize testnet backend: {}", e);
                    return Err(anyhow::anyhow!("Testnet initialization failed: {}", e));
                }
            }
        }
        "localnet" => {
            info!("🏠 Running in LOCALNET mode");

            match TestnetBackend::localnet().await {
                Ok(backend) => {
                    if let Some(addrs) = backend.contract_addresses() {
                        info!("  Settlement contract: {}", addrs.settlement);
                        info!("  Escrow contract: {}", addrs.escrow);
                    }
                    Arc::new(backend)
                }
                Err(e) => {
                    warn!("Failed to connect to localnet: {}", e);
                    warn!("Falling back to simulated mode");
                    Arc::new(SimulatedBackend::new())
                }
            }
        }
        other => {
            error!("Unknown mode: {}. Use simulated, testnet, or localnet", other);
            return Err(anyhow::anyhow!("Unknown mode: {}", other));
        }
    };

    // Load configuration
    let config = Config {
        api_port: args.port,
        auction_interval_ms: args.auction_interval,
        mock_latency_ms: args.mock_latency,
        enable_analytics: true,
        initial_prices: Config::default().initial_prices,
    };
    info!("Configuration loaded: {:?}", config);

    // Display mode information
    match backend.mode() {
        BackendMode::Simulated => {
            info!("📊 All settlements will be simulated in-memory");
        }
        BackendMode::Testnet { chain_id, .. } => {
            info!("📊 Settlements will execute on testnet: {}", chain_id);
        }
        BackendMode::Localnet { .. } => {
            info!("📊 Settlements will execute on local docker chains");
        }
    }

    // Initialize application state with backend
    let state = Arc::new(RwLock::new(AppState::new_with_backend(config.clone(), backend.clone())));

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
    let settlement_backend = backend.clone();
    tokio::spawn(async move {
        settlement::run_settlement_processor_with_backend(settlement_state, settlement_backend).await;
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
        // Mode endpoint (shows testnet/simulated status)
        .route("/api/v1/mode", get(api::get_mode))
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
