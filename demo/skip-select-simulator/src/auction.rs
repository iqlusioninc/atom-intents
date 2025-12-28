//! Batch auction engine for intent execution

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use crate::models::*;
use crate::oracle;
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

/// Run the main auction loop
pub async fn run_auction_loop(state: AppStateRef, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;
        run_auction_cycle(&state).await;
    }
}

/// Execute one auction cycle
async fn run_auction_cycle(state: &AppStateRef) {
    // Get pending intents
    let pending_intents = {
        let state = state.read().await;
        state.get_pending_intents()
    };

    if pending_intents.is_empty() {
        return;
    }

    info!("Starting auction with {} pending intents", pending_intents.len());

    // Create auction
    let intent_ids: Vec<String> = pending_intents.iter().map(|i| i.id.clone()).collect();
    let mut auction = Auction::new(intent_ids.clone());
    auction.stats.num_intents = pending_intents.len();

    // Update state with new auction
    {
        let mut state = state.write().await;
        state.current_auction_id = Some(auction.id.clone());
        state.auctions.insert(auction.id.clone(), auction.clone());

        // Mark intents as in auction
        for intent_id in &intent_ids {
            if let Some(intent) = state.intents.get_mut(intent_id) {
                intent.status = IntentStatus::InAuction;
                intent.auction_id = Some(auction.id.clone());
            }
        }

        state.stats.total_auctions += 1;
        state.broadcast(WsMessage::AuctionStarted(auction.clone()));
    }

    // Simulate quote collection phase
    let quotes = collect_quotes(state, &auction.id, &pending_intents).await;

    // Update auction with quotes
    {
        let mut state = state.write().await;
        if let Some(auction) = state.auctions.get_mut(&auction.id) {
            auction.status = AuctionStatus::Collecting;
            auction.quotes = quotes.clone();
            auction.stats.num_quotes = quotes.len();
        }
    }

    // Run clearing algorithm
    let clearing_result = run_clearing_algorithm(&quotes, &pending_intents);

    // Complete auction
    {
        let mut state = state.write().await;
        let auction_to_broadcast = if let Some(auction) = state.auctions.get_mut(&auction.id) {
            auction.status = AuctionStatus::Completed;
            auction.completed_at = Some(Utc::now());
            auction.winning_quote = clearing_result.winning_quote.clone();
            auction.clearing_price = clearing_result.clearing_price;
            auction.stats = clearing_result.stats.clone();
            Some(auction.clone())
        } else {
            None
        };

        // Broadcast completion outside of the mutable borrow
        if let Some(auction_clone) = auction_to_broadcast {
            state.broadcast(WsMessage::AuctionCompleted(auction_clone));
        }

        // Update intents based on result
        if let Some(winning_quote) = &clearing_result.winning_quote {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            for intent_id in &winning_quote.intent_ids {
                if let Some(intent) = state.intents.get_mut(intent_id) {
                    // Simulate partial fills for orders that allow it (30% chance)
                    let is_partial_fill = intent.fill_config.allow_partial
                        && intent.fill_config.strategy != FillStrategy::AllOrNothing
                        && rng.gen_bool(0.30);

                    if is_partial_fill {
                        // Generate a fill percentage between min_fill_percent and 95%
                        let min_pct = intent.fill_config.min_fill_percent.max(50) as f64;
                        let fill_pct = rng.gen_range(min_pct..95.0) as u8;
                        let filled_amount = (intent.input.amount as f64 * (fill_pct as f64 / 100.0)) as u128;
                        let remaining_amount = intent.input.amount - filled_amount;

                        intent.status = IntentStatus::PartiallyFilled;
                        intent.filled_amount = filled_amount;
                        intent.remaining_amount = remaining_amount;
                        intent.fill_percentage = fill_pct;
                    } else {
                        intent.status = IntentStatus::Matched;
                        intent.filled_amount = intent.input.amount;
                        intent.remaining_amount = 0;
                        intent.fill_percentage = 100;
                    }
                }
            }

            // Collect partial fill info and intent data for settlement creation
            let (is_partial, fill_pct, original_amount, intent_data) = {
                winning_quote.intent_ids.first()
                    .and_then(|id| state.intents.get(id))
                    .map(|intent| (
                        intent.status == IntentStatus::PartiallyFilled,
                        intent.fill_percentage,
                        intent.input.amount,
                        Some((
                            intent.user_address.clone(),
                            intent.input.chain_id.clone(),
                            intent.input.denom.clone(),
                            intent.output.chain_id.clone(),
                            intent.output.denom.clone(),
                        ))
                    ))
                    .unwrap_or((false, 100, winning_quote.input_amount, None))
            };

            // Extract intent fields with defaults
            let (user_address, input_chain, input_denom, output_chain, output_denom) =
                intent_data.unwrap_or_else(|| (
                    String::new(),
                    String::new(),
                    "uatom".to_string(),
                    String::new(),
                    "uosmo".to_string(),
                ));

            // Calculate actual settlement amounts for partial fills
            let (settlement_input, settlement_output) = if is_partial {
                let input = (winning_quote.input_amount as f64 * (fill_pct as f64 / 100.0)) as u128;
                let output = (winning_quote.output_amount as f64 * (fill_pct as f64 / 100.0)) as u128;
                (input, output)
            } else {
                (winning_quote.input_amount, winning_quote.output_amount)
            };

            // Create settlement
            let settlement_description = if is_partial {
                format!("Partial fill settlement ({}%) created from auction", fill_pct)
            } else {
                "Settlement created from auction".to_string()
            };

            let settlement = Settlement {
                id: format!("settlement_{}", Uuid::new_v4()),
                auction_id: auction.id.clone(),
                intent_ids: winning_quote.intent_ids.clone(),
                solver_id: winning_quote.solver_id.clone(),
                status: SettlementStatus::Pending,
                phase: SettlementPhase::Init,
                input_amount: settlement_input,
                output_amount: settlement_output,
                escrow_txid: None,
                execution_txid: None,
                ibc_packet_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                completed_at: None,
                events: vec![SettlementEvent {
                    event_type: "created".to_string(),
                    timestamp: Utc::now(),
                    description: settlement_description,
                    metadata: serde_json::json!({
                        "auction_id": auction.id,
                        "solver_id": winning_quote.solver_id,
                        "is_partial_fill": is_partial,
                        "fill_percentage": fill_pct,
                    }),
                }],
                is_partial_fill: is_partial,
                fill_percentage: fill_pct,
                original_input_amount: original_amount,
                // Intent-derived fields for settlement execution
                user_address: user_address.clone(),
                user_output_address: user_address, // Same address for now, could differ for cross-chain
                input_chain_id: input_chain,
                input_denom,
                output_chain_id: output_chain,
                output_denom,
            };

            for intent_id in &winning_quote.intent_ids {
                if let Some(intent) = state.intents.get_mut(intent_id) {
                    intent.settlement_id = Some(settlement.id.clone());
                }
            }

            state.settlements.insert(settlement.id.clone(), settlement.clone());
            state.stats.total_settlements += 1;
            state.broadcast(WsMessage::SettlementUpdate(settlement));
        } else {
            // No winning quote - return intents to pending
            for intent_id in &intent_ids {
                if let Some(intent) = state.intents.get_mut(intent_id) {
                    if intent.is_expired() {
                        intent.status = IntentStatus::Expired;
                    } else {
                        intent.status = IntentStatus::Pending;
                        intent.auction_id = None;
                    }
                }
            }
        }

        state.update_stats();
    }
}

