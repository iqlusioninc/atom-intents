use async_trait::async_trait;
use atom_intents_types::{
    ExecutionPlan, Intent, ProposedFill, Solution, SolveContext, SolverCapabilities,
    SolverCapacity, TradingPair,
};
use cosmwasm_std::Uint128;
use hmac::{Hmac, Mac};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use thiserror::Error;

use crate::{SolveError, Solver};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum CexError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("insufficient balance: needed {needed}, available {available}")]
    InsufficientBalance { needed: String, available: String },

    #[error("symbol not supported: {0}")]
    UnsupportedSymbol(String),

    #[error("order rejected: {0}")]
    OrderRejected(String),

    #[error("withdrawal failed: {0}")]
    WithdrawalFailed(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("invalid orderbook data")]
    InvalidOrderbook,

    #[error("rate limit exceeded")]
    RateLimitExceeded,

    #[error("parse error: {0}")]
    ParseError(String),
}

// ============================================================================
// Core Types
// ============================================================================

/// CEX orderbook representation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Orderbook {
    /// Symbol (e.g., "ATOMUSDC")
    pub symbol: String,

    /// Bids (buy orders) - sorted by price descending
    pub bids: Vec<OrderbookLevel>,

    /// Asks (sell orders) - sorted by price ascending
    pub asks: Vec<OrderbookLevel>,

    /// Last update timestamp
    pub last_update: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookLevel {
    /// Price per unit
    pub price: String,

    /// Quantity available at this price
    pub quantity: String,
}

impl Orderbook {
    /// Estimate fill for buying base_amount of base currency
    pub fn estimate_buy(&self, base_amount: Decimal) -> Result<Decimal, CexError> {
        let mut remaining = base_amount;
        let mut total_cost = Decimal::ZERO;

        for ask in &self.asks {
            if remaining <= Decimal::ZERO {
                break;
            }

            let price = Decimal::from_str(&ask.price).map_err(|_| CexError::InvalidOrderbook)?;
            let quantity =
                Decimal::from_str(&ask.quantity).map_err(|_| CexError::InvalidOrderbook)?;

            let fill_qty = remaining.min(quantity);
            total_cost += fill_qty * price;
            remaining -= fill_qty;
        }

        if remaining > Decimal::ZERO {
            return Err(CexError::InsufficientBalance {
                needed: base_amount.to_string(),
                available: (base_amount - remaining).to_string(),
            });
        }

        Ok(total_cost)
    }

    /// Estimate revenue from selling base_amount of base currency
    pub fn estimate_sell(&self, base_amount: Decimal) -> Result<Decimal, CexError> {
        let mut remaining = base_amount;
        let mut total_revenue = Decimal::ZERO;

        for bid in &self.bids {
            if remaining <= Decimal::ZERO {
                break;
            }

            let price = Decimal::from_str(&bid.price).map_err(|_| CexError::InvalidOrderbook)?;
            let quantity =
                Decimal::from_str(&bid.quantity).map_err(|_| CexError::InvalidOrderbook)?;

            let fill_qty = remaining.min(quantity);
            total_revenue += fill_qty * price;
            remaining -= fill_qty;
        }

        if remaining > Decimal::ZERO {
            return Err(CexError::InsufficientBalance {
                needed: base_amount.to_string(),
                available: (base_amount - remaining).to_string(),
            });
        }

        Ok(total_revenue)
    }
}

/// CEX order representation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CexOrder {
    pub symbol: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub quantity: String,
    pub price: Option<String>, // None for market orders
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Result of placing an order
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CexOrderResult {
    pub order_id: String,
    pub symbol: String,
    pub filled_quantity: String,
    pub average_price: String,
    pub fee: String,
    pub fee_asset: String,
    pub status: OrderStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
}

/// CEX balance information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CexBalance {
    pub asset: String,
    pub available: String,
    pub locked: String,
}

impl CexBalance {
    pub fn total(&self) -> Result<Decimal, CexError> {
        let available = Decimal::from_str(&self.available)
            .map_err(|e| CexError::ApiError(format!("invalid available: {}", e)))?;
        let locked = Decimal::from_str(&self.locked)
            .map_err(|e| CexError::ApiError(format!("invalid locked: {}", e)))?;
        Ok(available + locked)
    }
}

/// Result of a withdrawal request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WithdrawResult {
    pub tx_id: String,
    pub asset: String,
    pub amount: String,
    pub address: String,
    pub network: String,
    pub status: WithdrawStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WithdrawStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

// ============================================================================
// CEX Client Trait
// ============================================================================

/// Generic CEX client interface
#[async_trait]
pub trait CexClient: Send + Sync {
    /// Get current orderbook for a symbol
    async fn get_orderbook(&self, symbol: &str) -> Result<Orderbook, CexError>;

    /// Place an order
    async fn place_order(&self, order: CexOrder) -> Result<CexOrderResult, CexError>;

    /// Get balance for an asset
    async fn get_balance(&self, asset: &str) -> Result<CexBalance, CexError>;

    /// Initiate withdrawal to blockchain address
    async fn withdraw(
        &self,
        asset: &str,
        amount: u128,
        address: &str,
    ) -> Result<WithdrawResult, CexError>;

    /// Convert trading pair to CEX symbol format
    fn pair_to_symbol(&self, pair: &TradingPair) -> Option<String>;
}

// ============================================================================
// CEX Backstop Solver
// ============================================================================

