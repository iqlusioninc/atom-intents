use async_trait::async_trait;
use atom_intents_types::TradingPair;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::RwLock;

/// Helper function to compute 10^exp for Decimal
fn decimal_pow10(exp: i32) -> Decimal {
    if exp >= 0 {
        let mut result = Decimal::ONE;
        for _ in 0..exp {
            result *= Decimal::from(10u64);
        }
        result
    } else {
        let mut result = Decimal::ONE;
        for _ in 0..(-exp) {
            result *= Decimal::from(10u64);
        }
        Decimal::ONE / result
    }
}

/// Oracle error types
#[derive(Debug, Error)]
pub enum OracleError {
    #[error("price data is stale: age {age_seconds}s exceeds threshold {threshold_seconds}s")]
    Stale {
        age_seconds: u64,
        threshold_seconds: u64,
    },

    #[error("price not found for pair {pair}")]
    NotFound { pair: String },

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("invalid price: {0}")]
    InvalidPrice(String),

    #[error("invalid confidence: {0}")]
    InvalidConfidence(String),

    #[error("no oracle sources available")]
    NoSourcesAvailable,

    #[error("all oracle sources failed")]
    AllSourcesFailed,
}

/// Oracle price data with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePrice {
    /// Price as a Decimal
    pub price: Decimal,

    /// Unix timestamp when price was recorded
    pub timestamp: u64,

    /// Confidence interval (e.g., 0.01 = 1% uncertainty)
    pub confidence: Decimal,

    /// Source identifier (e.g., "pyth", "chainlink", "slinky")
    pub source: String,
}

impl OraclePrice {
    pub fn new(price: Decimal, timestamp: u64, confidence: Decimal, source: String) -> Self {
        Self {
            price,
            timestamp,
            confidence,
            source,
        }
    }

    /// Check if price is stale based on age threshold
    pub fn is_stale(&self, max_age_seconds: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now - self.timestamp > max_age_seconds
    }

    /// Get age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }
}

/// Price oracle trait
#[async_trait]
pub trait PriceOracle: Send + Sync {
    /// Get price for a trading pair
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError>;

    /// Get oracle identifier
    fn id(&self) -> &str;

    /// Check if oracle supports a trading pair
    fn supports_pair(&self, pair: &TradingPair) -> bool;

    /// Health check
    async fn health_check(&self) -> bool;
}

/// Mock oracle for testing
pub struct MockOracle {
    id: String,
    prices: Arc<RwLock<HashMap<String, OraclePrice>>>,
}

impl MockOracle {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            prices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set a price for testing
    pub async fn set_price(
        &self,
        pair: &TradingPair,
        price: Decimal,
        confidence: Decimal,
    ) -> Result<(), OracleError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let oracle_price = OraclePrice::new(price, timestamp, confidence, self.id.clone());

        self.prices
            .write()
            .await
            .insert(pair.to_symbol(), oracle_price);

        Ok(())
    }

    /// Clear all prices for testing
    pub async fn clear_prices(&self) {
        self.prices.write().await.clear();
    }
}

#[async_trait]
impl PriceOracle for MockOracle {
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError> {
        self.prices
            .read()
            .await
            .get(&pair.to_symbol())
            .cloned()
            .ok_or_else(|| OracleError::NotFound {
                pair: pair.to_symbol(),
            })
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        // Mock oracle can support any pair if configured
        futures::executor::block_on(async {
            self.prices.read().await.contains_key(&pair.to_symbol())
        })
    }

    async fn health_check(&self) -> bool {
        true
    }
}

/// Pyth Network oracle implementation
pub struct PythOracle {
    id: String,
    client: Client,
    base_url: String,
    feed_ids: HashMap<String, String>, // pair symbol -> feed ID
}

impl PythOracle {
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut feed_ids = HashMap::new();

        // Common Cosmos pairs - these are example feed IDs
        // In production, use actual Pyth feed IDs from https://pyth.network/developers/price-feed-ids
        feed_ids.insert("ATOM/USDC".to_string(), "b00b60f88b03a6a625a8d1c048c3f66653edf217439983d037e7222c4e612819".to_string());
        feed_ids.insert("OSMO/USDC".to_string(), "5867f5683c757393a0670ef0f701490950fe93fdb006d181c8265a831ac0c5c6".to_string());
        feed_ids.insert("TIA/USDC".to_string(), "09f7c1d7dfbb7df2b8fe3d3d87ee94a2259d212da4f30c1f0540d066dfa44723".to_string());

