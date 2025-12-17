use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

use crate::{Fill, TradingPair};

/// Result of matching an intent against the order book
#[cw_serde]
pub struct MatchResult {
    /// Fills generated from matching
    pub fills: Vec<Fill>,

    /// Remaining unfilled amount
    pub remaining: Uint128,
}

impl MatchResult {
    pub fn empty(remaining: Uint128) -> Self {
        Self {
            fills: vec![],
            remaining,
        }
    }

    pub fn total_filled(&self) -> Uint128 {
        self.fills.iter().map(|f| f.input_amount).sum()
    }
}

/// Result of a batch auction
#[cw_serde]
pub struct AuctionResult {
    /// Auction epoch identifier
    pub epoch_id: u64,

    /// Uniform clearing price
    pub clearing_price: String,

    /// Fills from internal order crossing
    pub internal_fills: Vec<AuctionFill>,

    /// Fills from solver quotes
    pub solver_fills: Vec<AuctionFill>,
}

/// A fill within an auction
#[cw_serde]
pub struct AuctionFill {
    pub intent_id: String,
    pub counterparty: String, // Other intent ID or solver ID
    pub input_amount: Uint128,
    pub output_amount: Uint128,
}

/// Price level in an order book
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PriceLevel(pub String);

impl PriceLevel {
    pub fn new(price: impl Into<String>) -> Self {
        Self(price.into())
    }
}

/// Order book snapshot for a trading pair
#[cw_serde]
pub struct OrderBookSnapshot {
    pub pair: TradingPair,
    pub timestamp: u64,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
}

/// A level in the order book
#[cw_serde]
pub struct OrderBookLevel {
    pub price: String,
    pub amount: Uint128,
    pub order_count: u32,
}

/// Market data for a trading pair
#[cw_serde]
pub struct MarketData {
    pub pair: TradingPair,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub mid_price: Option<String>,
    pub oracle_price: String,
    pub volume_24h: Uint128,
}

/// IBC transfer details
#[cw_serde]
pub struct IbcTransferInfo {
    /// Source chain
    pub source_chain: String,

    /// Destination chain
    pub dest_chain: String,

    /// IBC channel
    pub channel: String,

    /// Transfer amount
    pub amount: Uint128,

    /// Token denom
    pub denom: String,

    /// Sender address
    pub sender: String,

    /// Receiver address
    pub receiver: String,

    /// Timeout timestamp
    pub timeout_timestamp: u64,

    /// Memo for IBC hooks/PFM
    pub memo: Option<String>,
}

/// Settlement details
#[cw_serde]
pub struct Settlement {
    pub intent_id: String,
    pub solver_id: String,
    pub user_input: Uint128,
    pub solver_output: Uint128,
    pub ibc_transfers: Vec<IbcTransferInfo>,
    pub status: SettlementStatus,
}

#[cw_serde]
pub enum SettlementStatus {
    Pending,
    UserLocked,
    SolverLocked,
    Executing,
    Complete,
    Failed { reason: String },
    TimedOut,
}

/// Slashing configuration
#[cw_serde]
pub struct SlashingConfig {
    /// Base slash percentage (e.g., 2%)
    pub base_slash_pct: String,

    /// Multiplier after repeated failures
    pub repeat_multiplier: String,

    /// Minimum slash amount
    pub min_slash: Uint128,

    /// Maximum slash amount
    pub max_slash: Uint128,

    /// Bond lock multiplier (e.g., 1.5x fill value)
    pub bond_lock_multiplier: String,
}

impl Default for SlashingConfig {
    fn default() -> Self {
        Self {
            base_slash_pct: "0.02".to_string(),     // 2%
            repeat_multiplier: "2.0".to_string(),   // 2x after 3 failures
            min_slash: Uint128::new(10_000_000),    // 10 ATOM (assuming uatom)
            max_slash: Uint128::new(1_000_000_000), // 1000 ATOM
            bond_lock_multiplier: "1.5".to_string(),
        }
    }
}