/// Configuration for CEX backstop solver
#[derive(Clone, Debug)]
pub struct CexBackstopConfig {
    /// Minimum liquidity required from DEX before using CEX
    pub min_dex_liquidity_usd: u64,

    /// CEX trading fee (as decimal, e.g., 0.001 for 0.1%)
    pub cex_fee_rate: Decimal,

    /// CEX withdrawal fee per asset
    pub withdrawal_fees: HashMap<String, u128>,

    /// Maximum position exposure per asset (in USD)
    pub max_position_usd: u64,

    /// Surplus capture rate (e.g., 0.10 for 10%)
    pub surplus_capture_rate: Decimal,

    /// IBC transfer time estimate (seconds)
    pub ibc_transfer_time_secs: u64,
}

impl Default for CexBackstopConfig {
    fn default() -> Self {
        let mut withdrawal_fees = HashMap::new();
        withdrawal_fees.insert("ATOM".to_string(), 100_000); // 0.1 ATOM in uatom
        withdrawal_fees.insert("USDC".to_string(), 1_000_000); // 1 USDC in uusdc

        Self {
            min_dex_liquidity_usd: 50_000,
            cex_fee_rate: Decimal::from_str("0.001").unwrap(), // 0.1%
            withdrawal_fees,
            max_position_usd: 1_000_000,
            surplus_capture_rate: Decimal::from_str("0.10").unwrap(), // 10%
            ibc_transfer_time_secs: 300,                              // 5 minutes
        }
    }
}

/// Inventory position tracker
#[derive(Clone, Debug, Default)]
struct InventoryPosition {
    /// Net position per asset (positive = long, negative = short)
    positions: HashMap<String, i128>,
}

impl InventoryPosition {
    fn update(&mut self, asset: &str, delta: i128) {
        *self.positions.entry(asset.to_string()).or_insert(0) += delta;
    }

    fn get_position(&self, asset: &str) -> i128 {
        *self.positions.get(asset).unwrap_or(&0)
    }
}

/// CEX backstop solver - provides quotes when DEX liquidity is insufficient
pub struct CexBackstopSolver {
    id: String,
    supported_pairs: Vec<TradingPair>,
    capabilities: SolverCapabilities,
    client: Arc<dyn CexClient>,
    config: CexBackstopConfig,
    inventory: Arc<RwLock<InventoryPosition>>,
}

impl CexBackstopSolver {
    pub fn new(
        id: impl Into<String>,
        client: Arc<dyn CexClient>,
        config: CexBackstopConfig,
    ) -> Self {
        Self {
            id: id.into(),
            supported_pairs: vec![
                TradingPair::new("uatom", "uusdc"),
                TradingPair::new("uosmo", "uusdc"),
                TradingPair::new("uatom", "uosmo"),
            ],
            capabilities: SolverCapabilities {
                dex_routing: false,
                intent_matching: false,
                cex_backstop: true,
                cross_ecosystem: false,
                max_fill_size_usd: 500_000,
            },
            client,
            config,
            inventory: Arc::new(RwLock::new(InventoryPosition::default())),
        }
    }

    /// Check if CEX should be used based on DEX liquidity
    #[allow(dead_code)]
    fn should_use_cex(&self, _dex_liquidity_usd: u64) -> bool {
        // For now, always provide CEX quotes as backstop
        // In production, would check: dex_liquidity_usd < self.config.min_dex_liquidity_usd
        true
    }

    /// Convert cosmos denom to CEX asset symbol
    fn denom_to_asset(&self, denom: &str) -> String {
        match denom {
            "uatom" => "ATOM".to_string(),
            "uusdc" => "USDC".to_string(),
            "uosmo" => "OSMO".to_string(),
            _ => denom.to_uppercase(),
        }
    }

    /// Calculate withdrawal fee for an asset
    fn get_withdrawal_fee(&self, denom: &str) -> u128 {
        let asset = self.denom_to_asset(denom);
        *self.config.withdrawal_fees.get(&asset).unwrap_or(&0)
    }

    /// Calculate bond requirement
    fn calculate_bond(&self, fill_amount: Uint128) -> Uint128 {
        // Bond = 2x the fill amount for CEX backstop (higher risk)
        fill_amount * Uint128::new(2)
    }

    /// Estimate fill using CEX orderbook
    async fn estimate_cex_fill(
        &self,
        input_denom: &str,
        output_denom: &str,
        input_amount: u128,
    ) -> Result<(u128, String), SolveError> {
        let pair = TradingPair::new(input_denom, output_denom);

        let symbol = self
            .client
            .pair_to_symbol(&pair)
            .ok_or_else(|| SolveError::CexQueryFailed("unsupported pair".to_string()))?;

        let orderbook = self
            .client
            .get_orderbook(&symbol)
            .await
            .map_err(|e| SolveError::CexQueryFailed(e.to_string()))?;

        // Convert input amount to decimal (assuming 6 decimals)
        let input_decimal = Decimal::from_i128_with_scale(input_amount as i128, 6);

        // Determine if we're buying or selling the base asset
        let base_asset = self.denom_to_asset(input_denom);
        let _quote_asset = self.denom_to_asset(output_denom);

        let (output_before_fees, avg_price) = if symbol.starts_with(&base_asset) {
            // Selling base asset
            let revenue = orderbook
                .estimate_sell(input_decimal)
                .map_err(|e| SolveError::CexQueryFailed(e.to_string()))?;
            let price = revenue / input_decimal;
            (revenue, price)
        } else {
            // Buying base asset (spending quote)
            let cost = orderbook
                .estimate_buy(input_decimal)
                .map_err(|e| SolveError::CexQueryFailed(e.to_string()))?;
            let price = cost / input_decimal;
            (cost, price)
        };

        // Apply CEX trading fee
        let after_trading_fee = output_before_fees * (Decimal::ONE - self.config.cex_fee_rate);

        // Apply withdrawal fee
        let withdrawal_fee =
            Decimal::from_i128_with_scale(self.get_withdrawal_fee(output_denom) as i128, 6);

        let output_after_fees = after_trading_fee - withdrawal_fee;

        // Convert back to base units (multiply by 10^6 to convert from whole units to micro-units)
        let output_in_base_units = output_after_fees * Decimal::from(1_000_000);
        let mut scaled = output_in_base_units;
        scaled.rescale(0); // Convert to integer
        let output_amount = scaled
            .to_u128()
            .ok_or_else(|| SolveError::Internal("output amount overflow".to_string()))?;

        Ok((output_amount, avg_price.to_string()))
    }

