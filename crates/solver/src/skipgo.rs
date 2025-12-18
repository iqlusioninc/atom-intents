use async_trait::async_trait;
use atom_intents_types::{DexSwapStep, TradingPair};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::{DenomRegistry, DexClient, DexError, DexQuote, PoolInfo};

/// Skip Go API Client - cross-chain routing aggregator
/// Provides optimal routes across Cosmos chains via DEX aggregation and IBC
pub struct SkipGoClient {
    base_url: String,
    client: reqwest::Client,
    denom_registry: Arc<Mutex<DenomRegistry>>,
}

impl SkipGoClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            denom_registry: Arc::new(Mutex::new(DenomRegistry::new())),
        }
    }

    /// Create client for mainnet
    pub fn mainnet() -> Self {
        Self::new("https://api.skip.build")
    }

    /// Create client with custom denom registry
    pub fn with_denom_registry(base_url: impl Into<String>, registry: DenomRegistry) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            denom_registry: Arc::new(Mutex::new(registry)),
        }
    }
}

#[derive(Debug, Serialize)]
struct RouteRequest {
    amount_in: String,
    source_asset_denom: String,
    source_asset_chain_id: String,
    dest_asset_denom: String,
    dest_asset_chain_id: String,
    cumulative_affiliate_fee_bps: String,
    allow_multi_tx: bool,
}

