//! Price oracle with real CoinGecko price feeds

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{debug, warn, info};

use crate::models::{PriceFeed, WsMessage};
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

/// CoinGecko API response for price data
#[derive(Debug, Deserialize)]
struct CoinGeckoPrice {
    usd: f64,
    #[serde(default)]
    usd_24h_change: Option<f64>,
    #[serde(default)]
    usd_24h_vol: Option<f64>,
}

/// Mapping from our token symbols to CoinGecko IDs
fn get_coingecko_id(symbol: &str) -> Option<&'static str> {
    match symbol.to_uppercase().as_str() {
        "ATOM" => Some("cosmos"),
        "OSMO" => Some("osmosis"),
        "USDC" => Some("usd-coin"),
        "NTRN" => Some("neutron-3"),
        "STRD" => Some("stride"),
        "TIA" => Some("celestia"),
        // LST tokens - try to get from CoinGecko but fallback to base token
        "STATOM" => Some("stride-staked-atom"),
        "STOSMO" => Some("stride-staked-osmo"),
        "STTIA" => Some("stride-staked-tia"),
        "STKATOM" => Some("pstake-staked-atom"),
        "QATOM" => Some("quicksilver-staked-atom"),
        _ => None,
    }
}

/// Get the base token and premium multiplier for LST tokens
fn get_lst_base_token(symbol: &str) -> Option<(&'static str, f64)> {
    match symbol.to_uppercase().as_str() {
        "STATOM" => Some(("cosmos", 1.05)),      // ~5% premium for staking yield
        "STOSMO" => Some(("osmosis", 1.05)),
        "STTIA" => Some(("celestia", 1.05)),
        "STKATOM" => Some(("cosmos", 1.03)),     // ~3% premium
        "QATOM" => Some(("cosmos", 1.03)),
        _ => None,
    }
}

/// Fetch prices from CoinGecko API
async fn fetch_coingecko_prices(client: &Client) -> Result<HashMap<String, CoinGeckoPrice>, reqwest::Error> {
    let url = "https://api.coingecko.com/api/v3/simple/price?ids=cosmos,osmosis,usd-coin,neutron-3,stride,celestia&vs_currencies=usd&include_24hr_change=true&include_24hr_vol=true";

    let response = client
        .get(url)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json::<HashMap<String, CoinGeckoPrice>>()
        .await?;

    Ok(response)
}

/// Run the price feed update loop
pub async fn run_price_feed(state: AppStateRef) {
    // Update every 30 seconds to respect CoinGecko rate limits (free tier)
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    let client = Client::new();

    info!("Starting real-time price feed from CoinGecko");

    loop {
        interval.tick().await;
        update_prices(&state, &client).await;
    }
}

async fn update_prices(state: &AppStateRef, client: &Client) {
    // Try to fetch real prices from CoinGecko
    match fetch_coingecko_prices(client).await {
        Ok(cg_prices) => {
            let mut state = state.write().await;
            let mut updated_prices = Vec::new();

            for price_feed in state.prices.values_mut() {
                // Map our symbol to CoinGecko ID
                if let Some(cg_id) = get_coingecko_id(&price_feed.denom) {
                    if let Some(cg_price) = cg_prices.get(cg_id) {
                        // Update with real price data
                        price_feed.price_usd = cg_price.usd;
                        price_feed.change_24h = cg_price.usd_24h_change.unwrap_or(0.0);
                        price_feed.volume_24h = cg_price.usd_24h_vol.unwrap_or(0.0);
                        price_feed.confidence = 0.99; // High confidence for real data
                        price_feed.updated_at = Utc::now();

                        updated_prices.push(price_feed.clone());
                    } else {
                        // For LST tokens, derive price from base token with premium
                        if let Some((base_id, premium)) = get_lst_base_token(&price_feed.denom) {
                            if let Some(base_price) = cg_prices.get(base_id) {
                                price_feed.price_usd = base_price.usd * premium;
                                price_feed.change_24h = base_price.usd_24h_change.unwrap_or(0.0);
                                price_feed.volume_24h = 0.0; // LST volume not tracked
                                price_feed.confidence = 0.95; // Slightly lower confidence for derived price
                                price_feed.updated_at = Utc::now();

                                updated_prices.push(price_feed.clone());
                            }
                        }
                    }
                }
            }

            info!("Updated {} prices from CoinGecko: ATOM=${:.4}, OSMO=${:.4}",
                updated_prices.len(),
                cg_prices.get("cosmos").map(|p| p.usd).unwrap_or(0.0),
                cg_prices.get("osmosis").map(|p| p.usd).unwrap_or(0.0)
            );

            // Broadcast price update
            state.broadcast(WsMessage::PriceUpdate(updated_prices));
        }
        Err(e) => {
            warn!("Failed to fetch prices from CoinGecko: {}. Prices unchanged.", e);
        }
    }
}

/// Get the exchange rate between two denominations
pub fn get_exchange_rate(
    prices: &std::collections::HashMap<String, PriceFeed>,
    from_denom: &str,
    to_denom: &str,
) -> Option<f64> {
    let from_price = prices.get(from_denom)?.price_usd;
    let to_price = prices.get(to_denom)?.price_usd;

    if to_price == 0.0 {
        return None;
    }

    Some(from_price / to_price)
}

/// Calculate output amount for a swap
pub fn calculate_output_amount(
    prices: &std::collections::HashMap<String, PriceFeed>,
    input_denom: &str,
    output_denom: &str,
    input_amount: u128,
    slippage_bps: u16,
) -> Option<u128> {
    let rate = get_exchange_rate(prices, input_denom, output_denom)?;

    // Apply slippage
    let slippage_multiplier = 1.0 - (slippage_bps as f64 / 10000.0);
    let output = (input_amount as f64) * rate * slippage_multiplier;

    Some(output as u128)
}
