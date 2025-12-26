//! Application state management

use std::collections::HashMap;

use chrono::Utc;
use tokio::sync::broadcast;

use crate::models::*;
use crate::Config;

/// Central application state
pub struct AppState {
    pub config: Config,
    pub intents: HashMap<String, Intent>,
    pub auctions: HashMap<String, Auction>,
    pub settlements: HashMap<String, Settlement>,
    pub solvers: HashMap<String, Solver>,
    pub prices: HashMap<String, PriceFeed>,
    pub stats: SystemStats,
    pub current_auction_id: Option<String>,
    pub ws_broadcast: broadcast::Sender<WsMessage>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (ws_broadcast, _) = broadcast::channel(1000);

        // Initialize price feeds
        let mut prices = HashMap::new();
        for (denom, price) in &config.initial_prices {
            prices.insert(
                denom.clone(),
                PriceFeed {
                    denom: denom.clone(),
                    price_usd: *price,
                    change_24h: 0.0,
                    volume_24h: 0.0,
                    confidence: 0.99,
                    updated_at: Utc::now(),
                },
            );
        }

        // Initialize mock solvers with advantage profiles
        let mut solvers = HashMap::new();
        let mock_solvers = vec![
            Solver {
                id: "solver_dex_osmosis".to_string(),
                name: "Osmosis DEX Router".to_string(),
                solver_type: SolverType::DexRouter,
                status: SolverStatus::Active,
                reputation_score: 0.95,
                total_volume: 0,
                success_rate: 0.98,
                avg_execution_time_ms: 2500,
                supported_chains: vec![
                    "cosmoshub-4".to_string(),
                    "osmosis-1".to_string(),
                    "neutron-1".to_string(),
                ],
                supported_denoms: vec![
                    "ATOM".to_string(),
                    "OSMO".to_string(),
                    "USDC".to_string(),
                    "NTRN".to_string(),
                ],
                connected_at: Some(Utc::now()),
                advantage_profile: SolverAdvantageProfile {
                    preferred_pairs: vec![
                        ("ATOM".to_string(), "OSMO".to_string()),
                        ("OSMO".to_string(), "ATOM".to_string()),
                        ("OSMO".to_string(), "USDC".to_string()),
                        ("USDC".to_string(), "OSMO".to_string()),
                    ],
                    size_preference: SizePreference::Medium, // Good for small-medium orders
                    chain_specialty: vec!["osmosis-1".to_string()],
                },
            },
            Solver {
                id: "solver_intent_matcher".to_string(),
                name: "Intent Matcher".to_string(),
                solver_type: SolverType::IntentMatcher,
                status: SolverStatus::Active,
                reputation_score: 0.92,
                total_volume: 0,
                success_rate: 0.99,
                avg_execution_time_ms: 1500,
                supported_chains: vec![
                    "cosmoshub-4".to_string(),
                    "osmosis-1".to_string(),
                ],
                supported_denoms: vec![
                    "ATOM".to_string(),
                    "OSMO".to_string(),
                    "USDC".to_string(),
                ],
                connected_at: Some(Utc::now()),
                advantage_profile: SolverAdvantageProfile {
                    preferred_pairs: vec![], // Any pair when counter-order exists
                    size_preference: SizePreference::Medium, // Best for medium orders
                    chain_specialty: vec![], // No chain preference
                },
            },
            Solver {
                id: "solver_cex_backstop".to_string(),
                name: "CEX Backstop".to_string(),
                solver_type: SolverType::CexBackstop,
                status: SolverStatus::Active,
                reputation_score: 0.88,
                total_volume: 0,
                success_rate: 0.97,
                avg_execution_time_ms: 3500,
                supported_chains: vec!["cosmoshub-4".to_string()],
                supported_denoms: vec![
                    "ATOM".to_string(),
                    "USDC".to_string(),
                ],
                connected_at: Some(Utc::now()),
                advantage_profile: SolverAdvantageProfile {
                    preferred_pairs: vec![
                        ("ATOM".to_string(), "USDC".to_string()),
                        ("USDC".to_string(), "ATOM".to_string()),
                        ("USDC".to_string(), "USDT".to_string()),
                    ],
                    size_preference: SizePreference::Large, // Deep CEX liquidity for large orders
                    chain_specialty: vec![], // Chain-agnostic via off-chain
                },
            },
            Solver {
                id: "solver_astroport".to_string(),
                name: "Astroport Router".to_string(),
                solver_type: SolverType::DexRouter,
                status: SolverStatus::Active,
                reputation_score: 0.91,
                total_volume: 0,
                success_rate: 0.96,
                avg_execution_time_ms: 2800,
                supported_chains: vec![
                    "neutron-1".to_string(),
                    "injective-1".to_string(),
                ],
                supported_denoms: vec![
                    "NTRN".to_string(),
                    "ATOM".to_string(),
                    "USDC".to_string(),
                ],
                connected_at: Some(Utc::now()),
                advantage_profile: SolverAdvantageProfile {
                    preferred_pairs: vec![
                        ("NTRN".to_string(), "ATOM".to_string()),
                        ("ATOM".to_string(), "NTRN".to_string()),
                        ("NTRN".to_string(), "USDC".to_string()),
                    ],
                    size_preference: SizePreference::Medium,
                    chain_specialty: vec!["neutron-1".to_string()],
                },
            },
            // Celestia cross-chain solver (Hub escrow + relay risk)
            Solver {
                id: "solver_celestia_bridge".to_string(),
                name: "Celestia Bridge Solver".to_string(),
                solver_type: SolverType::Hybrid,
                status: SolverStatus::Active,
                reputation_score: 0.94,
                total_volume: 0,
                success_rate: 0.97,
                avg_execution_time_ms: 13000, // ~13s for cross-chain via Hub escrow
                supported_chains: vec![
                    "celestia".to_string(),
                    "cosmoshub-4".to_string(),
                    "osmosis-1".to_string(),
                    "noble-1".to_string(),
                ],
                supported_denoms: vec![
                    "TIA".to_string(),
                    "ATOM".to_string(),
                    "USDC".to_string(),
                    "OSMO".to_string(),
                ],
                connected_at: Some(Utc::now()),
                advantage_profile: SolverAdvantageProfile {
                    preferred_pairs: vec![
                        ("TIA".to_string(), "USDC".to_string()),
                        ("TIA".to_string(), "ATOM".to_string()),
                        ("TIA".to_string(), "OSMO".to_string()),
                        ("ATOM".to_string(), "TIA".to_string()),
                    ],
                    size_preference: SizePreference::Medium,
                    chain_specialty: vec!["celestia".to_string()],
                },
            },
        ];