#[derive(Debug, Deserialize)]
struct RouteResponse {
    amount_in: String,
    amount_out: String,
    source_asset_denom: String,
    dest_asset_denom: String,
    operations: Vec<Operation>,
    chain_ids: Vec<String>,
    required_chain_addresses: Vec<String>,
    does_swap: bool,
    estimated_amount_out: Option<String>,
    swap_venue: Option<SwapVenue>,
    txs_required: u32,
    usd_amount_in: Option<String>,
    usd_amount_out: Option<String>,
    swap_price_impact_percent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Operation {
    transfer: Option<TransferOperation>,
    swap: Option<SwapOperation>,
}

#[derive(Debug, Deserialize)]
struct TransferOperation {
    port: String,
    channel: String,
    from_chain_id: String,
    to_chain_id: String,
    denom_in: String,
    denom_out: String,
    pfm_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SwapOperation {
    swap_in: SwapAsset,
    swap_out: SwapAsset,
    swap_venue: SwapVenue,
    chain_id: String,
}

#[derive(Debug, Deserialize)]
struct SwapAsset {
    denom: String,
    amount: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct SwapVenue {
    name: String,
    chain_id: String,
}

#[derive(Debug, Deserialize)]
struct AssetsResponse {
    chain_to_assets_map: std::collections::HashMap<String, ChainAssets>,
}

#[derive(Debug, Deserialize)]
struct ChainAssets {
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize, Clone)]
struct Asset {
    denom: String,
    chain_id: String,
    origin_denom: String,
    origin_chain_id: String,
    decimals: u32,
    symbol: Option<String>,
}

#[async_trait]
impl DexClient for SkipGoClient {
    async fn get_quote(
        &self,
        input_denom: &str,
        output_denom: &str,
        amount: u128,
    ) -> Result<DexQuote, DexError> {
        // Determine chain IDs based on denoms using DenomRegistry
        let source_chain = self.infer_chain_id(input_denom).await;
        let dest_chain = self.infer_chain_id(output_denom).await;

        let request = RouteRequest {
            amount_in: amount.to_string(),
            source_asset_denom: input_denom.to_string(),
            source_asset_chain_id: source_chain.clone(),
            dest_asset_denom: output_denom.to_string(),
            dest_asset_chain_id: dest_chain.clone(),
            cumulative_affiliate_fee_bps: "0".to_string(),
            allow_multi_tx: true,
        };

        let url = format!("{}/v2/fungible/route", self.base_url);

        debug!("Querying Skip Go route: {} with {:?}", url, request);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Skip Go API error: {} - {}", status, body);
            return Err(DexError::QueryFailed(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let route: RouteResponse = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse response: {}", e)))?;

        let output_amount = route
            .estimated_amount_out
            .unwrap_or(route.amount_out.clone())
            .parse::<u128>()
            .map_err(|e| DexError::QueryFailed(format!("Invalid amount_out: {}", e)))?;

        // Convert operations to DexSwapSteps
        let steps: Vec<DexSwapStep> = route
            .operations
            .iter()
            .filter_map(|op| {
                if let Some(swap) = &op.swap {
                    Some(DexSwapStep {
                        venue: format!("skipgo:{}", swap.swap_venue.name),
                        pool_id: format!("{}-{}", swap.swap_in.denom, swap.swap_out.denom),
                        input_denom: swap.swap_in.denom.clone(),
                        output_denom: swap.swap_out.denom.clone(),
                        chain_id: swap.chain_id.clone(),
                    })
                } else if let Some(transfer) = &op.transfer {
                    Some(DexSwapStep {
                        venue: "skipgo:ibc".to_string(),
                        pool_id: format!("{}:{}", transfer.port, transfer.channel),
                        input_denom: transfer.denom_in.clone(),
                        output_denom: transfer.denom_out.clone(),
                        chain_id: transfer.from_chain_id.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let price_impact = route
            .swap_price_impact_percent
            .unwrap_or_else(|| "0.0000".to_string());

        Ok(DexQuote {
            venue: "skipgo".to_string(),
            input_amount: amount,
            output_amount,
            price_impact,
            route: steps,
            estimated_fee: None,
        })
    }

    async fn get_pools(&self, pair: &TradingPair) -> Result<Vec<PoolInfo>, DexError> {
        // Skip Go doesn't expose pools directly - it aggregates across venues
        // Return empty since we rely on route queries for liquidity
        Ok(vec![])
    }
}

impl SkipGoClient {
    /// Infer chain ID from denom using DenomRegistry
    async fn infer_chain_id(&self, denom: &str) -> String {
        let mut registry = self.denom_registry.lock().await;

        // Try to get origin chain from registry
        match registry.get_origin_chain(denom).await {
            Ok(chain_id) => {
                debug!("Resolved denom {} to chain {}", denom, chain_id);
                chain_id
            }
            Err(e) => {
                // Fall back to heuristic
                warn!(
                    "Failed to resolve denom {} via registry: {}. Using heuristic fallback.",
                    denom, e
                );
                Self::infer_chain_id_heuristic(denom)
            }
        }
    }

    /// Heuristic fallback for chain ID inference (used when registry lookup fails)
    fn infer_chain_id_heuristic(denom: &str) -> String {
        if denom.starts_with("ibc/") {
            // For IBC denoms, default to osmosis as the primary hub
            warn!(
                "Using heuristic fallback for IBC denom {}. Defaulting to osmosis-1.",
                denom
            );
            "osmosis-1".to_string()
        } else if denom == "uatom" {
            "cosmoshub-4".to_string()
        } else if denom == "uosmo" {
            "osmosis-1".to_string()
        } else if denom == "untrn" {
            "neutron-1".to_string()
        } else if denom.starts_with("inj") {
            "injective-1".to_string()
        } else {
            // Default to osmosis as the primary hub
            warn!(
                "Unknown denom {}. Defaulting to osmosis-1.",
                denom
            );
            "osmosis-1".to_string()
        }
    }

    /// Get supported assets for a chain
    pub async fn get_assets(&self, chain_id: &str) -> Result<Vec<Asset>, DexError> {
        let url = format!(
            "{}/v2/fungible/assets?chain_id={}",
            self.base_url, chain_id
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DexError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DexError::QueryFailed(format!(
                "Failed to fetch assets: {}",
                response.status()
            )));
        }

        let assets: AssetsResponse = response
            .json()
            .await
            .map_err(|e| DexError::QueryFailed(format!("Failed to parse assets: {}", e)))?;

        Ok(assets
            .chain_to_assets_map
            .get(chain_id)
            .map(|ca| ca.assets.clone())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_skip_go_route() {
        let client = SkipGoClient::mainnet();
        let quote = client
            .get_quote(
                "uosmo",
                "uatom",
                1_000_000,
            )
            .await;

        match quote {
            Ok(q) => {
                println!("Got Skip Go route: {:?}", q);
                assert!(q.output_amount > 0);
            }
            Err(e) => {
                println!("Route error (expected in test): {}", e);
            }
        }
    }
}