/// Find a matching counter-order for Intent Matcher
/// Two intents match when: A wants X→Y, B wants Y→X, amounts within 20%
fn find_matching_intent<'a>(intent: &Intent, all_intents: &'a [Intent]) -> Option<&'a Intent> {
    all_intents.iter().find(|other| {
        if other.id == intent.id {
            return false;
        }

        // Check if denoms are swapped (counter-order)
        let denoms_match = other.input.denom == intent.output.denom
            && other.output.denom == intent.input.denom;

        if !denoms_match {
            return false;
        }

        // Check if amounts are within 20% of each other
        let intent_value = intent.input.amount as f64;
        let other_value = other.input.amount as f64;
        let ratio = intent_value / other_value;
        (0.8..=1.25).contains(&ratio)
    })
}

/// Collect quotes from mock solvers
async fn collect_quotes(
    state: &AppStateRef,
    auction_id: &str,
    intents: &[Intent],
) -> Vec<SolverQuote> {
    let mut quotes = Vec::new();

    let (solvers, prices) = {
        let state = state.read().await;
        (state.solvers.clone(), state.prices.clone())
    };

    for solver in solvers.values() {
        if solver.status != SolverStatus::Active {
            continue;
        }

        // Simulate solver latency
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Generate quotes for compatible intents
        for intent in intents {
            // Check if solver supports this trading pair
            if !solver.supported_denoms.contains(&intent.input.denom)
                || !solver.supported_denoms.contains(&intent.output.denom)
            {
                continue;
            }

            // Special handling for Intent Matcher - only quote if counter-order exists
            if solver.solver_type == SolverType::IntentMatcher {
                let matching_intent = find_matching_intent(intent, intents);
                if matching_intent.is_none() {
                    continue; // No counter-order, Intent Matcher can't participate
                }

                // Generate quote with "Direct match" advantage
                if let Some(mut quote) = generate_solver_quote(solver, intent, &prices, matching_intent) {
                    quote.advantage_reason = Some("Direct match".to_string());
                    {
                        let state = state.read().await;
                        state.broadcast(WsMessage::QuoteReceived(quote.clone()));
                    }
                    quotes.push(quote);
                }
                continue;
            }

            // Calculate quote based on solver type
            let quote = generate_solver_quote(solver, intent, &prices, None);
            if let Some(quote) = quote {
                // Broadcast quote received
                {
                    let state = state.read().await;
                    state.broadcast(WsMessage::QuoteReceived(quote.clone()));
                }
                quotes.push(quote);
            }
        }
    }

    debug!("Collected {} quotes for auction {}", quotes.len(), auction_id);
    quotes
}

