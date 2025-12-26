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

        // Initialize mock solvers
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
    }
}