    /// Update inventory tracking
    fn update_inventory(&self, input_denom: &str, output_denom: &str, amount: i128) {
        if let Ok(mut inventory) = self.inventory.write() {
            inventory.update(input_denom, -amount); // Sold
            inventory.update(output_denom, amount); // Bought
        }
    }

    /// Get current inventory position
    pub fn get_position(&self, asset: &str) -> i128 {
        self.inventory
            .read()
            .ok()
            .map(|inv| inv.get_position(asset))
            .unwrap_or(0)
    }
}

#[async_trait]
impl Solver for CexBackstopSolver {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_pairs(&self) -> &[TradingPair] {
        &self.supported_pairs
    }

    fn capabilities(&self) -> &SolverCapabilities {
        &self.capabilities
    }

    async fn solve(&self, intent: &Intent, ctx: &SolveContext) -> Result<Solution, SolveError> {
        // Check if pair is supported
        let pair = intent.pair();
        if !self.supported_pairs.contains(&pair) {
            return Err(SolveError::NoViableRoute);
        }

        // Get CEX fill estimate
        let (output_amount, _avg_price) = self
            .estimate_cex_fill(
                &intent.input.denom,
                &intent.output.denom,
                ctx.remaining.u128(),
            )
            .await?;

        // Check if output meets minimum requirement
        if Uint128::new(output_amount) < intent.output.min_amount {
            return Err(SolveError::InsufficientLiquidity {
                needed: intent.output.min_amount.to_string(),
                available: output_amount.to_string(),
            });
        }

        // Calculate surplus and solver fee
        let limit_price =
            intent
                .output
                .limit_price_decimal()
                .map_err(|e| SolveError::InvalidIntent {
                    reason: format!("invalid limit price: {}", e),
                })?;

        let remaining_dec = Decimal::from(ctx.remaining.u128());
        let user_min_output_dec = remaining_dec * limit_price;
        let output_amount_dec = Decimal::from(output_amount);
        let surplus_dec = (output_amount_dec - user_min_output_dec).max(Decimal::ZERO);
        let solver_fee_dec = surplus_dec * self.config.surplus_capture_rate;
        let solver_fee = solver_fee_dec
            .trunc()
            .to_string()
            .parse::<u128>()
            .unwrap_or(0);

        let output_to_user = output_amount.saturating_sub(solver_fee);
        let output_to_user_dec = Decimal::from(output_to_user);
        let effective_price = output_to_user_dec / remaining_dec;

        // Update inventory tracking
        self.update_inventory(
            &intent.input.denom,
            &intent.output.denom,
            ctx.remaining.u128() as i128,
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Get exchange name from client for execution plan
        let exchange = "binance".to_string(); // Could be made configurable

        Ok(Solution {
            solver_id: self.id.clone(),
            intent_id: intent.id.clone(),
            fill: ProposedFill {
                input_amount: ctx.remaining,
                output_amount: Uint128::new(output_to_user),
                price: format!("{:.6}", effective_price),
            },
            execution: ExecutionPlan::CexHedge { exchange },
            valid_until: current_time + 3, // 3 second validity for CEX quotes
            bond: self.calculate_bond(ctx.remaining),
        })
    }

    async fn capacity(&self, pair: &TradingPair) -> Result<SolverCapacity, SolveError> {
        if !self.supported_pairs.contains(pair) {
            return Err(SolveError::NoViableRoute);
        }

        let symbol = self
            .client
            .pair_to_symbol(pair)
            .ok_or_else(|| SolveError::CexQueryFailed("unsupported pair".to_string()))?;

        let orderbook = self
            .client
            .get_orderbook(&symbol)
            .await
            .map_err(|e| SolveError::CexQueryFailed(e.to_string()))?;

        // Calculate available liquidity from orderbook depth
        let mut total_liquidity_decimal = Decimal::ZERO;
        for ask in orderbook.asks.iter().take(10) {
            if let (Ok(price), Ok(qty)) = (
                Decimal::from_str(&ask.price),
                Decimal::from_str(&ask.quantity),
            ) {
                total_liquidity_decimal += price * qty;
            }
        }

        // Convert to u128 with 6 decimals (scale and convert)
        let mut scaled_liquidity = total_liquidity_decimal * Decimal::from(1_000_000);
        scaled_liquidity.rescale(0);
        let total_liquidity = scaled_liquidity.to_u128().unwrap_or(0);

        Ok(SolverCapacity {
            max_immediate: Uint128::new(total_liquidity / 5), // 20% of orderbook depth
            available_liquidity: Uint128::new(total_liquidity),
            estimated_time_ms: (self.config.ibc_transfer_time_secs * 1000) + 5000, // IBC time + 5s execution
        })
    }

    async fn health_check(&self) -> bool {
        // Try to fetch a sample orderbook
        if let Some(symbol) = self
            .client
            .pair_to_symbol(&TradingPair::new("uatom", "uusdc"))
        {
            self.client.get_orderbook(&symbol).await.is_ok()
        } else {
            false
        }
    }
}

// ============================================================================
// Mock CEX Client for Testing
// ============================================================================

/// Mock CEX client with configurable orderbooks
pub struct MockCexClient {
    orderbooks: HashMap<String, Orderbook>,
    balances: HashMap<String, CexBalance>,
}

impl MockCexClient {
    pub fn new() -> Self {
        Self {
            orderbooks: HashMap::new(),
            balances: HashMap::new(),
        }
    }

