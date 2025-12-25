//! Mock price oracle for simulated price feeds

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rand::Rng;
use tokio::sync::RwLock;
use tracing::debug;

use crate::models::{PriceFeed, WsMessage};
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

/// Run the price feed update loop
pub async fn run_price_feed(state: AppStateRef) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;
        update_prices(&state).await;
    }
}

async fn update_prices(state: &AppStateRef) {
    let mut rng = rand::thread_rng();
    let mut state = state.write().await;
    let mut updated_prices = Vec::new();

    for price_feed in state.prices.values_mut() {
        // Simulate small price movements (-0.5% to +0.5%)
        let change_percent = rng.gen_range(-0.5..0.5);
        let new_price = price_feed.price_usd * (1.0 + change_percent / 100.0);

        // Update the price feed
        price_feed.price_usd = new_price;
        price_feed.change_24h += change_percent; // Accumulate for 24h change
        price_feed.volume_24h += rng.gen_range(10000.0..100000.0);
        price_feed.confidence = rng.gen_range(0.95..0.99);
        price_feed.updated_at = Utc::now();

        updated_prices.push(price_feed.clone());
    }

    debug!("Updated {} price feeds", updated_prices.len());

    // Broadcast price update
    state.broadcast(WsMessage::PriceUpdate(updated_prices));
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
