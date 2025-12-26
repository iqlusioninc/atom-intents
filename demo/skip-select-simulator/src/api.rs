//! REST API handlers

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

use crate::models::*;
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

#[derive(Serialize)]
pub struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
}

pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: 0, // Would track actual uptime in production
    })
}

#[derive(Serialize)]
pub struct IntentResponse {
    pub success: bool,
    pub intent: Intent,
}

/// Generate a synthetic counter-order to enable Intent Matcher competition
/// Returns None if we decide not to generate one (85% of the time)
fn maybe_generate_counter_order(intent: &Intent) -> Option<Intent> {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    // 15% chance to generate a counter-order
    if rng.gen::<f64>() > 0.15 {
        return None;
    }

    // Create a counter-order with swapped denoms and similar amount (80-120%)
    let amount_multiplier = rng.gen_range(0.8..1.2);
    let counter_amount = (intent.output.min_amount as f64 * amount_multiplier) as u128;

    let req = CreateIntentRequest {
        user_address: format!("cosmos1synthetic{:08x}", rng.gen::<u32>()),
        input: Asset {
            chain_id: intent.output.chain_id.clone(),
            denom: intent.output.denom.clone(),
            amount: counter_amount,
        },
        output: OutputSpec {
            chain_id: intent.input.chain_id.clone(),
            denom: intent.input.denom.clone(),
            min_amount: (intent.input.amount as f64 * 0.9) as u128, // 10% slippage tolerance
            max_price: None,
        },
        fill_config: Some(FillConfig::default()),
        constraints: Some(ExecutionConstraints::default()),
        timeout_seconds: Some(60),
    };

    Some(Intent::new(req))
}

pub async fn submit_intent(
    State(state): State<AppStateRef>,
    Json(req): Json<CreateIntentRequest>,
) -> Result<Json<IntentResponse>, (StatusCode, String)> {
    info!("Received intent submission from {}", req.user_address);

    let intent = Intent::new(req);
    let intent_clone = intent.clone();

    // Maybe generate a synthetic counter-order to enable Intent Matcher
    let counter_order = maybe_generate_counter_order(&intent);

    {
        let mut state = state.write().await;
        state.add_intent(intent.clone());
        state.broadcast(WsMessage::IntentSubmitted(intent.clone()));

        // Add counter-order if generated (with small delay for realism)
        if let Some(counter) = counter_order {
            info!("Generated synthetic counter-order for Intent Matcher");
            state.add_intent(counter.clone());
            state.broadcast(WsMessage::IntentSubmitted(counter));
        }
    }

    Ok(Json(IntentResponse {
        success: true,
        intent: intent_clone,
    }))
}

#[derive(Serialize)]
pub struct IntentListResponse {
    pub intents: Vec<Intent>,
    pub total: usize,
}

#[derive(Deserialize)]
pub struct ListIntentsQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn list_intents(
    State(state): State<AppStateRef>,
) -> Json<IntentListResponse> {
    let state = state.read().await;
    let intents: Vec<Intent> = state.intents.values().cloned().collect();
    let total = intents.len();

    Json(IntentListResponse { intents, total })
}