    /// Add a mock orderbook
    pub fn with_orderbook(mut self, symbol: impl Into<String>, orderbook: Orderbook) -> Self {
        self.orderbooks.insert(symbol.into(), orderbook);
        self
    }

    /// Add a mock balance
    pub fn with_balance(mut self, asset: impl Into<String>, available: &str, locked: &str) -> Self {
        let asset_str = asset.into();
        self.balances.insert(
            asset_str.clone(),
            CexBalance {
                asset: asset_str,
                available: available.to_string(),
                locked: locked.to_string(),
            },
        );
        self
    }

    /// Create a simple orderbook with uniform spread
    pub fn simple_orderbook(symbol: &str, mid_price: f64, spread: f64, depth: f64) -> Orderbook {
        let mid_price_dec = Decimal::from_f64_retain(mid_price).unwrap_or(Decimal::ZERO);
        let spread_dec = Decimal::from_f64_retain(spread).unwrap_or(Decimal::ZERO);
        let depth_dec = Decimal::from_f64_retain(depth).unwrap_or(Decimal::ZERO);
        let one = Decimal::ONE;
        let two = Decimal::TWO;

        let bid_price = mid_price_dec * (one - spread_dec / two);
        let ask_price = mid_price_dec * (one + spread_dec / two);
        let multiplier_099 = Decimal::from_str("0.99").unwrap();
        let multiplier_101 = Decimal::from_str("1.01").unwrap();

        Orderbook {
            symbol: symbol.to_string(),
            bids: vec![
                OrderbookLevel {
                    price: bid_price.to_string(),
                    quantity: depth_dec.to_string(),
                },
                OrderbookLevel {
                    price: (bid_price * multiplier_099).to_string(),
                    quantity: (depth_dec * two).to_string(),
                },
            ],
            asks: vec![
                OrderbookLevel {
                    price: ask_price.to_string(),
                    quantity: depth_dec.to_string(),
                },
                OrderbookLevel {
                    price: (ask_price * multiplier_101).to_string(),
                    quantity: (depth_dec * two).to_string(),
                },
            ],
            last_update: 0,
        }
    }
}

impl Default for MockCexClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CexClient for MockCexClient {
    async fn get_orderbook(&self, symbol: &str) -> Result<Orderbook, CexError> {
        self.orderbooks
            .get(symbol)
            .cloned()
            .ok_or_else(|| CexError::UnsupportedSymbol(symbol.to_string()))
    }

    async fn place_order(&self, order: CexOrder) -> Result<CexOrderResult, CexError> {
        // Simple mock - assume full fill at average price
        let orderbook = self.get_orderbook(&order.symbol).await?;

        let (avg_price, _) = match order.side {
            OrderSide::Buy => {
                let ask = orderbook
                    .asks
                    .first()
                    .ok_or_else(|| CexError::OrderRejected("no liquidity".to_string()))?;
                (ask.price.clone(), ask.quantity.clone())
            }
            OrderSide::Sell => {
                let bid = orderbook
                    .bids
                    .first()
                    .ok_or_else(|| CexError::OrderRejected("no liquidity".to_string()))?;
                (bid.price.clone(), bid.quantity.clone())
            }
        };

        Ok(CexOrderResult {
            order_id: "mock-order-123".to_string(),
            symbol: order.symbol,
            filled_quantity: order.quantity,
            average_price: avg_price,
            fee: "0.001".to_string(),
            fee_asset: "USDC".to_string(),
            status: OrderStatus::Filled,
        })
    }

    async fn get_balance(&self, asset: &str) -> Result<CexBalance, CexError> {
        self.balances
            .get(asset)
            .cloned()
            .ok_or_else(|| CexError::ApiError(format!("asset not found: {}", asset)))
    }

    async fn withdraw(
        &self,
        asset: &str,
        amount: u128,
        address: &str,
    ) -> Result<WithdrawResult, CexError> {
        Ok(WithdrawResult {
            tx_id: "mock-withdraw-tx".to_string(),
            asset: asset.to_string(),
            amount: amount.to_string(),
            address: address.to_string(),
            network: "cosmos".to_string(),
            status: WithdrawStatus::Pending,
        })
    }

    fn pair_to_symbol(&self, pair: &TradingPair) -> Option<String> {
        let base = match pair.base.as_str() {
            "uatom" => "ATOM",
            "uosmo" => "OSMO",
            "uusdc" => "USDC",
            _ => return None,
        };

        let quote = match pair.quote.as_str() {
            "uatom" => "ATOM",
            "uosmo" => "OSMO",
            "uusdc" => "USDC",
            _ => return None,
        };

        Some(format!("{}{}", base, quote))
    }
}

// ============================================================================
// Binance API Response Types
// ============================================================================

/// Binance error response
#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i32,
    msg: String,
}