        Self {
            id: "pyth".to_string(),
            client: Client::new(),
            base_url: base_url.into(),
            feed_ids,
        }
    }

    pub fn with_feed_id(mut self, pair: impl Into<String>, feed_id: impl Into<String>) -> Self {
        self.feed_ids.insert(pair.into(), feed_id.into());
        self
    }
}

#[derive(Debug, Deserialize)]
struct PythPriceResponse {
    price: PythPrice,
}

#[derive(Debug, Deserialize)]
struct PythPrice {
    price: String,
    conf: String,
    expo: i32,
    publish_time: u64,
}

#[async_trait]
impl PriceOracle for PythOracle {
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError> {
        let symbol = pair.to_symbol();
        let feed_id = self
            .feed_ids
            .get(&symbol)
            .ok_or_else(|| OracleError::NotFound {
                pair: symbol.clone(),
            })?;

        let url = format!("{}/api/latest_price_feeds?ids[]={}", self.base_url, feed_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(OracleError::NetworkError(format!(
                "Pyth API returned status {}",
                response.status()
            )));
        }

        let data: Vec<PythPriceResponse> = response
            .json()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        let price_data = data
            .first()
            .ok_or_else(|| OracleError::NotFound {
                pair: symbol.clone(),
            })?;

        // Parse price with exponent
        let price_raw = Decimal::from_str(&price_data.price.price)
            .map_err(|e| OracleError::InvalidPrice(e.to_string()))?;

        let expo = decimal_pow10(price_data.price.expo);
        let price = price_raw * expo;

        // Calculate confidence as a percentage of price
        let conf_raw = Decimal::from_str(&price_data.price.conf)
            .map_err(|e| OracleError::InvalidConfidence(e.to_string()))?;
        let conf = conf_raw * expo;
        let confidence = if price.is_zero() {
            Decimal::ZERO
        } else {
            conf / price
        };

        Ok(OraclePrice::new(
            price,
            price_data.price.publish_time,
            confidence,
            self.id.clone(),
        ))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        self.feed_ids.contains_key(&pair.to_symbol())
    }

