use async_trait::async_trait;
use atom_intents_types::{
    Intent, Solution, SolveContext, SolverCapabilities, SolverCapacity, TradingPair,
};

use crate::SolveError;

/// Core trait that all solvers must implement
#[async_trait]
pub trait Solver: Send + Sync {
    /// Unique identifier for this solver
    fn id(&self) -> &str;

    /// Trading pairs this solver supports
    fn supported_pairs(&self) -> &[TradingPair];

    /// Solver capabilities declaration
    fn capabilities(&self) -> &SolverCapabilities;

    /// Attempt to solve an intent
    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError>;

    /// Get current capacity for a trading pair
    async fn capacity(&self, pair: &TradingPair) -> Result<SolverCapacity, SolveError>;

    /// Health check
    async fn health_check(&self) -> bool {
        true
    }
}

/// Extension trait for solvers with relayer integration
#[async_trait]
pub trait RelayerIntegratedSolver: Solver {
    /// Get the associated relayer
    fn relayer(&self) -> &dyn SolverRelayer;

    /// Track a settlement for priority relaying
    async fn track_settlement(&self, intent_id: &str) -> Result<(), SolveError>;
}

/// Relayer interface for solver-integrated relayers
#[async_trait]
pub trait SolverRelayer: Send + Sync {
    /// Add a packet to priority queue
    async fn add_priority_packet(&self, packet_info: PacketInfo) -> Result<(), RelayerError>;

    /// Get pending packet count
    async fn pending_count(&self) -> usize;

    /// Check if relayer is healthy
    async fn is_healthy(&self) -> bool;
}

/// Information about an IBC packet to relay
#[derive(Clone, Debug)]
pub struct PacketInfo {
    pub source_chain: String,
    pub dest_chain: String,
    pub channel: String,
    pub sequence: u64,
    pub solver_exposure: u128,
    pub timeout: u64,
}

impl PacketInfo {
    /// Calculate priority based on exposure and timeout
    pub fn priority(&self) -> u64 {
        // Higher exposure and closer timeout = higher priority
        let time_factor = self.timeout.saturating_sub(current_timestamp());
        let exposure_factor = self.solver_exposure / 1_000_000; // Normalize

        exposure_factor as u64 * 1000 / (time_factor + 1)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerError {
    #[error("failed to submit packet: {0}")]
    SubmitFailed(String),

    #[error("relayer not connected")]
    NotConnected,

    #[error("packet already queued")]
    AlreadyQueued,
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// DEX client trait for querying liquidity
#[async_trait]
pub trait DexClient: Send + Sync {
    /// Get a quote for a swap
    async fn get_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Result<DexQuote, DexError>;

    /// Get available pools for a pair
    async fn get_pools(&self, pair: &TradingPair) -> Result<Vec<PoolInfo>, DexError>;
}

#[derive(Clone, Debug)]
pub struct DexQuote {
    pub venue: String,
    pub input_amount: u128,
    pub output_amount: u128,
    pub price_impact: String,
    pub route: Vec<atom_intents_types::DexSwapStep>,
    pub estimated_fee: Option<crate::FeeEstimate>,
}

#[derive(Clone, Debug)]
pub struct PoolInfo {
    pub pool_id: String,
    pub token_a: String,
    pub token_b: String,
    pub liquidity_a: u128,
    pub liquidity_b: u128,
    pub fee_rate: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DexError {
    #[error("pair not found: {0}")]
    PairNotFound(String),

    #[error("insufficient liquidity")]
    InsufficientLiquidity,

    #[error("query failed: {0}")]
    QueryFailed(String),
}
