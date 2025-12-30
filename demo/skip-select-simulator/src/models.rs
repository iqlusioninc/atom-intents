//! Data models for the Skip Select Simulator

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Intent submitted by a user for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub user_address: String,
    pub input: Asset,
    pub output: OutputSpec,
    pub fill_config: FillConfig,
    pub constraints: ExecutionConstraints,
    pub status: IntentStatus,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub auction_id: Option<String>,
    pub settlement_id: Option<String>,
    /// Amount of input that has been filled (for partial fills)
    #[serde(default)]
    pub filled_amount: u128,
    /// Remaining amount to be filled
    #[serde(default)]
    pub remaining_amount: u128,
    /// Fill percentage (0-100)
    #[serde(default)]
    pub fill_percentage: u8,
    /// Whether this intent was created by the demo generator (vs real Keplr user)
    #[serde(default)]
    pub is_demo: bool,
}

impl Intent {
    pub fn new(req: CreateIntentRequest) -> Self {
        let id = format!("intent_{}", Uuid::new_v4());
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(req.timeout_seconds.unwrap_or(60) as i64);
        let input_amount = req.input.amount;

        Self {
            id,
            user_address: req.user_address,
            input: req.input,
            output: req.output,
            fill_config: req.fill_config.unwrap_or_default(),
            constraints: req.constraints.unwrap_or_default(),
            status: IntentStatus::Pending,
            created_at: now,
            expires_at,
            auction_id: None,
            settlement_id: None,
            filled_amount: 0,
            remaining_amount: input_amount,
            fill_percentage: 0,
            is_demo: req.is_demo.unwrap_or(false),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub chain_id: String,
    pub denom: String,
    pub amount: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSpec {
    pub chain_id: String,
    pub denom: String,
    pub min_amount: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillConfig {
    pub allow_partial: bool,
    pub min_fill_percent: u8,
    pub strategy: FillStrategy,
}

impl Default for FillConfig {
    fn default() -> Self {
        Self {
            allow_partial: true,
            min_fill_percent: 80,
            strategy: FillStrategy::Eager,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FillStrategy {
    Eager,
    AllOrNothing,
    TimeBased,
    PriceBased,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConstraints {
    pub max_hops: u8,
    pub allowed_venues: Vec<String>,
    pub excluded_venues: Vec<String>,
    pub max_slippage_bps: u16,
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self {
            max_hops: 3,
            allowed_venues: vec![],
            excluded_venues: vec![],
            max_slippage_bps: 100, // 1%
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    Pending,
    InAuction,
    Matched,
    PartiallyFilled,
    Settling,
    Completed,
    Failed,
    Expired,
    Cancelled,
}

/// Request to create a new intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateIntentRequest {
    pub user_address: String,
    pub input: Asset,
    pub output: OutputSpec,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_config: Option<FillConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<ExecutionConstraints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Mark as demo-generated intent (lower priority than real intents)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_demo: Option<bool>,
}

/// Batch auction state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auction {
    pub id: String,
    pub intent_ids: Vec<String>,
    pub status: AuctionStatus,
    pub quotes: Vec<SolverQuote>,
    pub winning_quote: Option<SolverQuote>,
    pub clearing_price: Option<f64>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub stats: AuctionStats,
}

impl Auction {
    pub fn new(intent_ids: Vec<String>) -> Self {
        Self {
            id: format!("auction_{}", Uuid::new_v4()),
            intent_ids,
            status: AuctionStatus::Open,
            quotes: vec![],
            winning_quote: None,
            clearing_price: None,
            started_at: Utc::now(),
            completed_at: None,
            stats: AuctionStats::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuctionStatus {
    Open,
    Collecting,
    Clearing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuctionStats {
    pub num_intents: usize,
    pub num_quotes: usize,
    pub total_input_amount: u128,
    pub total_output_amount: u128,
    pub matched_volume: u128,
    pub price_improvement_bps: i32,
    pub solver_competition_score: f64,
}

/// Quote from a solver
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverQuote {
    pub id: String,
    pub solver_id: String,
    pub solver_name: String,
    pub solver_type: SolverType,
    pub intent_ids: Vec<String>,
    pub input_amount: u128,
    pub output_amount: u128,
    pub effective_price: f64,
    pub execution_plan: ExecutionPlan,
    pub estimated_gas: u64,
    pub confidence: f64,
    pub submitted_at: DateTime<Utc>,
    /// Reason this solver had an advantage (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advantage_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SolverType {
    DexRouter,
    IntentMatcher,
    CexBackstop,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub plan_type: ExecutionPlanType,
    pub steps: Vec<ExecutionStep>,
    pub estimated_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPlanType {
    DexRoute,
    DirectMatch,
    CexHedge,
    MultiHop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_type: String,
    pub chain_id: String,
    pub venue: Option<String>,
    pub input_denom: String,
    pub output_denom: String,
    pub amount: u128,
    pub description: String,
}

/// Settlement state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub id: String,
    pub auction_id: String,
    pub intent_ids: Vec<String>,
    pub solver_id: String,
    pub status: SettlementStatus,
    pub phase: SettlementPhase,
    pub input_amount: u128,
    pub output_amount: u128,
    pub escrow_txid: Option<String>,
    pub execution_txid: Option<String>,
    pub ibc_packet_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub events: Vec<SettlementEvent>,
    /// Whether this is a partial fill settlement
    #[serde(default)]
    pub is_partial_fill: bool,
    /// Fill percentage for partial fills (0-100)
    #[serde(default)]
    pub fill_percentage: u8,
    /// Original requested input amount (for calculating partial fill ratio)
    #[serde(default)]
    pub original_input_amount: u128,

    // Intent-derived fields for settlement execution
    /// User's address on the input chain
    #[serde(default)]
    pub user_address: String,
    /// User's address on the output chain (for cross-chain settlements)
    #[serde(default)]
    pub user_output_address: String,
    /// Input chain ID
    #[serde(default)]
    pub input_chain_id: String,
    /// Input denomination
    #[serde(default)]
    pub input_denom: String,
    /// Output chain ID
    #[serde(default)]
    pub output_chain_id: String,
    /// Output denomination
    #[serde(default)]
    pub output_denom: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    Pending,
    Committing,
    Executing,
    Completed,
    Failed,
    Refunded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SettlementPhase {
    Init,
    EscrowLocked,
    SolverCommitted,
    IbcInFlight,
    Finalized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementEvent {
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub description: String,
    pub metadata: serde_json::Value,
}

/// Price feed data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceFeed {
    pub denom: String,
    pub price_usd: f64,
    pub change_24h: f64,
    pub volume_24h: f64,
    pub confidence: f64,
    pub updated_at: DateTime<Utc>,
}

/// Size preference for solver advantages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SizePreference {
    Small,      // < $500
    Medium,     // $500 - $5K
    Large,      // > $5K
    Any,        // No preference
}

/// Advantage profile for a solver - determines competitive edges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverAdvantageProfile {
    /// Preferred trading pairs (input_denom, output_denom) - order matters
    pub preferred_pairs: Vec<(String, String)>,
    /// Preferred order size range
    pub size_preference: SizePreference,
    /// Chain specialties (chain_ids where solver excels)
    pub chain_specialty: Vec<String>,
}

impl Default for SolverAdvantageProfile {
    fn default() -> Self {
        Self {
            preferred_pairs: vec![],
            size_preference: SizePreference::Any,
            chain_specialty: vec![],
        }
    }
}

/// Solver information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Solver {
    pub id: String,
    pub name: String,
    pub solver_type: SolverType,
    pub status: SolverStatus,
    pub reputation_score: f64,
    pub total_volume: u128,
    pub success_rate: f64,
    pub avg_execution_time_ms: u64,
    pub supported_chains: Vec<String>,
    pub supported_denoms: Vec<String>,
    pub connected_at: Option<DateTime<Utc>>,
    /// Advantage profile defining competitive edges
    #[serde(default)]
    pub advantage_profile: SolverAdvantageProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SolverStatus {
    Active,
    Idle,
    Suspended,
    Offline,
}

/// System statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemStats {
    pub total_intents: u64,
    pub total_auctions: u64,
    pub total_settlements: u64,
    pub total_volume_usd: f64,
    pub avg_execution_time_ms: u64,
    pub avg_price_improvement_bps: i32,
    pub success_rate: f64,
    pub active_solvers: u64,
    pub pending_intents: u64,
    pub intents_per_minute: f64,
}

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    // Client -> Server
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
    Ping,

    // Server -> Client
    IntentSubmitted(Intent),
    AuctionStarted(Auction),
    QuoteReceived(SolverQuote),
    AuctionCompleted(Auction),
    SettlementUpdate(Settlement),
    PriceUpdate(Vec<PriceFeed>),
    StatsUpdate(SystemStats),
    Error { message: String },
    Pong,
}

/// Demo scenario configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoScenario {
    pub name: String,
    pub description: String,
    pub intents: Vec<CreateIntentRequest>,
    pub expected_outcome: String,
}