    async fn health_check(&self) -> bool {
        // Try to reach the Pyth API
        self.client
            .get(&self.base_url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Chainlink oracle implementation (via CosmWasm contract queries)
pub struct ChainlinkOracle {
    id: String,
    client: Client,
    rpc_endpoint: String,
    contract_addresses: HashMap<String, String>, // pair symbol -> contract address
}

impl ChainlinkOracle {
    pub fn new(rpc_endpoint: impl Into<String>) -> Self {
        Self {
            id: "chainlink".to_string(),
            client: Client::new(),
            rpc_endpoint: rpc_endpoint.into(),
            contract_addresses: HashMap::new(),
        }
    }

    pub fn with_feed(mut self, pair: impl Into<String>, contract: impl Into<String>) -> Self {
        self.contract_addresses
            .insert(pair.into(), contract.into());
        self
    }
}

#[derive(Debug, Serialize)]
struct ChainlinkQueryMsg {
    #[serde(rename = "latest_round_data")]
    latest_round_data: EmptyStruct,
}

#[derive(Debug, Serialize)]
struct EmptyStruct {}

#[derive(Debug, Deserialize)]
struct ChainlinkRoundData {
    answer: String,
    updated_at: u64,
}

#[async_trait]
impl PriceOracle for ChainlinkOracle {
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError> {
        let symbol = pair.to_symbol();
        let contract = self
            .contract_addresses
            .get(&symbol)
            .ok_or_else(|| OracleError::NotFound {
                pair: symbol.clone(),
            })?;

        // Query CosmWasm contract via RPC
        let query_msg = ChainlinkQueryMsg {
            latest_round_data: EmptyStruct {},
        };
        let query_data = serde_json::to_string(&query_msg)
            .map_err(|e| OracleError::InvalidPrice(e.to_string()))?;

        // Encode the query data in base64
        use base64::{engine::general_purpose, Engine as _};
        let encoded_query = general_purpose::STANDARD.encode(&query_data);

        // This is a simplified version - in production, use cosmwasm_std::WasmQuery
        let url = format!(
            "{}/cosmwasm/wasm/v1/contract/{}/smart/{}",
            self.rpc_endpoint, contract, encoded_query
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(OracleError::NetworkError(format!(
                "Chainlink query failed with status {}",
                response.status()
            )));
        }

        let data: ChainlinkRoundData = response
            .json()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        let price = Decimal::from_str(&data.answer)
            .map_err(|e| OracleError::InvalidPrice(e.to_string()))?;

        // Chainlink typically uses 8 decimals
        let price = price / Decimal::from(100_000_000u64);

        // Chainlink doesn't provide confidence intervals, use a default
        let confidence = Decimal::from_str("0.001").unwrap(); // 0.1%

        Ok(OraclePrice::new(
            price,
            data.updated_at,
            confidence,
            self.id.clone(),
        ))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        self.contract_addresses.contains_key(&pair.to_symbol())
    }

    async fn health_check(&self) -> bool {
        // Try to reach the RPC endpoint
        self.client
            .get(&self.rpc_endpoint)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Skip's Slinky oracle implementation
pub struct SlinkyOracle {
    id: String,
    client: Client,
    rpc_endpoint: String,
}

impl SlinkyOracle {
    pub fn new(rpc_endpoint: impl Into<String>) -> Self {
        Self {
            id: "slinky".to_string(),
            client: Client::new(),
            rpc_endpoint: rpc_endpoint.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct SlinkyPriceResponse {
    price: SlinkyPrice,
}

#[derive(Debug, Deserialize)]
struct SlinkyPrice {
    price: String,
    decimals: u32,
    timestamp: u64,
}

#[async_trait]
impl PriceOracle for SlinkyOracle {
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError> {
        let symbol = pair.to_symbol();

        // Slinky uses currency pair format
        let url = format!(
            "{}/slinky/oracle/v1/get_price?currency_pair={}",
            self.rpc_endpoint, symbol
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(OracleError::NetworkError(format!(
                "Slinky API returned status {}",
                response.status()
            )));
        }

        let data: SlinkyPriceResponse = response
            .json()
            .await
            .map_err(|e| OracleError::NetworkError(e.to_string()))?;

        let price_raw = Decimal::from_str(&data.price.price)
            .map_err(|e| OracleError::InvalidPrice(e.to_string()))?;

        // Adjust for decimals
        let decimals = decimal_pow10(data.price.decimals as i32);
        let price = price_raw / decimals;

        // Slinky doesn't provide confidence, use default
        let confidence = Decimal::from_str("0.001").unwrap(); // 0.1%

        Ok(OraclePrice::new(
            price,
            data.price.timestamp,
            confidence,
            self.id.clone(),
        ))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_pair(&self, _pair: &TradingPair) -> bool {
        // Slinky supports most major pairs
        true
    }

    async fn health_check(&self) -> bool {
        self.client
            .get(&self.rpc_endpoint)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Aggregated oracle that combines multiple sources
pub struct AggregatedOracle {
    id: String,
    sources: Vec<Arc<dyn PriceOracle>>,
    max_age_seconds: u64,
    min_sources: usize,
}

impl AggregatedOracle {
    pub fn new(sources: Vec<Arc<dyn PriceOracle>>) -> Self {
        Self {
            id: "aggregated".to_string(),
            sources,
            max_age_seconds: 60, // 1 minute default
            min_sources: 1,
        }
    }

    pub fn with_max_age(mut self, seconds: u64) -> Self {
        self.max_age_seconds = seconds;
        self
    }

    pub fn with_min_sources(mut self, min: usize) -> Self {
        self.min_sources = min;
        self
    }

    /// Calculate median from a list of prices
    fn median_price(mut prices: Vec<Decimal>) -> Decimal {
        if prices.is_empty() {
            return Decimal::ZERO;
        }

        prices.sort();
        let len = prices.len();

        if len % 2 == 0 {
            // Even number: average of two middle values
            let mid1 = prices[len / 2 - 1];
            let mid2 = prices[len / 2];
            (mid1 + mid2) / Decimal::from(2)
        } else {
            // Odd number: middle value
            prices[len / 2]
        }
    }

    /// Calculate weighted average confidence
    fn avg_confidence(prices: &[OraclePrice]) -> Decimal {
        if prices.is_empty() {
            return Decimal::ZERO;
        }

        let sum: Decimal = prices.iter().map(|p| p.confidence).sum();
        sum / Decimal::from(prices.len())
    }
}

#[async_trait]
impl PriceOracle for AggregatedOracle {
    async fn get_price(&self, pair: &TradingPair) -> Result<OraclePrice, OracleError> {
        if self.sources.is_empty() {
            return Err(OracleError::NoSourcesAvailable);
        }

        // Query all sources in parallel
        let futures: Vec<_> = self
            .sources
            .iter()
            .filter(|s| s.supports_pair(pair))
            .map(|source| async move { source.get_price(pair).await })
            .collect();

        if futures.is_empty() {
            return Err(OracleError::NotFound {
                pair: pair.to_symbol(),
            });
        }

        let results = futures::future::join_all(futures).await;

        // Filter successful, non-stale prices
        let valid_prices: Vec<OraclePrice> = results
            .into_iter()
            .filter_map(|r| r.ok())
            .filter(|p| !p.is_stale(self.max_age_seconds))
            .collect();

        if valid_prices.is_empty() {
            return Err(OracleError::AllSourcesFailed);
        }

        if valid_prices.len() < self.min_sources {
            return Err(OracleError::AllSourcesFailed);
        }

        // Calculate median price
        let price_values: Vec<Decimal> = valid_prices.iter().map(|p| p.price).collect();
        let median = Self::median_price(price_values);

        // Use most recent timestamp
        let latest_timestamp = valid_prices.iter().map(|p| p.timestamp).max().unwrap_or(0);

        // Average confidence
        let avg_conf = Self::avg_confidence(&valid_prices);

        // Build source list
        let sources: Vec<String> = valid_prices.iter().map(|p| p.source.clone()).collect();
        let source = format!("aggregated[{}]", sources.join(","));

        Ok(OraclePrice::new(
            median,
            latest_timestamp,
            avg_conf,
            source,
        ))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        self.sources.iter().any(|s| s.supports_pair(pair))
    }

    async fn health_check(&self) -> bool {
        // At least one source should be healthy
        let futures: Vec<_> = self.sources.iter().map(|s| s.health_check()).collect();
        let results = futures::future::join_all(futures).await;
        results.iter().any(|&healthy| healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_oracle_basic() {
        let oracle = MockOracle::new("test");
        let pair = TradingPair::new("ATOM", "USDC");

        // Price not set yet
        assert!(oracle.get_price(&pair).await.is_err());

        // Set price
        let price = Decimal::from_str("10.45").unwrap();
        let confidence = Decimal::from_str("0.01").unwrap();
        oracle.set_price(&pair, price, confidence).await.unwrap();

        // Retrieve price
        let result = oracle.get_price(&pair).await.unwrap();
        assert_eq!(result.price, price);
        assert_eq!(result.confidence, confidence);
        assert_eq!(result.source, "test");
    }

    #[tokio::test]
    async fn test_mock_oracle_supports_pair() {
        let oracle = MockOracle::new("test");
        let pair = TradingPair::new("ATOM", "USDC");

        // Initially doesn't support
        assert!(!oracle.supports_pair(&pair));

        // After setting price, supports
        oracle
            .set_price(
                &pair,
                Decimal::from_str("10.45").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        assert!(oracle.supports_pair(&pair));
    }

    #[tokio::test]
    async fn test_oracle_price_staleness() {
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 120; // 2 minutes ago

        let price = OraclePrice::new(
            Decimal::from_str("10.45").unwrap(),
            old_timestamp,
            Decimal::from_str("0.01").unwrap(),
            "test".to_string(),
        );

        assert!(price.is_stale(60)); // Stale if older than 1 minute
        assert!(!price.is_stale(180)); // Not stale if threshold is 3 minutes
    }

    #[tokio::test]
    async fn test_aggregated_oracle_median() {
        let mock1 = Arc::new(MockOracle::new("oracle1"));
        let mock2 = Arc::new(MockOracle::new("oracle2"));
        let mock3 = Arc::new(MockOracle::new("oracle3"));

        let pair = TradingPair::new("ATOM", "USDC");

        // Set different prices
        mock1
            .set_price(
                &pair,
                Decimal::from_str("10.00").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        mock2
            .set_price(
                &pair,
                Decimal::from_str("10.50").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        mock3
            .set_price(
                &pair,
                Decimal::from_str("11.00").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregated = AggregatedOracle::new(vec![
            mock1 as Arc<dyn PriceOracle>,
            mock2 as Arc<dyn PriceOracle>,
            mock3 as Arc<dyn PriceOracle>,
        ]);

        let result = aggregated.get_price(&pair).await.unwrap();

        // Median of [10.00, 10.50, 11.00] is 10.50
        assert_eq!(result.price, Decimal::from_str("10.50").unwrap());
        assert!(result.source.contains("aggregated"));
    }

    #[tokio::test]
    async fn test_aggregated_oracle_staleness_rejection() {
        let mock = Arc::new(MockOracle::new("oracle1"));
        let pair = TradingPair::new("ATOM", "USDC");

        // Create a stale price manually
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 120; // 2 minutes ago

        let stale_price = OraclePrice::new(
            Decimal::from_str("10.45").unwrap(),
            old_timestamp,
            Decimal::from_str("0.01").unwrap(),
            "oracle1".to_string(),
        );

        mock.prices
            .write()
            .await
            .insert(pair.to_symbol(), stale_price);

        let aggregated = AggregatedOracle::new(vec![mock as Arc<dyn PriceOracle>])
            .with_max_age(60); // 1 minute threshold

        // Should fail because price is stale
        let result = aggregated.get_price(&pair).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_aggregated_oracle_fallback() {
        let mock1 = Arc::new(MockOracle::new("oracle1"));
        let mock2 = Arc::new(MockOracle::new("oracle2"));

        let pair = TradingPair::new("ATOM", "USDC");

        // Only mock2 has the price
        mock2
            .set_price(
                &pair,
                Decimal::from_str("10.45").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        let aggregated = AggregatedOracle::new(vec![
            mock1 as Arc<dyn PriceOracle>,
            mock2 as Arc<dyn PriceOracle>,
        ]);

        let result = aggregated.get_price(&pair).await.unwrap();
        assert_eq!(result.price, Decimal::from_str("10.45").unwrap());
    }

    #[tokio::test]
    async fn test_aggregated_oracle_no_sources() {
        let aggregated = AggregatedOracle::new(vec![]);
        let pair = TradingPair::new("ATOM", "USDC");

        let result = aggregated.get_price(&pair).await;
        assert!(matches!(result, Err(OracleError::NoSourcesAvailable)));
    }

    #[tokio::test]
    async fn test_aggregated_oracle_all_fail() {
        let mock = Arc::new(MockOracle::new("oracle1"));
        let pair = TradingPair::new("ATOM", "USDC");

        // Don't set any price, so query will fail

        let aggregated = AggregatedOracle::new(vec![mock as Arc<dyn PriceOracle>]);

        let result = aggregated.get_price(&pair).await;
        assert!(matches!(result, Err(OracleError::NotFound { .. })));
    }

    #[test]
    fn test_median_calculation() {
        // Odd number of prices
        let prices = vec![
            Decimal::from_str("10.00").unwrap(),
            Decimal::from_str("10.50").unwrap(),
            Decimal::from_str("11.00").unwrap(),
        ];
        let median = AggregatedOracle::median_price(prices);
        assert_eq!(median, Decimal::from_str("10.50").unwrap());

        // Even number of prices
        let prices = vec![
            Decimal::from_str("10.00").unwrap(),
            Decimal::from_str("10.50").unwrap(),
            Decimal::from_str("11.00").unwrap(),
            Decimal::from_str("11.50").unwrap(),
        ];
        let median = AggregatedOracle::median_price(prices);
        assert_eq!(median, Decimal::from_str("10.75").unwrap());

        // Single price
        let prices = vec![Decimal::from_str("10.45").unwrap()];
        let median = AggregatedOracle::median_price(prices);
        assert_eq!(median, Decimal::from_str("10.45").unwrap());

        // Empty
        let prices = vec![];
        let median = AggregatedOracle::median_price(prices);
        assert_eq!(median, Decimal::ZERO);
    }

    #[tokio::test]
    async fn test_pyth_oracle_supports_pair() {
        let oracle = PythOracle::new("https://hermes.pyth.network");

        let atom_usdc = TradingPair::new("ATOM", "USDC");
        let osmo_usdc = TradingPair::new("OSMO", "USDC");
        let unknown = TradingPair::new("UNKNOWN", "USDC");

        assert!(oracle.supports_pair(&atom_usdc));
        assert!(oracle.supports_pair(&osmo_usdc));
        assert!(!oracle.supports_pair(&unknown));
    }

    #[tokio::test]
    async fn test_chainlink_oracle_with_feed() {
        let oracle = ChainlinkOracle::new("https://rpc.example.com")
            .with_feed("ATOM/USDC", "cosmos1contract...");

        let pair = TradingPair::new("ATOM", "USDC");
        assert!(oracle.supports_pair(&pair));

        let unknown = TradingPair::new("UNKNOWN", "USDC");
        assert!(!oracle.supports_pair(&unknown));
    }

    #[tokio::test]
    async fn test_slinky_oracle_supports_all() {
        let oracle = SlinkyOracle::new("https://rpc.example.com");

        let pair = TradingPair::new("ATOM", "USDC");
        assert!(oracle.supports_pair(&pair));
    }
}