pub async fn get_intent(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<Intent>, StatusCode> {
    let state = state.read().await;
    state
        .intents
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn get_current_auction(
    State(state): State<AppStateRef>,
) -> Result<Json<Auction>, StatusCode> {
    let state = state.read().await;
    if let Some(auction_id) = &state.current_auction_id {
        state
            .auctions
            .get(auction_id)
            .cloned()
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn get_auction(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<Auction>, StatusCode> {
    let state = state.read().await;
    state
        .auctions
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
pub struct QuotesResponse {
    pub auction_id: String,
    pub quotes: Vec<SolverQuote>,
    pub total: usize,
}

pub async fn get_auction_quotes(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<QuotesResponse>, StatusCode> {
    let state = state.read().await;
    let auction = state.auctions.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(QuotesResponse {
        auction_id: id,
        quotes: auction.quotes.clone(),
        total: auction.quotes.len(),
    }))
}

pub async fn get_settlement(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<Settlement>, StatusCode> {
    let state = state.read().await;
    state
        .settlements
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
pub struct PricesResponse {
    pub prices: Vec<PriceFeed>,
    pub updated_at: chrono::DateTime<Utc>,
}

pub async fn get_prices(State(state): State<AppStateRef>) -> Json<PricesResponse> {
    let state = state.read().await;
    let prices: Vec<PriceFeed> = state.prices.values().cloned().collect();

    Json(PricesResponse {
        prices,
        updated_at: Utc::now(),
    })
}

#[derive(Serialize)]
pub struct SolversResponse {
    pub solvers: Vec<Solver>,
    pub total: usize,
}

pub async fn list_solvers(State(state): State<AppStateRef>) -> Json<SolversResponse> {
    let state = state.read().await;
    let solvers: Vec<Solver> = state.solvers.values().cloned().collect();
    let total = solvers.len();

    Json(SolversResponse { solvers, total })
}

pub async fn get_stats(State(state): State<AppStateRef>) -> Json<SystemStats> {
    let state = state.read().await;
    Json(state.stats.clone())
}

// Demo endpoints

#[derive(Serialize)]
pub struct GenerateDemoIntentResponse {
    pub intent: Intent,
    pub description: String,
}

pub async fn generate_demo_intent(
    State(state): State<AppStateRef>,
) -> Json<GenerateDemoIntentResponse> {
    use rand::Rng;

    // Get prices first for calculating realistic min_amount
    let prices = {
        let state = state.read().await;
        state.prices.clone()
    };

    // Generate all random values in a block that ends before await
    let (req, input_denom, output_denom, output_chain, amount) = {
        let mut rng = rand::thread_rng();

        // Generate random demo intent
        let pairs = vec![
            ("ATOM", "OSMO", "cosmoshub-4", "osmosis-1"),
            ("OSMO", "ATOM", "osmosis-1", "cosmoshub-4"),
            ("ATOM", "USDC", "cosmoshub-4", "noble-1"),
            ("USDC", "ATOM", "noble-1", "cosmoshub-4"),
            ("NTRN", "ATOM", "neutron-1", "cosmoshub-4"),
            ("TIA", "USDC", "celestia", "noble-1"),  // Featured: Celestia swap
            ("TIA", "ATOM", "celestia", "cosmoshub-4"),
        ];

        let (input_denom, output_denom, input_chain, output_chain) =
            pairs[rng.gen_range(0..pairs.len())];

        let amount: u128 = rng.gen_range(1_000_000..100_000_000); // 1-100 tokens

        // Calculate expected output based on current exchange rate
        let expected_output = crate::oracle::get_exchange_rate(&prices, input_denom, output_denom)
            .map(|rate| (amount as f64 * rate) as u128)
            .unwrap_or(amount); // Fallback to 1:1 if no price data

        // min_amount is 80% of expected output (20% slippage tolerance for demo)
        let min_amount = expected_output * 80 / 100;

        let req = CreateIntentRequest {
            user_address: format!("cosmos1demo{:08x}", rng.gen::<u32>()),
            input: Asset {
                chain_id: input_chain.to_string(),
                denom: input_denom.to_string(),
                amount,
            },
            output: OutputSpec {
                chain_id: output_chain.to_string(),
                denom: output_denom.to_string(),
                min_amount,
                max_price: None,
            },
            fill_config: Some(FillConfig::default()),
            constraints: Some(ExecutionConstraints::default()),
            timeout_seconds: Some(60),
        };

        (req, input_denom.to_string(), output_denom.to_string(), output_chain.to_string(), amount)
    };
    // rng is dropped here, before the await

    let intent = Intent::new(req);
    let intent_clone = intent.clone();

    // Maybe generate a synthetic counter-order for demo
    let counter_order = maybe_generate_counter_order(&intent);

    {
        let mut state = state.write().await;
        state.add_intent(intent.clone());
        state.broadcast(WsMessage::IntentSubmitted(intent.clone()));

        if let Some(counter) = counter_order {
            state.add_intent(counter.clone());
            state.broadcast(WsMessage::IntentSubmitted(counter));
        }
    }

    Json(GenerateDemoIntentResponse {
        intent: intent_clone,
        description: format!(
            "Demo intent: Swap {} {} for {} on {}",
            amount as f64 / 1_000_000.0,
            input_denom,
            output_denom,
            output_chain
        ),
    })
}

#[derive(Serialize)]
pub struct ScenarioResponse {
    pub scenario: String,
    pub description: String,
    pub intents_created: usize,
    pub intent_ids: Vec<String>,
}

pub async fn run_scenario(
    State(state): State<AppStateRef>,
    Path(name): Path<String>,
) -> Result<Json<ScenarioResponse>, (StatusCode, String)> {
    let scenario = get_scenario(&name).ok_or((
        StatusCode::NOT_FOUND,
        format!("Scenario '{}' not found", name),
    ))?;

    let mut intent_ids = Vec::new();

    for req in scenario.intents {
        let intent = Intent::new(req);
        intent_ids.push(intent.id.clone());

        let mut state = state.write().await;
        state.add_intent(intent.clone());
        state.broadcast(WsMessage::IntentSubmitted(intent));
    }

    Ok(Json(ScenarioResponse {
        scenario: scenario.name,
        description: scenario.description,
        intents_created: intent_ids.len(),
        intent_ids,
    }))
}

fn get_scenario(name: &str) -> Option<DemoScenario> {
    match name {
        "simple_swap" => Some(DemoScenario {
            name: "simple_swap".to_string(),
            description: "Simple ATOM -> OSMO swap via DEX routing".to_string(),
            intents: vec![CreateIntentRequest {
                user_address: "cosmos1demo_alice".to_string(),
                input: Asset {
                    chain_id: "cosmoshub-4".to_string(),
                    denom: "ATOM".to_string(),
                    amount: 10_000_000, // 10 ATOM
                },
                output: OutputSpec {
                    chain_id: "osmosis-1".to_string(),
                    denom: "OSMO".to_string(),
                    min_amount: 140_000_000, // ~14.6 OSMO at current prices
                    max_price: None,
                },
                fill_config: None,
                constraints: None,
                timeout_seconds: Some(60),
            }],
            expected_outcome: "DEX Router fills via Osmosis AMM".to_string(),
        }),
        "tia_usdc_swap" => Some(DemoScenario {
            name: "tia_usdc_swap".to_string(),
            description: "TIA -> USDC cross-chain swap from Celestia (no smart contracts) via Hub escrow".to_string(),
            intents: vec![CreateIntentRequest {
                user_address: "celestia1demo_user".to_string(),
                input: Asset {
                    chain_id: "celestia".to_string(),
                    denom: "TIA".to_string(),
                    amount: 100_000_000, // 100 TIA
                },
                output: OutputSpec {
                    chain_id: "noble-1".to_string(),
                    denom: "USDC".to_string(),
                    min_amount: 500_000_000, // ~$500 USDC (at ~$5/TIA)
                    max_price: None,
                },
                fill_config: None,
                constraints: Some(ExecutionConstraints {
                    max_hops: 3,
                    allowed_venues: vec!["osmosis".to_string()],
                    excluded_venues: vec![],
                    max_slippage_bps: 100,
                }),
                timeout_seconds: Some(60),
            }],
            expected_outcome: "Flow: Celestia -> Hub Escrow (IBC Hooks) -> Solver delivers USDC -> Hub releases TIA. Solver takes relay risk.".to_string(),
        }),
        "intent_matching" => Some(DemoScenario {
            name: "intent_matching".to_string(),
            description: "Two opposing intents matched directly (zero capital)".to_string(),
            intents: vec![
                CreateIntentRequest {
                    user_address: "cosmos1demo_alice".to_string(),
                    input: Asset {
                        chain_id: "cosmoshub-4".to_string(),
                        denom: "ATOM".to_string(),
                        amount: 50_000_000, // 50 ATOM
                    },
                    output: OutputSpec {
                        chain_id: "osmosis-1".to_string(),
                        denom: "OSMO".to_string(),
                        min_amount: 700_000_000, // ~73 OSMO
                        max_price: None,
                    },
                    fill_config: None,
                    constraints: None,
                    timeout_seconds: Some(60),
                },
                CreateIntentRequest {
                    user_address: "cosmos1demo_bob".to_string(),
                    input: Asset {
                        chain_id: "osmosis-1".to_string(),
                        denom: "OSMO".to_string(),
                        amount: 730_000_000, // 73 OSMO
                    },
                    output: OutputSpec {
                        chain_id: "cosmoshub-4".to_string(),
                        denom: "ATOM".to_string(),
                        min_amount: 48_000_000, // 48 ATOM
                        max_price: None,
                    },
                    fill_config: None,
                    constraints: None,
                    timeout_seconds: Some(60),
                },
            ],
            expected_outcome: "Intent Matcher matches Alice and Bob directly".to_string(),
        }),
        "multi_hop" => Some(DemoScenario {
            name: "multi_hop".to_string(),
            description: "Multi-hop settlement via IBC PFM".to_string(),
            intents: vec![CreateIntentRequest {
                user_address: "cosmos1demo_charlie".to_string(),
                input: Asset {
                    chain_id: "cosmoshub-4".to_string(),
                    denom: "ATOM".to_string(),
                    amount: 100_000_000, // 100 ATOM
                },
                output: OutputSpec {
                    chain_id: "neutron-1".to_string(),
                    denom: "NTRN".to_string(),
                    min_amount: 2_000_000_000, // ~2000 NTRN
                    max_price: None,
                },
                fill_config: None,
                constraints: Some(ExecutionConstraints {
                    max_hops: 3,
                    allowed_venues: vec!["osmosis".to_string(), "astroport".to_string()],
                    excluded_venues: vec![],
                    max_slippage_bps: 150,
                }),
                timeout_seconds: Some(120),
            }],
            expected_outcome: "Route: Hub -> Osmosis (swap) -> Neutron via PFM".to_string(),
        }),
        "cex_backstop" => Some(DemoScenario {
            name: "cex_backstop".to_string(),
            description: "Large order using CEX backstop liquidity".to_string(),
            intents: vec![CreateIntentRequest {
                user_address: "cosmos1demo_whale".to_string(),
                input: Asset {
                    chain_id: "cosmoshub-4".to_string(),
                    denom: "ATOM".to_string(),
                    amount: 500_000_000_000, // 500,000 ATOM (whale)
                },
                output: OutputSpec {
                    chain_id: "noble-1".to_string(),
                    denom: "USDC".to_string(),
                    min_amount: 4_500_000_000_000, // $4.5M USDC
                    max_price: None,
                },
                fill_config: Some(FillConfig {
                    allow_partial: true,
                    min_fill_percent: 50,
                    strategy: FillStrategy::Eager,
                }),
                constraints: None,
                timeout_seconds: Some(300),
            }],
            expected_outcome: "CEX Backstop provides deep liquidity for large order".to_string(),
        }),
        "auction_competition" => Some(DemoScenario {
            name: "auction_competition".to_string(),
            description: "Multiple solvers competing for best execution".to_string(),
            intents: vec![
                CreateIntentRequest {
                    user_address: "cosmos1demo_user1".to_string(),
                    input: Asset {
                        chain_id: "cosmoshub-4".to_string(),
                        denom: "ATOM".to_string(),
                        amount: 25_000_000, // 25 ATOM
                    },
                    output: OutputSpec {
                        chain_id: "osmosis-1".to_string(),
                        denom: "USDC".to_string(),
                        min_amount: 230_000_000, // ~$230
                        max_price: None,
                    },
                    fill_config: None,
                    constraints: None,
                    timeout_seconds: Some(60),
                },
                CreateIntentRequest {
                    user_address: "cosmos1demo_user2".to_string(),
                    input: Asset {
                        chain_id: "cosmoshub-4".to_string(),
                        denom: "ATOM".to_string(),
                        amount: 15_000_000, // 15 ATOM
                    },
                    output: OutputSpec {
                        chain_id: "osmosis-1".to_string(),
                        denom: "USDC".to_string(),
                        min_amount: 138_000_000, // ~$138
                        max_price: None,
                    },
                    fill_config: None,
                    constraints: None,
                    timeout_seconds: Some(60),
                },
            ],
            expected_outcome: "Batch auction with uniform clearing price".to_string(),
        }),
        _ => None,
    }
}