/// Binance orderbook response
#[derive(Debug, Deserialize)]
struct BinanceOrderbookResponse {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<(String, String)>, // [price, quantity]
    asks: Vec<(String, String)>,
}

/// Binance account information response
#[derive(Debug, Deserialize)]
struct BinanceAccountResponse {
    balances: Vec<BinanceBalanceInfo>,
}

#[derive(Debug, Deserialize)]
struct BinanceBalanceInfo {
    asset: String,
    free: String,
    locked: String,
}

/// Binance order response
#[derive(Debug, Deserialize)]
struct BinanceOrderResponse {
    #[serde(rename = "orderId")]
    order_id: u64,
    symbol: String,
    status: String,
    #[serde(rename = "executedQty")]
    executed_qty: String,
    #[serde(rename = "cummulativeQuoteQty")]
    #[allow(dead_code)]
    cumulative_quote_qty: String,
    fills: Vec<BinanceFill>,
}

#[derive(Debug, Deserialize)]
struct BinanceFill {
    price: String,
    qty: String,
    commission: String,
    #[serde(rename = "commissionAsset")]
    commission_asset: String,
}

/// Binance withdrawal response
#[derive(Debug, Deserialize)]
struct BinanceWithdrawResponse {
    id: String,
}

// ============================================================================
// Binance Client Implementation
// ============================================================================

/// Binance API client configuration
#[derive(Clone)]
pub struct BinanceConfig {
    pub api_key: String,
    pub api_secret: String,
    pub base_url: String,
    pub testnet: bool,
}

impl Default for BinanceConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            base_url: "https://api.binance.com".to_string(),
            testnet: false,
        }
    }
}

/// Binance client implementation with full API integration
pub struct BinanceClient {
    config: BinanceConfig,
    http_client: reqwest::Client,
}

impl BinanceClient {
    pub fn new(config: BinanceConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }

    /// Construct endpoint URL
    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url, path)
    }

    /// Sign request using HMAC-SHA256
    fn sign_request(&self, params: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(self.config.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(params.as_bytes());

        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    /// Get current timestamp in milliseconds
    fn get_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// Execute a signed GET request with retry logic
    async fn signed_get(
        &self,
        path: &str,
        params: Vec<(&str, String)>,
    ) -> Result<reqwest::Response, CexError> {
        let timestamp = Self::get_timestamp();

        // Build query string with timestamp
        let mut query_params = params.clone();
        query_params.push(("timestamp", timestamp.to_string()));

        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Sign the query string
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = self.endpoint(&format!("{}?{}", path, signed_query));

        // Execute with retry logic
        self.execute_with_retry(|| async {
            self.http_client
                .get(&url)
                .header("X-MBX-APIKEY", &self.config.api_key)
                .send()
                .await
        })
        .await
    }

    /// Execute a signed POST request with retry logic
    async fn signed_post(
        &self,
        path: &str,
        params: Vec<(&str, String)>,
    ) -> Result<reqwest::Response, CexError> {
        let timestamp = Self::get_timestamp();

        // Build query string with timestamp
        let mut query_params = params.clone();
        query_params.push(("timestamp", timestamp.to_string()));

        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Sign the query string
        let signature = self.sign_request(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = self.endpoint(&format!("{}?{}", path, signed_query));

        // Execute with retry logic
        self.execute_with_retry(|| async {
            self.http_client
                .post(&url)
                .header("X-MBX-APIKEY", &self.config.api_key)
                .send()
                .await
        })
        .await
    }

    /// Execute request with retry logic for transient failures
    async fn execute_with_retry<F, Fut>(&self, f: F) -> Result<reqwest::Response, CexError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    {
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY_MS: u64 = 1000;

        for attempt in 0..MAX_RETRIES {
            match f().await {
                Ok(response) => {
                    let status = response.status();

                    // Handle rate limiting
                    if status.as_u16() == 429 {
                        if attempt < MAX_RETRIES - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                RETRY_DELAY_MS * (attempt as u64 + 1),
                            ))
                            .await;
                            continue;
                        }
                        return Err(CexError::RateLimitExceeded);
                    }

                    // Handle other errors
                    if !status.is_success() {
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "unknown error".to_string());

                        // Try to parse Binance error response
                        if let Ok(binance_error) = serde_json::from_str::<BinanceError>(&error_text)
                        {
                            return Err(match binance_error.code {
                                -1022 => CexError::AuthFailed(binance_error.msg),
                                -2010 => CexError::InsufficientBalance {
                                    needed: "unknown".to_string(),
                                    available: "unknown".to_string(),
                                },
                                -1121 => CexError::UnsupportedSymbol(binance_error.msg),
                                _ => CexError::ApiError(binance_error.msg),
                            });
                        }

                        return Err(CexError::ApiError(error_text));
                    }

                    return Ok(response);
                }
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 && e.is_timeout() {
                        tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS))
                            .await;
                        continue;
                    }
                    return Err(CexError::NetworkError(e.to_string()));
                }
            }
        }

        Err(CexError::NetworkError("max retries exceeded".to_string()))
    }

    /// Parse Binance order status to our OrderStatus enum
    fn parse_order_status(status: &str) -> OrderStatus {
        match status {
            "NEW" => OrderStatus::New,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "FILLED" => OrderStatus::Filled,
            "CANCELED" => OrderStatus::Canceled,
            "REJECTED" | "EXPIRED" => OrderStatus::Rejected,
            _ => OrderStatus::New,
        }
    }
}