        for solver in mock_solvers {
            solvers.insert(solver.id.clone(), solver);
        }

        Self {
            config,
            intents: HashMap::new(),
            auctions: HashMap::new(),
            settlements: HashMap::new(),
            solvers,
            prices,
            stats: SystemStats::default(),
            current_auction_id: None,
            ws_broadcast,
        }
    }

    pub fn add_intent(&mut self, intent: Intent) {
        self.stats.total_intents += 1;
        self.stats.pending_intents += 1;
        self.intents.insert(intent.id.clone(), intent);
    }

    pub fn get_pending_intents(&self) -> Vec<Intent> {
        self.intents
            .values()
            .filter(|i| i.status == IntentStatus::Pending && !i.is_expired())
            .cloned()
            .collect()
    }

    pub fn get_price(&self, denom: &str) -> Option<f64> {
        self.prices.get(denom).map(|p| p.price_usd)
    }

    pub fn broadcast(&self, msg: WsMessage) {
        let _ = self.ws_broadcast.send(msg);
    }

    pub fn update_stats(&mut self) {
        // Count intents
        self.stats.total_intents = self.intents.len() as u64;
        self.stats.pending_intents = self
            .intents
            .values()
            .filter(|i| i.status == IntentStatus::Pending)
            .count() as u64;

        // Count auctions
        self.stats.total_auctions = self.auctions.len() as u64;

        // Count active solvers
        self.stats.active_solvers = self
            .solvers
            .values()
            .filter(|s| s.status == SolverStatus::Active)
            .count() as u64;

        // Calculate success rate from settlements
        self.stats.total_settlements = self.settlements.len() as u64;
        let completed = self
            .settlements
            .values()
            .filter(|s| s.status == SettlementStatus::Completed)
            .count() as f64;
        let total_finished = self
            .settlements
            .values()
            .filter(|s| {
                s.status == SettlementStatus::Completed
                    || s.status == SettlementStatus::Failed
                    || s.status == SettlementStatus::Refunded
            })
            .count() as f64;

        self.stats.success_rate = if total_finished > 0.0 {
            completed / total_finished
        } else {
            0.0
        };

        // Calculate avg execution time from completed settlements
        let completed_settlements: Vec<_> = self
            .settlements
            .values()
            .filter(|s| s.status == SettlementStatus::Completed && s.completed_at.is_some())
            .collect();

        if !completed_settlements.is_empty() {
            let total_exec_time: i64 = completed_settlements
                .iter()
                .map(|s| {
                    let duration = s.completed_at.unwrap() - s.created_at;
                    duration.num_milliseconds()
                })
                .sum();
            self.stats.avg_execution_time_ms =
                (total_exec_time / completed_settlements.len() as i64) as u64;
        }

        // Calculate avg price improvement from completed auctions
        let completed_auctions: Vec<_> = self
            .auctions
            .values()
            .filter(|a| a.status == AuctionStatus::Completed)
            .collect();

        if !completed_auctions.is_empty() {
            let total_improvement: i32 = completed_auctions
                .iter()
                .map(|a| a.stats.price_improvement_bps)
                .sum();
            self.stats.avg_price_improvement_bps =
                total_improvement / completed_auctions.len() as i32;
        }

        // Calculate total volume in USD
        let total_volume: f64 = completed_settlements
            .iter()
            .map(|s| {
                // Get input denom from the intent
                if let Some(intent_id) = s.intent_ids.first() {
                    if let Some(intent) = self.intents.get(intent_id) {
                        let price = self.prices.get(&intent.input.denom).map(|p| p.price_usd).unwrap_or(1.0);
                        return (s.input_amount as f64 / 1_000_000.0) * price;
                    }
                }
                0.0
            })
            .sum();
        self.stats.total_volume_usd = total_volume;
    }
}
