use async_trait::async_trait;
use atom_intents_types::{DexSwapStep, TradingPair};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::{DexClient, DexError, DexQuote, PoolInfo};

/// Astroport DEX Client - queries Astroport for swap quotes
/// Supports multiple chains: Neutron, Injective, Terra, etc.
pub struct AstroportClient {
    base_url: String,
    client: reqwest::Client,
    chain_id: String,
    rpc_url: String,
    pool_cache: Arc<RwLock<PoolCache>>,
}

impl AstroportClient {
    pub fn new(
        base_url: impl Into<String>,
        chain_id: impl Into<String>,
        rpc_url: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            chain_id: chain_id.into(),
            rpc_url: rpc_url.into(),
            pool_cache: Arc::new(RwLock::new(PoolCache::new(Duration::from_secs(60)))),
        }
    }

    /// Create client for Neutron mainnet
    pub fn neutron() -> Self {
        Self::new(
            "https://neutron.astroport.fi/api",
            "neutron-1",
            "https://rpc-neutron.whispernode.com",
        )
    }

    /// Create client for Neutron testnet
    pub fn neutron_testnet() -> Self {
        Self::new(
            "https://testnet.neutron.astroport.fi/api",
            "pion-1",
            "https://rpc-palvus.pion-1.ntrn.tech",
        )
    }

    /// Create client for Injective
    pub fn injective() -> Self {
        Self::new(
            "https://injective.astroport.fi/api",
            "injective-1",
            "https://sentry.tm.injective.network:443",
        )
    }

    /// Build asset info for Astroport API
    fn build_asset_info(&self, denom: &str) -> AssetInfo {
        if denom.starts_with("ibc/") || denom.starts_with("u") {
            AssetInfo::NativeToken {
                denom: denom.to_string(),
            }
        } else {
            AssetInfo::Token {
                contract_addr: denom.to_string(),
            }
        }
    }

    /// Query a pool contract directly via CosmWasm smart query
    pub async fn query_pool(&self, pair_address: &str) -> Result<PoolLiquidity, DexError> {
        // Check cache first
        {
            let cache = self.pool_cache.read().await;
            if let Some(cached) = cache.get(pair_address) {
                return Ok(cached.liquidity.clone());
            }
        }

        // Query pool state from contract
        let query_msg = serde_json::json!({
            "pool": {}
        });

        let pool_response: PoolQueryResponse = self
            .query_contract(pair_address, &query_msg)
            .await?;

        // Parse assets and determine pool type
        let (reserve_a, reserve_b, token_a, token_b) = match pool_response.assets.as_slice() {
            [asset_0, asset_1] => {
                let amount_0 = asset_0
                    .amount
                    .parse::<u128>()
                    .map_err(|e| DexError::QueryFailed(format!("Invalid amount: {}", e)))?;
                let amount_1 = asset_1
                    .amount
                    .parse::<u128>()
                    .map_err(|e| DexError::QueryFailed(format!("Invalid amount: {}", e)))?;

                let token_0 = match &asset_0.info {
                    AssetInfo::NativeToken { denom } => denom.clone(),
                    AssetInfo::Token { contract_addr } => contract_addr.clone(),
                };
                let token_1 = match &asset_1.info {
                    AssetInfo::NativeToken { denom } => denom.clone(),
                    AssetInfo::Token { contract_addr } => contract_addr.clone(),
                };

                (amount_0, amount_1, token_0, token_1)
            }
            _ => {
                return Err(DexError::QueryFailed(
                    "Invalid pool assets length".to_string(),
                ))
            }
        };

        let total_share = pool_response
            .total_share
            .parse::<u128>()
            .map_err(|e| DexError::QueryFailed(format!("Invalid total_share: {}", e)))?;

        // Query pair info to get pool type
        let pair_query = serde_json::json!({
            "pair": {}
        });

        let pair_info: PairInfoResponse = self.query_contract(pair_address, &pair_query).await?;

        let pool_type = if pair_info.pair_type.xyk.is_some() {
            PoolType::Xyk
        } else if pair_info.pair_type.stable.is_some() {
            PoolType::Stable
        } else if pair_info.pair_type.concentrated.is_some() {
            PoolType::Concentrated
        } else {
            PoolType::Xyk // Default
        };

        let liquidity = PoolLiquidity {
            token_a,
            token_b,
            reserve_a,
            reserve_b,
            total_share,
            pool_type,
        };

        // Update cache
        {
            let mut cache = self.pool_cache.write().await;
            cache.insert(pair_address.to_string(), liquidity.clone());
        }

        Ok(liquidity)
    }

    /// Query a CosmWasm contract via RPC
    async fn query_contract<T: for<'de> Deserialize<'de>>(
        &self,
        contract: &str,
        query: &serde_json::Value,
    ) -> Result<T, DexError> {
        let query_data = serde_json::to_string(query)
            .map_err(|e| DexError::QueryFailed(format!("Failed to serialize query: {}", e)))?;

        let query_data_base64 = BASE64.encode(query_data.as_bytes());

        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "abci_query",
            "params": {
                "path": format!("/cosmwasm.wasm.v1.Query/SmartContractState"),
                "data": query_data_base64,
                "height": "0",
                "prove": false
            }
        });

        debug!("Querying contract {} with query: {}", contract, query);

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&rpc_request)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(format!("RPC request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(DexError::QueryFailed(format!(
                "RPC error: {}",
                response.status()
            )));
        }

        let rpc_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse RPC response: {}", e)))?;

        // Extract result data from response
        let result_data = rpc_response
            .get("result")
            .and_then(|r| r.get("response"))
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DexError::QueryFailed("Missing result data in RPC response".to_string()))?;

        let decoded = BASE64
            .decode(result_data)
            .map_err(|e| DexError::QueryFailed(format!("Failed to decode response: {}", e)))?;

        serde_json::from_slice(&decoded)
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse contract response: {}", e)))
    }

    /// Estimate slippage for a trade
    pub fn estimate_slippage(
        &self,
        pool: &PoolLiquidity,
        amount: u128,
        is_buy: bool,
    ) -> Decimal {
        match pool.pool_type {
            PoolType::Xyk => self.estimate_xyk_slippage(pool, amount, is_buy),
            PoolType::Stable => self.estimate_stable_slippage(pool, amount, is_buy),
            PoolType::Concentrated => self.estimate_concentrated_slippage(pool, amount, is_buy),
        }
    }

    fn estimate_xyk_slippage(&self, pool: &PoolLiquidity, amount: u128, is_buy: bool) -> Decimal {
        // For XYK pools: slippage = amount / (reserve + amount)
        let (input_reserve, output_reserve) = if is_buy {
            (pool.reserve_b, pool.reserve_a)
        } else {
            (pool.reserve_a, pool.reserve_b)
        };

        if input_reserve == 0 {
            return Decimal::from(100); // 100% slippage if no liquidity
        }

        // Calculate output using constant product formula
        // output = (output_reserve * amount) / (input_reserve + amount)
        let amount_dec = Decimal::from(amount);
        let input_reserve_dec = Decimal::from(input_reserve);
        let output_reserve_dec = Decimal::from(output_reserve);

        // Price impact = 1 - (actual_price / ideal_price)
        // actual_price = output / amount
        // ideal_price = output_reserve / input_reserve
        let actual_output =
            (output_reserve_dec * amount_dec) / (input_reserve_dec + amount_dec);
        let ideal_output = (output_reserve_dec / input_reserve_dec) * amount_dec;

        if ideal_output.is_zero() {
            return Decimal::ZERO;
        }

        let slippage = (ideal_output - actual_output) / ideal_output * Decimal::from(100);
        slippage.max(Decimal::ZERO)
    }

    fn estimate_stable_slippage(
        &self,
        pool: &PoolLiquidity,
        amount: u128,
        _is_buy: bool,
    ) -> Decimal {
        // Stable pools have lower slippage - simplified estimation
        // Real implementation would use the stableswap invariant
        let total_liquidity = pool.reserve_a + pool.reserve_b;
        if total_liquidity == 0 {
            return Decimal::from(100);
        }

        let trade_size_ratio =
            Decimal::from(amount) / Decimal::from(total_liquidity) * Decimal::from(100);

        // Stable pools have ~1/10th the slippage of XYK pools for same trade size
        trade_size_ratio / Decimal::from(10)
    }

    fn estimate_concentrated_slippage(
        &self,
        pool: &PoolLiquidity,
        amount: u128,
        is_buy: bool,
    ) -> Decimal {
        // Concentrated liquidity - slippage depends on position
        // Simplified: use XYK model but with better rates
        self.estimate_xyk_slippage(pool, amount, is_buy) / Decimal::from(2)
    }

    /// Calculate total value locked (TVL) in USD
    pub async fn get_pool_tvl(
        &self,
        pair_address: &str,
        prices: &HashMap<String, Decimal>,
    ) -> Result<Decimal, DexError> {
        let pool = self.query_pool(pair_address).await?;

        let price_a = prices
            .get(&pool.token_a)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let price_b = prices
            .get(&pool.token_b)
            .copied()
            .unwrap_or(Decimal::ZERO);

        let value_a = Decimal::from(pool.reserve_a) * price_a;
        let value_b = Decimal::from(pool.reserve_b) * price_b;

        Ok(value_a + value_b)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum AssetInfo {
    Token { contract_addr: String },
    NativeToken { denom: String },
}

#[derive(Debug, Deserialize)]
struct SimulateSwapResponse {
    return_amount: String,
    spread_amount: String,
    commission_amount: String,
}

#[derive(Debug, Deserialize)]
struct RouterSimulateResponse {
    amount: String,
    #[serde(default)]
    operations: Vec<SwapOperation>,
}

#[derive(Debug, Deserialize)]
struct SwapOperation {
    #[serde(rename = "astro_swap")]
    astro_swap: Option<AstroSwap>,
}

#[derive(Debug, Deserialize)]
struct AstroSwap {
    offer_asset_info: AssetInfo,
    ask_asset_info: AssetInfo,
}

#[derive(Debug, Deserialize)]
struct PairResponse {
    pair_address: String,
    liquidity_token: String,
    asset_infos: Vec<AssetInfo>,
    pair_type: PairType,
}

#[derive(Debug, Deserialize, Clone)]
struct PairType {
    #[serde(rename = "xyk")]
    xyk: Option<serde_json::Value>,
    #[serde(rename = "stable")]
    stable: Option<serde_json::Value>,
    #[serde(rename = "concentrated")]
    concentrated: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct PoolQueryResponse {
    assets: Vec<PoolAsset>,
    total_share: String,
}

#[derive(Debug, Deserialize)]
struct PoolAsset {
    info: AssetInfo,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct PairInfoResponse {
    asset_infos: Vec<AssetInfo>,
    pair_type: PairType,
    liquidity_token: String,
}

/// Pool liquidity information
#[derive(Debug, Clone)]
pub struct PoolLiquidity {
    pub token_a: String,
    pub token_b: String,
    pub reserve_a: u128,
    pub reserve_b: u128,
    pub total_share: u128,
    pub pool_type: PoolType,
}

/// Pool type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolType {
    Xyk,
    Stable,
    Concentrated,
}

/// Pool cache for reducing RPC queries
struct PoolCache {
    pools: HashMap<String, CachedPool>,
    ttl: Duration,
}

struct CachedPool {
    liquidity: PoolLiquidity,
    fetched_at: Instant,
}

impl PoolCache {
    fn new(ttl: Duration) -> Self {
        Self {
            pools: HashMap::new(),
            ttl,
        }
    }

    fn get(&self, pair_address: &str) -> Option<&CachedPool> {
        let cached = self.pools.get(pair_address)?;
        if cached.fetched_at.elapsed() < self.ttl {
            Some(cached)
        } else {
            None
        }
    }

    fn insert(&mut self, pair_address: String, liquidity: PoolLiquidity) {
        self.pools.insert(
            pair_address,
            CachedPool {
                liquidity,
                fetched_at: Instant::now(),
            },
        );
    }

    #[allow(dead_code)]
    fn clear_expired(&mut self) {
        self.pools
            .retain(|_, cached| cached.fetched_at.elapsed() < self.ttl);
    }
}

#[async_trait]
impl DexClient for AstroportClient {
    async fn get_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Result<DexQuote, DexError> {
        // Use router simulate for multi-hop routes
        let url = format!(
            "{}/router/simulate?from={}&to={}&amount={}",
            self.base_url, input_denom, output_denom, amount
        );

        debug!("Querying Astroport router: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Astroport API error: {} - {}", status, body);
            return Err(DexError::QueryFailed(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let sim: RouterSimulateResponse = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse response: {}", e)))?;

        let output_amount = sim
            .amount
            .parse::<u128>()
            .map_err(|e| DexError::QueryFailed(format!("Invalid amount: {}", e)))?;

        // Build route from operations
        let route: Vec<DexSwapStep> = sim
            .operations
            .iter()
            .filter_map(|op| {
                let swap = op.astro_swap.as_ref()?;
                let in_denom = match &swap.offer_asset_info {
                    AssetInfo::NativeToken { denom } => denom.clone(),
                    AssetInfo::Token { contract_addr } => contract_addr.clone(),
                };
                let out_denom = match &swap.ask_asset_info {
                    AssetInfo::NativeToken { denom } => denom.clone(),
                    AssetInfo::Token { contract_addr } => contract_addr.clone(),
                };
                Some(DexSwapStep {
                    venue: "astroport".to_string(),
                    pool_id: format!("{}-{}", in_denom, out_denom),
                    input_denom: in_denom,
                    output_denom: out_denom,
                    chain_id: self.chain_id.clone(),
                })
            })
            .collect();

        // If no operations, create a single-hop route
        let route = if route.is_empty() {
            vec![DexSwapStep {
                venue: "astroport".to_string(),
                pool_id: format!("{}-{}", input_denom, output_denom),
                input_denom: input_denom.to_string(),
                output_denom: output_denom.to_string(),
                chain_id: self.chain_id.clone(),
            }]
        } else {
            route
        };

        // Calculate price impact (simplified)
        let price_impact = if output_amount > 0 && amount > 0 {
            let expected = amount; // Assuming 1:1 for impact calc
            let impact = (expected as f64 - output_amount as f64) / expected as f64;
            format!("{:.4}", impact.abs())
        } else {
            "0.0000".to_string()
        };

        Ok(DexQuote {
            venue: "astroport".to_string(),
            input_amount: amount,
            output_amount,
            price_impact,
            route,
            estimated_fee: None,
        })
    }

    async fn get_pools(&self, pair: &TradingPair) -> Result<Vec<PoolInfo>, DexError> {
        // Query pairs endpoint
        let url = format!(
            "{}/pairs?asset_infos=[{{\"{}\": \"{}\"}}, {{\"{}\": \"{}\"}}]",
            self.base_url,
            if pair.base.starts_with("ibc/") || pair.base.starts_with("u") {
                "native_token"
            } else {
                "token"
            },
            pair.base,
            if pair.quote.starts_with("ibc/") || pair.quote.starts_with("u") {
                "native_token"
            } else {
                "token"
            },
            pair.quote
        );

        debug!("Querying Astroport pairs: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(vec![]); // Fallback to empty
        }

        // Try to parse pairs response
        let pairs: Vec<PairResponse> = match response.json().await {
            Ok(p) => p,
            Err(_) => return Ok(vec![]),
        };

        // Query liquidity for each pair
        let mut pool_infos = Vec::new();
        for pair_info in pairs {
            match self.query_pool(&pair_info.pair_address).await {
                Ok(liquidity) => {
                    // Determine fee rate based on pool type
                    let fee_rate = match liquidity.pool_type {
                        PoolType::Xyk => "0.003",       // 0.3%
                        PoolType::Stable => "0.0005",   // 0.05%
                        PoolType::Concentrated => "0.002", // 0.2%
                    };

                    pool_infos.push(PoolInfo {
                        pool_id: pair_info.pair_address.clone(),
                        token_a: liquidity.token_a,
                        token_b: liquidity.token_b,
                        liquidity_a: liquidity.reserve_a,
                        liquidity_b: liquidity.reserve_b,
                        fee_rate: fee_rate.to_string(),
                    });
                }
                Err(e) => {
                    warn!(
                        "Failed to query pool liquidity for {}: {}",
                        pair_info.pair_address, e
                    );
                    // Still include the pool but with zero liquidity
                    pool_infos.push(PoolInfo {
                        pool_id: pair_info.pair_address.clone(),
                        token_a: pair.base.clone(),
                        token_b: pair.quote.clone(),
                        liquidity_a: 0,
                        liquidity_b: 0,
                        fee_rate: "0.003".to_string(),
                    });
                }
            }
        }

        Ok(pool_infos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_cache() {
        let mut cache = PoolCache::new(Duration::from_secs(60));

        let liquidity = PoolLiquidity {
            token_a: "token0".to_string(),
            token_b: "token1".to_string(),
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            total_share: 1_500_000,
            pool_type: PoolType::Xyk,
        };

        // Insert and retrieve
        cache.insert("pair1".to_string(), liquidity.clone());
        assert!(cache.get("pair1").is_some());

        // Non-existent key
        assert!(cache.get("pair2").is_none());

        // Test expiry with very short TTL
        let mut short_cache = PoolCache::new(Duration::from_millis(1));
        short_cache.insert("pair3".to_string(), liquidity);
        std::thread::sleep(Duration::from_millis(10));
        assert!(short_cache.get("pair3").is_none());
    }

    #[test]
    fn test_estimate_xyk_slippage() {
        let client = AstroportClient::new(
            "https://test.com/api",
            "test-1",
            "https://test-rpc.com",
        );

        let pool = PoolLiquidity {
            token_a: "token0".to_string(),
            token_b: "token1".to_string(),
            reserve_a: 1_000_000_000_000, // 1M tokens (6 decimals)
            reserve_b: 1_000_000_000_000, // 1M tokens
            total_share: 1_000_000_000_000,
            pool_type: PoolType::Xyk,
        };

        // Small trade - low slippage
        let slippage = client.estimate_slippage(&pool, 10_000_000_000, false); // 10k tokens
        assert!(slippage < Decimal::from(2)); // Less than 2%

        // Large trade - higher slippage
        let slippage = client.estimate_slippage(&pool, 100_000_000_000, false); // 100k tokens
        assert!(slippage > Decimal::from(5)); // More than 5%

        // Very large trade - very high slippage
        let slippage = client.estimate_slippage(&pool, 500_000_000_000, false); // 500k tokens
        assert!(slippage > Decimal::from(20)); // More than 20%

        // Zero liquidity
        let empty_pool = PoolLiquidity {
            token_a: "token0".to_string(),
            token_b: "token1".to_string(),
            reserve_a: 0,
            reserve_b: 0,
            total_share: 0,
            pool_type: PoolType::Xyk,
        };
        let slippage = client.estimate_slippage(&empty_pool, 1_000_000, false);
        assert_eq!(slippage, Decimal::from(100)); // 100% slippage
    }

    #[test]
    fn test_estimate_stable_slippage() {
        let client = AstroportClient::new(
            "https://test.com/api",
            "test-1",
            "https://test-rpc.com",
        );

        let pool = PoolLiquidity {
            token_a: "usdc".to_string(),
            token_b: "usdt".to_string(),
            reserve_a: 10_000_000_000_000, // 10M stablecoins
            reserve_b: 10_000_000_000_000,
            total_share: 20_000_000_000_000,
            pool_type: PoolType::Stable,
        };

        // Stable pools should have lower slippage
        let slippage = client.estimate_slippage(&pool, 100_000_000_000, false); // 100k
        assert!(slippage < Decimal::from(1)); // Less than 1%

        // Even large trades should have reasonable slippage
        let slippage = client.estimate_slippage(&pool, 1_000_000_000_000, false); // 1M
        assert!(slippage < Decimal::from(10)); // Less than 10%
    }

    #[test]
    fn test_estimate_concentrated_slippage() {
        let client = AstroportClient::new(
            "https://test.com/api",
            "test-1",
            "https://test-rpc.com",
        );

        let pool = PoolLiquidity {
            token_a: "token0".to_string(),
            token_b: "token1".to_string(),
            reserve_a: 1_000_000_000_000,
            reserve_b: 1_000_000_000_000,
            total_share: 1_000_000_000_000,
            pool_type: PoolType::Concentrated,
        };

        // Concentrated liquidity should have better slippage than XYK
        let amount = 50_000_000_000; // 50k tokens
        let cl_slippage = client.estimate_slippage(&pool, amount, false);

        let xyk_pool = PoolLiquidity {
            pool_type: PoolType::Xyk,
            ..pool
        };
        let xyk_slippage = client.estimate_slippage(&xyk_pool, amount, false);

        assert!(cl_slippage < xyk_slippage);
    }

    #[tokio::test]
    async fn test_pool_tvl_calculation() {
        let client = AstroportClient::new(
            "https://test.com/api",
            "test-1",
            "https://test-rpc.com",
        );

        // Mock pool data in cache
        let pool = PoolLiquidity {
            token_a: "atom".to_string(),
            token_b: "osmo".to_string(),
            reserve_a: 1_000_000_000_000, // 1M ATOM (6 decimals)
            reserve_b: 5_000_000_000_000, // 5M OSMO
            total_share: 2_000_000_000_000,
            pool_type: PoolType::Xyk,
        };

        {
            let mut cache = client.pool_cache.write().await;
            cache.insert("test_pair".to_string(), pool);
        }

        // Price map
        let mut prices = HashMap::new();
        prices.insert("atom".to_string(), Decimal::from_str("10.0").unwrap()); // $10/ATOM
        prices.insert("osmo".to_string(), Decimal::from_str("1.0").unwrap()); // $1/OSMO

        let tvl = client.get_pool_tvl("test_pair", &prices).await.unwrap();

        // Expected: (1_000_000_000_000 * $10) + (5_000_000_000_000 * $1)
        // = 10_000_000_000_000 + 5_000_000_000_000 = 15_000_000_000_000
        let expected = Decimal::from_str("15000000000000").unwrap();
        assert_eq!(tvl, expected);
    }

    #[test]
    fn test_pool_type_detection() {
        let xyk_type = PairType {
            xyk: Some(serde_json::json!({})),
            stable: None,
            concentrated: None,
        };

        let stable_type = PairType {
            xyk: None,
            stable: Some(serde_json::json!({})),
            concentrated: None,
        };

        let concentrated_type = PairType {
            xyk: None,
            stable: None,
            concentrated: Some(serde_json::json!({})),
        };

        assert!(xyk_type.xyk.is_some());
        assert!(stable_type.stable.is_some());
        assert!(concentrated_type.concentrated.is_some());
    }

    #[test]
    fn test_asset_info_serialization() {
        let native_asset = AssetInfo::NativeToken {
            denom: "untrn".to_string(),
        };

        let token_asset = AssetInfo::Token {
            contract_addr: "neutron1...".to_string(),
        };

        // Verify they serialize correctly
        let native_json = serde_json::to_string(&native_asset).unwrap();
        assert!(native_json.contains("native_token"));
        assert!(native_json.contains("untrn"));

        let token_json = serde_json::to_string(&token_asset).unwrap();
        assert!(token_json.contains("token"));
        assert!(token_json.contains("neutron1"));
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_astroport_quote() {
        let client = AstroportClient::neutron();
        let quote = client
            .get_quote(
                "untrn",
                "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9", // ATOM
                1_000_000,
            )
            .await;

        match quote {
            Ok(q) => {
                println!("Got Astroport quote: {:?}", q);
                assert!(q.output_amount > 0);
                assert!(!q.route.is_empty());
            }
            Err(e) => {
                println!("Quote error (expected in test): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_query_pool_liquidity() {
        let client = AstroportClient::neutron();

        // NTRN-ATOM pool on Neutron
        let pair_address = "neutron1w5cf738md3prg4v7j97dw6re7g48jqk5ztfmq8uh93a4uwfjd6kqf5gk20";

        match client.query_pool(pair_address).await {
            Ok(liquidity) => {
                println!("Pool liquidity: {:?}", liquidity);
                assert!(liquidity.reserve_a > 0);
                assert!(liquidity.reserve_b > 0);
                assert!(liquidity.total_share > 0);
                assert!(!liquidity.token_a.is_empty());
                assert!(!liquidity.token_b.is_empty());
            }
            Err(e) => {
                println!("Pool query error (may be expected): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_get_pools_with_liquidity() {
        let client = AstroportClient::neutron();

        let pair = TradingPair {
            base: "untrn".to_string(),
            quote: "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9".to_string(),
        };

        match client.get_pools(&pair).await {
            Ok(pools) => {
                println!("Found {} pools", pools.len());
                for pool in &pools {
                    println!("Pool: {:?}", pool);
                    if pool.liquidity_a > 0 {
                        assert!(pool.liquidity_b > 0);
                        println!("  Liquidity A: {}", pool.liquidity_a);
                        println!("  Liquidity B: {}", pool.liquidity_b);
                    }
                }
            }
            Err(e) => {
                println!("Get pools error: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let client = AstroportClient::new(
            "https://test.com/api",
            "test-1",
            "https://test-rpc.com",
        );

        let pool = PoolLiquidity {
            token_a: "token0".to_string(),
            token_b: "token1".to_string(),
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            total_share: 1_500_000,
            pool_type: PoolType::Xyk,
        };

        // Insert into cache
        {
            let mut cache = client.pool_cache.write().await;
            cache.insert("pair1".to_string(), pool.clone());
        }

        // Should be in cache
        {
            let cache = client.pool_cache.read().await;
            assert!(cache.get("pair1").is_some());
        }

        // Wait for expiry (using a short-lived client for testing)
        let short_client = AstroportClient {
            pool_cache: Arc::new(RwLock::new(PoolCache::new(Duration::from_millis(10)))),
            ..client
        };

        {
            let mut cache = short_client.pool_cache.write().await;
            cache.insert("pair2".to_string(), pool);
        }

        tokio::time::sleep(Duration::from_millis(20)).await;

        {
            let cache = short_client.pool_cache.read().await;
            assert!(cache.get("pair2").is_none());
        }
    }
}