/// Calculate advantage score for a solver on a given intent
/// Returns (total_score, highest_scoring_dimension)
fn calculate_advantage_score(
    solver: &Solver,
    intent: &Intent,
    order_value_usd: f64,
) -> (f64, Option<String>) {
    let profile = &solver.advantage_profile;

    // Pair score (0.0 - 0.4)
    let pair_score: f64 = if profile.preferred_pairs.iter().any(|(input, output)| {
        input == &intent.input.denom && output == &intent.output.denom
    }) {
        0.4 // Exact match
    } else if profile.preferred_pairs.iter().any(|(input, _)| {
        input == &intent.input.denom || input == &intent.output.denom
    }) {
        0.2 // Partial match (one side of pair)
    } else {
        0.05 // No match - small base score
    };

    // Size score (0.0 - 0.3)
    // Thresholds: Small <$500, Medium $500-$5K, Large >$5K
    let order_size = if order_value_usd < 500.0 {
        SizePreference::Small
    } else if order_value_usd < 5000.0 {
        SizePreference::Medium
    } else {
        SizePreference::Large
    };

    let size_score: f64 = match (&profile.size_preference, &order_size) {
        (SizePreference::Any, _) => 0.15, // Neutral
        (pref, actual) if pref == actual => 0.3, // Perfect match
        (SizePreference::Medium, SizePreference::Small) => 0.2, // Adjacent
        (SizePreference::Medium, SizePreference::Large) => 0.2, // Adjacent
        (SizePreference::Small, SizePreference::Medium) => 0.15, // Adjacent
        (SizePreference::Large, SizePreference::Medium) => 0.15, // Adjacent
        _ => 0.05, // Mismatch
    };

    // Chain score (0.0 - 0.3)
    let is_cross_chain = intent.input.chain_id != intent.output.chain_id;
    let chain_score: f64 = if profile.chain_specialty.contains(&intent.input.chain_id)
        || profile.chain_specialty.contains(&intent.output.chain_id)
    {
        if is_cross_chain { 0.3 } else { 0.25 }
    } else if profile.chain_specialty.is_empty() {
        0.1 // No specialty means chain-agnostic
    } else {
        0.05 // Has specialty but not for this chain
    };

    // Determine highest scoring dimension for the tag
    let mut dimensions = vec![
        (pair_score, "Native pair"),
        (size_score, "Size specialist"),
    ];
    if is_cross_chain {
        dimensions.push((chain_score, "Cross-chain expert"));
    }

    let highest = dimensions
        .iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(score, reason)| (*score, reason.to_string()));

    let total_score: f64 = (pair_score + size_score + chain_score).min(1.0);

    // Only return a reason if the score is meaningful (>0.5)
    let reason = if total_score > 0.5 {
        highest.map(|(_, r)| r)
    } else {
        None
    };

    (total_score, reason)
}