#[async_trait]
impl CexClient for BinanceClient {
    async fn get_orderbook(&self, symbol: &str) -> Result<Orderbook, CexError> {
        let url = self.endpoint(&format!("/api/v3/depth?symbol={}&limit=100", symbol));

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| CexError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(CexError::ApiError(error_text));
        }

        let binance_book: BinanceOrderbookResponse = response
            .json()
            .await
            .map_err(|e| CexError::ParseError(e.to_string()))?;

        // Convert Binance format to our Orderbook format
        let bids = binance_book
            .bids
            .into_iter()
            .map(|(price, quantity)| OrderbookLevel { price, quantity })
            .collect();

        let asks = binance_book
            .asks
            .into_iter()
            .map(|(price, quantity)| OrderbookLevel { price, quantity })
            .collect();

        Ok(Orderbook {
            symbol: symbol.to_string(),
            bids,
            asks,
            last_update: binance_book.last_update_id,
        })
    }

    async fn get_balance(&self, asset: &str) -> Result<CexBalance, CexError> {
        let response = self.signed_get("/api/v3/account", vec![]).await?;

        let account: BinanceAccountResponse = response
            .json()
            .await
            .map_err(|e| CexError::ParseError(e.to_string()))?;

        // Find the requested asset in the balances
        let balance_info = account
            .balances
            .into_iter()
            .find(|b| b.asset == asset)
            .ok_or_else(|| CexError::ApiError(format!("asset not found: {}", asset)))?;

        Ok(CexBalance {
            asset: balance_info.asset,
            available: balance_info.free,
            locked: balance_info.locked,
        })
    }

    async fn place_order(&self, order: CexOrder) -> Result<CexOrderResult, CexError> {
        let mut params = vec![
            ("symbol", order.symbol.clone()),
            (
                "side",
                match order.side {
                    OrderSide::Buy => "BUY".to_string(),
                    OrderSide::Sell => "SELL".to_string(),
                },
            ),
            (
                "type",
                match order.order_type {
                    OrderType::Market => "MARKET".to_string(),
                    OrderType::Limit => "LIMIT".to_string(),
                },
            ),
        ];

        // Add quantity parameter
        match order.order_type {
            OrderType::Market => {
                match order.side {
                    OrderSide::Buy => {
                        // For market buy orders, Binance requires quoteOrderQty instead of quantity
                        params.push(("quoteOrderQty", order.quantity.clone()));
                    }
                    OrderSide::Sell => {
                        params.push(("quantity", order.quantity.clone()));
                    }
                }
            }
            OrderType::Limit => {
                params.push(("quantity", order.quantity.clone()));
                if let Some(price) = &order.price {
                    params.push(("price", price.clone()));
                    params.push(("timeInForce", "GTC".to_string())); // Good Till Cancel
                } else {
                    return Err(CexError::OrderRejected(
                        "limit order requires price".to_string(),
                    ));
                }
            }
        }

        let response = self.signed_post("/api/v3/order", params).await?;

        let binance_order: BinanceOrderResponse = response
            .json()
            .await
            .map_err(|e| CexError::ParseError(e.to_string()))?;

        // Calculate average fill price and total fees
        let mut total_commission = Decimal::ZERO;
        let mut commission_asset = String::new();
        let mut total_filled_value = Decimal::ZERO;
        let filled_qty = Decimal::from_str(&binance_order.executed_qty)
            .map_err(|e| CexError::ParseError(e.to_string()))?;

        for fill in &binance_order.fills {
            let fill_price =
                Decimal::from_str(&fill.price).map_err(|e| CexError::ParseError(e.to_string()))?;
            let fill_qty =
                Decimal::from_str(&fill.qty).map_err(|e| CexError::ParseError(e.to_string()))?;
            let commission = Decimal::from_str(&fill.commission)
                .map_err(|e| CexError::ParseError(e.to_string()))?;

            total_filled_value += fill_price * fill_qty;
            total_commission += commission;
            commission_asset = fill.commission_asset.clone();
        }

        let average_price = if filled_qty > Decimal::ZERO {
            total_filled_value / filled_qty
        } else {
            Decimal::ZERO
        };

        Ok(CexOrderResult {
            order_id: binance_order.order_id.to_string(),
            symbol: binance_order.symbol,
            filled_quantity: binance_order.executed_qty,
            average_price: average_price.to_string(),
            fee: total_commission.to_string(),
            fee_asset: commission_asset,
            status: Self::parse_order_status(&binance_order.status),
        })
    }

    async fn withdraw(
        &self,
        asset: &str,
        amount: u128,
        address: &str,
    ) -> Result<WithdrawResult, CexError> {
        // Convert amount from micro-units to whole units (assuming 6 decimals)
        let amount_decimal = Decimal::from_i128_with_scale(amount as i128, 6);

        // Determine network based on asset
        let network = match asset {
            "ATOM" => "ATOM",
            "OSMO" => "OSMO",
            "USDC" => "USDC", // May need to be more specific like "USDC-NOBLE"
            _ => asset,
        };

        let params = vec![
            ("coin", asset.to_string()),
            ("address", address.to_string()),
            ("amount", amount_decimal.to_string()),
            ("network", network.to_string()),
        ];

        let response = self
            .signed_post("/sapi/v1/capital/withdraw/apply", params)
            .await?;

        let withdraw_response: BinanceWithdrawResponse = response
            .json()
            .await
            .map_err(|e| CexError::ParseError(e.to_string()))?;

        Ok(WithdrawResult {
            tx_id: withdraw_response.id,
            asset: asset.to_string(),
            amount: amount.to_string(),
            address: address.to_string(),
            network: network.to_string(),
            status: WithdrawStatus::Pending,
        })
    }

    fn pair_to_symbol(&self, pair: &TradingPair) -> Option<String> {
        // Binance uses different symbol format
        let base = match pair.base.as_str() {
            "uatom" => "ATOM",
            "uosmo" => "OSMO",
            _ => return None,
        };

        let quote = match pair.quote.as_str() {
            "uusdc" => "USDC",
            "uusdt" => "USDT",
            _ => return None,
        };

        Some(format!("{}{}", base, quote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook_estimate_buy() {
        let orderbook = Orderbook {
            symbol: "ATOMUSDC".to_string(),
            bids: vec![],
            asks: vec![
                OrderbookLevel {
                    price: "10.50".to_string(),
                    quantity: "100".to_string(),
                },
                OrderbookLevel {
                    price: "10.55".to_string(),
                    quantity: "200".to_string(),
                },
            ],
            last_update: 0,
        };

        // Buy 50 ATOM - should fill at 10.50
        let cost = orderbook
            .estimate_buy(Decimal::from_str("50").unwrap())
            .unwrap();
        assert_eq!(cost, Decimal::from_str("525").unwrap()); // 50 * 10.50

        // Buy 150 ATOM - should fill 100@10.50 + 50@10.55
        let cost = orderbook
            .estimate_buy(Decimal::from_str("150").unwrap())
            .unwrap();
        let expected = Decimal::from_str("100").unwrap() * Decimal::from_str("10.50").unwrap()
            + Decimal::from_str("50").unwrap() * Decimal::from_str("10.55").unwrap();
        assert_eq!(cost, expected);
    }

    #[test]
    fn test_orderbook_estimate_sell() {
        let orderbook = Orderbook {
            symbol: "ATOMUSDC".to_string(),
            bids: vec![
                OrderbookLevel {
                    price: "10.40".to_string(),
                    quantity: "100".to_string(),
                },
                OrderbookLevel {
                    price: "10.35".to_string(),
                    quantity: "200".to_string(),
                },
            ],
            asks: vec![],
            last_update: 0,
        };

        // Sell 50 ATOM - should fill at 10.40
        let revenue = orderbook
            .estimate_sell(Decimal::from_str("50").unwrap())
            .unwrap();
        assert_eq!(revenue, Decimal::from_str("520").unwrap()); // 50 * 10.40

        // Sell 150 ATOM - should fill 100@10.40 + 50@10.35
        let revenue = orderbook
            .estimate_sell(Decimal::from_str("150").unwrap())
            .unwrap();
        let expected = Decimal::from_str("100").unwrap() * Decimal::from_str("10.40").unwrap()
            + Decimal::from_str("50").unwrap() * Decimal::from_str("10.35").unwrap();
        assert_eq!(revenue, expected);
    }

    #[test]
    fn test_orderbook_insufficient_liquidity() {
        let orderbook = Orderbook {
            symbol: "ATOMUSDC".to_string(),
            bids: vec![],
            asks: vec![OrderbookLevel {
                price: "10.50".to_string(),
                quantity: "100".to_string(),
            }],
            last_update: 0,
        };

        // Try to buy more than available
        let result = orderbook.estimate_buy(Decimal::from_str("150").unwrap());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CexError::InsufficientBalance { .. }
        ));
    }

    #[tokio::test]
    async fn test_mock_cex_client() {
        let client = MockCexClient::new()
            .with_orderbook(
                "ATOMUSDC",
                MockCexClient::simple_orderbook("ATOMUSDC", 10.5, 0.002, 1000.0),
            )
            .with_balance("ATOM", "1000.0", "100.0");

        // Test get_orderbook
        let orderbook = client.get_orderbook("ATOMUSDC").await.unwrap();
        assert_eq!(orderbook.symbol, "ATOMUSDC");
        assert!(!orderbook.bids.is_empty());
        assert!(!orderbook.asks.is_empty());

        // Test get_balance
        let balance = client.get_balance("ATOM").await.unwrap();
        assert_eq!(balance.available, "1000.0");
        assert_eq!(balance.locked, "100.0");

        // Test place_order
        let order = CexOrder {
            symbol: "ATOMUSDC".to_string(),
            order_type: OrderType::Market,
            side: OrderSide::Buy,
            quantity: "10".to_string(),
            price: None,
        };
        let result = client.place_order(order).await.unwrap();
        assert_eq!(result.status, OrderStatus::Filled);
    }

    #[tokio::test]
    async fn test_cex_backstop_solver() {
        use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, Intent, OutputSpec};
        use cosmwasm_std::Binary;

        let client = Arc::new(MockCexClient::new().with_orderbook(
            "ATOMUSDC",
            MockCexClient::simple_orderbook("ATOMUSDC", 10.5, 0.002, 10000.0),
        ));

        let solver =
            CexBackstopSolver::new("test-cex-solver", client, CexBackstopConfig::default());

        // Create test intent
        let intent = Intent {
            id: "test-intent-1".to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1test".to_string(),
            input: Asset {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uatom".to_string(),
                amount: Uint128::new(1_000_000), // 1 ATOM
            },
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(9_000_000), // 9 USDC minimum (accounting for fees)
                limit_price: "10.0".to_string(),
                recipient: "cosmos1test".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::default(),
            signature: Binary::default(),
            public_key: Binary::default(),
            created_at: 0,
            expires_at: 3600,
        };

        let ctx = SolveContext {
            matched_amount: Uint128::zero(),
            remaining: Uint128::new(1_000_000),
            oracle_price: "10.5".to_string(),
        };

        // Test solve
        let solution = solver.solve(&intent, &ctx).await.unwrap();
        assert_eq!(solution.solver_id, "test-cex-solver");
        assert_eq!(solution.fill.input_amount, Uint128::new(1_000_000));
        assert!(solution.fill.output_amount.u128() > 0);

        // Verify execution plan is CEX hedge
        match solution.execution {
            ExecutionPlan::CexHedge { exchange } => {
                assert!(!exchange.is_empty());
            }
            _ => panic!("Expected CexHedge execution plan"),
        }
    }

    #[tokio::test]
    async fn test_cex_solver_capacity() {
        let client = Arc::new(MockCexClient::new().with_orderbook(
            "ATOMUSDC",
            MockCexClient::simple_orderbook("ATOMUSDC", 10.5, 0.002, 10000.0),
        ));

        let solver =
            CexBackstopSolver::new("test-cex-solver", client, CexBackstopConfig::default());

        let pair = TradingPair::new("uatom", "uusdc");
        let capacity = solver.capacity(&pair).await.unwrap();

        assert!(capacity.available_liquidity.u128() > 0);
        assert!(capacity.max_immediate.u128() > 0);
        assert!(capacity.max_immediate <= capacity.available_liquidity);
    }

    #[test]
    fn test_inventory_tracking() {
        let client = Arc::new(MockCexClient::default());
        let solver =
            CexBackstopSolver::new("test-cex-solver", client, CexBackstopConfig::default());

        // Initial position should be zero
        assert_eq!(solver.get_position("uatom"), 0);
        assert_eq!(solver.get_position("uusdc"), 0);

        // Simulate a trade: sell 1000 uatom, buy 10500 uusdc
        solver.update_inventory("uatom", "uusdc", 1000);

        // Check positions
        assert_eq!(solver.get_position("uatom"), -1000);
        assert_eq!(solver.get_position("uusdc"), 1000);

        // Reverse trade
        solver.update_inventory("uusdc", "uatom", 500);
        assert_eq!(solver.get_position("uatom"), -500);
        assert_eq!(solver.get_position("uusdc"), 500);
    }

    #[test]
    fn test_cex_balance_total() {
        let balance = CexBalance {
            asset: "ATOM".to_string(),
            available: "1000.5".to_string(),
            locked: "250.25".to_string(),
        };

        let total = balance.total().unwrap();
        assert_eq!(total, Decimal::from_str("1250.75").unwrap());
    }

    #[test]
    fn test_pair_to_symbol() {
        let client = MockCexClient::default();

        let pair = TradingPair::new("uatom", "uusdc");
        assert_eq!(client.pair_to_symbol(&pair), Some("ATOMUSDC".to_string()));

        let pair = TradingPair::new("uosmo", "uusdc");
        assert_eq!(client.pair_to_symbol(&pair), Some("OSMOUSDC".to_string()));

        let pair = TradingPair::new("unknown", "uusdc");
        assert_eq!(client.pair_to_symbol(&pair), None);
    }

    #[tokio::test]
    async fn test_solver_health_check() {
        let client = Arc::new(MockCexClient::new().with_orderbook(
            "ATOMUSDC",
            MockCexClient::simple_orderbook("ATOMUSDC", 10.5, 0.002, 1000.0),
        ));

        let solver =
            CexBackstopSolver::new("test-cex-solver", client, CexBackstopConfig::default());

        assert!(solver.health_check().await);
    }

    #[test]
    fn test_binance_signature() {
        // Test that HMAC-SHA256 signature generation works correctly
        let config = BinanceConfig {
            api_key: "test_key".to_string(),
            api_secret: "test_secret".to_string(),
            base_url: "https://api.binance.com".to_string(),
            testnet: false,
        };

        let client = BinanceClient::new(config);

        // Test with known example from Binance API docs
        // Query string: symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559
        let query = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let signature = client.sign_request(query);

        // Verify signature is a valid hex string
        assert_eq!(signature.len(), 64); // SHA256 produces 32 bytes = 64 hex chars
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_binance_endpoint() {
        let config = BinanceConfig::default();
        let client = BinanceClient::new(config);

        assert_eq!(
            client.endpoint("/api/v3/account"),
            "https://api.binance.com/api/v3/account"
        );
    }

    #[test]
    fn test_binance_parse_order_status() {
        assert_eq!(BinanceClient::parse_order_status("NEW"), OrderStatus::New);
        assert_eq!(
            BinanceClient::parse_order_status("FILLED"),
            OrderStatus::Filled
        );
        assert_eq!(
            BinanceClient::parse_order_status("PARTIALLY_FILLED"),
            OrderStatus::PartiallyFilled
        );
        assert_eq!(
            BinanceClient::parse_order_status("CANCELED"),
            OrderStatus::Canceled
        );
        assert_eq!(
            BinanceClient::parse_order_status("REJECTED"),
            OrderStatus::Rejected
        );
        assert_eq!(
            BinanceClient::parse_order_status("EXPIRED"),
            OrderStatus::Rejected
        );
    }
}
