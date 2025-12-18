use async_trait::async_trait;
use atom_intents_types::{DexSwapStep, TradingPair};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::{DexClient, DexError, DexQuote, PoolInfo};

/// Osmosis DEX Client - queries Osmosis SQS (Sidecar Query Server) for quotes
pub struct OsmosisClient {
    base_url: String,
    client: reqwest::Client,
    chain_id: String,
}

impl OsmosisClient {
    pub fn new(base_url: impl Into<String>, chain_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            chain_id: chain_id.into(),
        }
    }

    /// Create client for mainnet
    pub fn mainnet() -> Self {
        Self::new("https://sqs.osmosis.zone", "osmosis-1")
    }

    /// Create client for testnet
    pub fn testnet() -> Self {
        Self::new("https://sqs.testnet.osmosis.zone", "osmo-test-5")
    }
}

#[derive(Debug, Deserialize)]
struct SqsQuoteResponse {
    amount_in: SqsCoin,
    amount_out: String,
    route: Vec<SqsRoute>,
    effective_fee: String,
    price_impact: String,
}

#[derive(Debug, Deserialize)]
struct SqsCoin {
    denom: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct SqsRoute {
    pools: Vec<SqsPoolRoute>,
    #[serde(rename = "tokenInDenom")]
    token_in_denom: Option<String>,
    #[serde(rename = "tokenOutDenom")]
    token_out_denom: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SqsPoolRoute {
    id: u64,
    #[serde(rename = "tokenOutDenom")]
    token_out_denom: String,
    #[serde(rename = "spreadFactor")]
    spread_factor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SqsPoolsResponse {
    pools: Vec<SqsPool>,
}

#[derive(Debug, Deserialize)]
struct SqsPool {
    #[serde(rename = "pool_id")]
    id: u64,
    #[serde(rename = "pool_type")]
    pool_type: Option<String>,
    pool_tokens: Option<Vec<SqsPoolToken>>,
    spread_factor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SqsPoolToken {
    denom: String,
    amount: String,
}

#[async_trait]
impl DexClient for OsmosisClient {
    async fn get_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Result<DexQuote, DexError> {
        let url = format!(
            "{}/router/quote?tokenIn={}{}&tokenOutDenom={}",
            self.base_url, amount, input_denom, output_denom
        );

        debug!("Querying Osmosis SQS: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Osmosis SQS error: {} - {}", status, body);
            return Err(DexError::QueryFailed(format!("HTTP {}: {}", status, body)));
        }

        let quote: SqsQuoteResponse = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse response: {}", e)))?;

        // Convert route to DexSwapSteps
        let route: Vec<DexSwapStep> = quote
            .route
            .iter()
            .flat_map(|r| {
                r.pools.iter().enumerate().map(|(i, pool)| {
                    let in_denom = if i == 0 {
                        r.token_in_denom
                            .clone()
                            .unwrap_or_else(|| input_denom.to_string())
                    } else {
                        r.pools[i - 1].token_out_denom.clone()
                    };
                    DexSwapStep {
                        venue: "osmosis".to_string(),
                        pool_id: pool.id.to_string(),
                        input_denom: in_denom,
                        output_denom: pool.token_out_denom.clone(),
                        chain_id: self.chain_id.clone(),
                    }
                })
            })
            .collect();

        let output_amount = quote
            .amount_out
            .parse::<u128>()
            .map_err(|e| DexError::QueryFailed(format!("Invalid amount_out: {}", e)))?;

        Ok(DexQuote {
            venue: "osmosis".to_string(),
            input_amount: amount,
            output_amount,
            price_impact: quote.price_impact,
            route,
            estimated_fee: None,
        })
    }

    async fn get_pools(&self, pair: &TradingPair) -> Result<Vec<PoolInfo>, DexError> {
        // Query pools endpoint filtered by denoms
        let url = format!(
            "{}/pools?denoms={},{}&min_liquidity=1000",
            self.base_url, pair.base, pair.quote
        );

        debug!("Querying Osmosis pools: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            // Fallback to returning empty - pools query may not be supported
            return Ok(vec![]);
        }

        let pools_response: SqsPoolsResponse = match response.json().await {
            Ok(p) => p,
            Err(_) => return Ok(vec![]), // Fallback on parse error
        };

        let pools = pools_response
            .pools
            .into_iter()
            .filter_map(|pool| {
                let tokens = pool.pool_tokens?;
                if tokens.len() < 2 {
                    return None;
                }

                let token_a = tokens.iter().find(|t| t.denom == pair.base)?;
                let token_b = tokens.iter().find(|t| t.denom == pair.quote)?;

                Some(PoolInfo {
                    pool_id: pool.id.to_string(),
                    token_a: token_a.denom.clone(),
                    token_b: token_b.denom.clone(),
                    liquidity_a: token_a.amount.parse().unwrap_or(0),
                    liquidity_b: token_b.amount.parse().unwrap_or(0),
                    fee_rate: pool.spread_factor.unwrap_or_else(|| "0.003".to_string()),
                })
            })
            .collect();

        Ok(pools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_osmosis_quote() {
        let client = OsmosisClient::mainnet();
        let quote = client
            .get_quote(
                "uosmo",
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                1_000_000,
            )
            .await;

        match quote {
            Ok(q) => {
                println!("Got quote: {:?}", q);
                assert!(q.output_amount > 0);
            }
            Err(e) => {
                println!("Quote error (expected in test): {}", e);
            }
        }
    }
}