fn generate_solver_quote(
    solver: &Solver,
    intent: &Intent,
    prices: &std::collections::HashMap<String, PriceFeed>,
    _matching_intent: Option<&Intent>, // For Intent Matcher counter-order detection
) -> Option<SolverQuote> {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    // Get exchange rate
    let rate = oracle::get_exchange_rate(prices, &intent.input.denom, &intent.output.denom)?;

    // Calculate order value in USD for size-based advantages
    let input_price = prices.get(&intent.input.denom).map(|p| p.price_usd).unwrap_or(1.0);
    let order_value_usd = (intent.input.amount as f64 / 1_000_000.0) * input_price;

    // Calculate advantage score
    let (advantage_score, mut advantage_reason) = calculate_advantage_score(solver, intent, order_value_usd);

    // Apply solver-specific spread, influenced by advantage score
    // Higher advantage = tighter spread = better price
    let (spread_min, spread_max) = match solver.solver_type {
        SolverType::IntentMatcher => (0.0, 0.0), // No spread for direct matching
        SolverType::DexRouter => (0.001, 0.005), // 0.1-0.5%
        SolverType::CexBackstop => (0.003, 0.008), // 0.3-0.8%
        SolverType::Hybrid => (0.0015, 0.004), // 0.15-0.4%
    };

    // Advantage score reduces spread: high score → spread near minimum
    let spread_range = spread_max - spread_min;
    let base_spread = spread_max - (advantage_score * spread_range);

    // Add small noise for competition (±0.05%)
    let noise = rng.gen_range(-0.0005..0.0005);
    let spread = (base_spread + noise).max(spread_min).min(spread_max);

    let effective_rate = rate * (1.0 - spread);
    let output_amount = (intent.input.amount as f64 * effective_rate) as u128;

    // Check if output meets minimum
    if output_amount < intent.output.min_amount {
        return None;
    }

    // Generate execution plan
    let execution_plan = generate_execution_plan(solver, intent);

    // If no specific advantage but still competitive, use generic reason
    if advantage_reason.is_none() && advantage_score > 0.3 {
        advantage_reason = Some("Best execution".to_string());
    }

    Some(SolverQuote {
        id: format!("quote_{}", Uuid::new_v4()),
        solver_id: solver.id.clone(),
        solver_name: solver.name.clone(),
        solver_type: solver.solver_type.clone(),
        intent_ids: vec![intent.id.clone()],
        input_amount: intent.input.amount,
        output_amount,
        effective_price: effective_rate,
        execution_plan,
        estimated_gas: rng.gen_range(200_000..500_000),
        confidence: solver.reputation_score * rng.gen_range(0.9..1.0),
        submitted_at: Utc::now(),
        advantage_reason,
    })
}

fn generate_execution_plan(solver: &Solver, intent: &Intent) -> ExecutionPlan {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    match solver.solver_type {
        SolverType::IntentMatcher => ExecutionPlan {
            plan_type: ExecutionPlanType::DirectMatch,
            steps: vec![ExecutionStep {
                step_type: "direct_transfer".to_string(),
                chain_id: intent.output.chain_id.clone(),
                venue: None,
                input_denom: intent.input.denom.clone(),
                output_denom: intent.output.denom.clone(),
                amount: intent.input.amount,
                description: "Direct match with opposing intent".to_string(),
            }],
            estimated_duration_ms: 1500,
        },
        SolverType::DexRouter => {
            let needs_ibc = intent.input.chain_id != intent.output.chain_id;
            let steps = if needs_ibc {
                vec![
                    ExecutionStep {
                        step_type: "ibc_transfer".to_string(),
                        chain_id: intent.input.chain_id.clone(),
                        venue: None,
                        input_denom: intent.input.denom.clone(),
                        output_denom: intent.input.denom.clone(),
                        amount: intent.input.amount,
                        description: format!(
                            "IBC transfer to {}",
                            intent.output.chain_id
                        ),
                    },
                    ExecutionStep {
                        step_type: "swap".to_string(),
                        chain_id: intent.output.chain_id.clone(),
                        venue: Some("osmosis_amm".to_string()),
                        input_denom: intent.input.denom.clone(),
                        output_denom: intent.output.denom.clone(),
                        amount: intent.input.amount,
                        description: format!(
                            "Swap {} -> {} on Osmosis",
                            intent.input.denom, intent.output.denom
                        ),
                    },
                ]
            } else {
                vec![ExecutionStep {
                    step_type: "swap".to_string(),
                    chain_id: intent.output.chain_id.clone(),
                    venue: Some("osmosis_amm".to_string()),
                    input_denom: intent.input.denom.clone(),
                    output_denom: intent.output.denom.clone(),
                    amount: intent.input.amount,
                    description: format!(
                        "Swap {} -> {}",
                        intent.input.denom, intent.output.denom
                    ),
                }]
            };

            ExecutionPlan {
                plan_type: if needs_ibc {
                    ExecutionPlanType::MultiHop
                } else {
                    ExecutionPlanType::DexRoute
                },
                steps,
                estimated_duration_ms: rng.gen_range(2000..4000),
            }
        }
        SolverType::CexBackstop => ExecutionPlan {
            plan_type: ExecutionPlanType::CexHedge,
            steps: vec![
                ExecutionStep {
                    step_type: "escrow".to_string(),
                    chain_id: intent.input.chain_id.clone(),
                    venue: None,
                    input_denom: intent.input.denom.clone(),
                    output_denom: intent.input.denom.clone(),
                    amount: intent.input.amount,
                    description: "Lock funds in escrow".to_string(),
                },
                ExecutionStep {
                    step_type: "cex_hedge".to_string(),
                    chain_id: "cex".to_string(),
                    venue: Some("binance".to_string()),
                    input_denom: intent.input.denom.clone(),
                    output_denom: intent.output.denom.clone(),
                    amount: intent.input.amount,
                    description: "Hedge on CEX".to_string(),
                },
                ExecutionStep {
                    step_type: "delivery".to_string(),
                    chain_id: intent.output.chain_id.clone(),
                    venue: None,
                    input_denom: intent.output.denom.clone(),
                    output_denom: intent.output.denom.clone(),
                    amount: 0, // Will be filled in
                    description: "Deliver output to user".to_string(),
                },
            ],
            estimated_duration_ms: rng.gen_range(3000..5000),
        },
        SolverType::Hybrid => ExecutionPlan {
            plan_type: ExecutionPlanType::DexRoute,
            steps: vec![ExecutionStep {
                step_type: "hybrid_fill".to_string(),
                chain_id: intent.output.chain_id.clone(),
                venue: Some("multiple".to_string()),
                input_denom: intent.input.denom.clone(),
                output_denom: intent.output.denom.clone(),
                amount: intent.input.amount,
                description: "Split between DEX and inventory".to_string(),
            }],
            estimated_duration_ms: rng.gen_range(2500..3500),
        },
    }
}

struct ClearingResult {
    winning_quote: Option<SolverQuote>,
    clearing_price: Option<f64>,
    stats: AuctionStats,
}

fn run_clearing_algorithm(quotes: &[SolverQuote], intents: &[Intent]) -> ClearingResult {
    if quotes.is_empty() {
        return ClearingResult {
            winning_quote: None,
            clearing_price: None,
            stats: AuctionStats::default(),
        };
    }

    // Sort quotes by effective price (best price first)
    let mut sorted_quotes = quotes.to_vec();
    sorted_quotes.sort_by(|a, b| {
        b.effective_price
            .partial_cmp(&a.effective_price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Select winning quote (best price that meets requirements)
    let winning_quote = sorted_quotes.into_iter().find(|q| {
        // Check if quote meets all intent requirements
        q.intent_ids.iter().all(|id| {
            intents
                .iter()
                .find(|i| &i.id == id)
                .is_some_and(|intent| q.output_amount >= intent.output.min_amount)
        })
    });

    let clearing_price = winning_quote.as_ref().map(|q| q.effective_price);

    // Calculate stats
    let total_input: u128 = intents.iter().map(|i| i.input.amount).sum();
    let total_output: u128 = winning_quote
        .as_ref()
        .map(|q| q.output_amount)
        .unwrap_or(0);

    let matched_volume = winning_quote
        .as_ref()
        .map(|q| q.input_amount)
        .unwrap_or(0);

    // Calculate price improvement (vs worst quote)
    let price_improvement_bps = if let (Some(best), Some(worst)) = (
        quotes.iter().max_by(|a, b| {
            a.effective_price
                .partial_cmp(&b.effective_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        quotes.iter().min_by(|a, b| {
            a.effective_price
                .partial_cmp(&b.effective_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    ) {
        ((best.effective_price - worst.effective_price) / worst.effective_price * 10000.0) as i32
    } else {
        0
    };

    // Solver competition score (how many unique solvers)
    let unique_solvers: std::collections::HashSet<_> =
        quotes.iter().map(|q| &q.solver_id).collect();
    let competition_score = (unique_solvers.len() as f64).min(1.0);

    ClearingResult {
        winning_quote,
        clearing_price,
        stats: AuctionStats {
            num_intents: intents.len(),
            num_quotes: quotes.len(),
            total_input_amount: total_input,
            total_output_amount: total_output,
            matched_volume,
            price_improvement_bps,
            solver_competition_score: competition_score,
        },
    }
}
