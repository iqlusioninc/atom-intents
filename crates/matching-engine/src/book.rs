use atom_intents_types::{FillConfig, FillSource, Fill, Intent, MatchResult, Side, TradingPair};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, VecDeque};
use std::str::FromStr;

use crate::MatchingError;

/// Central limit order book for a trading pair
pub struct OrderBook {
    pub pair: TradingPair,

    /// Buy orders (bids) - price descending (best bid first)
    bids: BTreeMap<OrderedPrice, VecDeque<BookEntry>>,

    /// Sell orders (asks) - price ascending (best ask first)
    asks: BTreeMap<OrderedPrice, VecDeque<BookEntry>>,

    /// Sequence number for time priority
    sequence: u64,
}

/// Wrapper for price that implements correct ordering
#[derive(Clone, Debug, PartialEq, Eq)]
struct OrderedPrice {
    price: Decimal,
    is_bid: bool,
}

impl PartialOrd for OrderedPrice {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedPrice {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.is_bid {
            // Bids: higher price = better = comes first
            other.price.cmp(&self.price)
        } else {
            // Asks: lower price = better = comes first
            self.price.cmp(&other.price)
        }
    }
}

/// An entry in the order book
#[derive(Clone, Debug)]
pub struct BookEntry {
    pub intent_id: String,
    pub user: String,
    pub side: Side,
    pub original_amount: Uint128,
    pub remaining_amount: Uint128,
    pub limit_price: Decimal,
    pub fill_config: FillConfig,
    pub timestamp: u64,
    pub sequence: u64,
}

impl OrderBook {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            pair,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// Process an incoming intent
    pub fn process_intent(&mut self, intent: &Intent, current_time: u64) -> Result<MatchResult, MatchingError> {
        let side = self.determine_side(intent);
        let limit_price = Decimal::from_str(&intent.output.limit_price)
            .map_err(|e| MatchingError::InvalidPrice(e.to_string()))?;

        let mut remaining = intent.input.amount;
        let mut fills = Vec::new();

        // Get opposite side of book
        let opposite = match side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };

        // Walk the book at each price level
        let mut exhausted_levels = Vec::new();

        for (price_level, entries) in opposite.iter_mut() {
            // Check if prices cross
            if !Self::prices_cross(limit_price, price_level.price, side) {
                break;
            }

            // Match against entries at this level (FIFO)
            let mut exhausted_indices = Vec::new();

            for (idx, entry) in entries.iter_mut().enumerate() {
                if remaining.is_zero() {
                    break;
                }

                let match_amount = std::cmp::min(remaining, entry.remaining_amount);

                if !match_amount.is_zero() {
                    // Check if partial fill is allowed
                    if match_amount < entry.remaining_amount && !entry.fill_config.allow_partial {
                        continue;
                    }

                    // Execute at maker's price (price improvement for taker)
                    let output_amount = Self::calculate_output(match_amount, price_level.price);

                    fills.push(Fill {
                        input_amount: match_amount,
                        output_amount,
                        price: price_level.price.to_string(),
                        source: FillSource::IntentMatch {
                            counterparty: entry.intent_id.clone(),
                        },
                    });

                    remaining = remaining.saturating_sub(match_amount);
                    entry.remaining_amount = entry.remaining_amount.saturating_sub(match_amount);

                    if entry.remaining_amount.is_zero() {
                        exhausted_indices.push(idx);
                    }
                }
            }

            // Remove exhausted entries (in reverse order to preserve indices)
            for idx in exhausted_indices.into_iter().rev() {
                entries.remove(idx);
            }

            if entries.is_empty() {
                exhausted_levels.push(price_level.clone());
            }
        }

        // Remove exhausted price levels
        for level in exhausted_levels {
            opposite.remove(&level);
        }

        // Add remainder to book if partial allowed
        if !remaining.is_zero() && intent.fill_config.allow_partial {
            self.add_to_book(intent, remaining, side, limit_price, current_time);
        }

        Ok(MatchResult { fills, remaining })
    }

    fn determine_side(&self, intent: &Intent) -> Side {
        // If input is base asset, user is selling
        // If input is quote asset, user is buying
        if intent.input.denom == self.pair.base {
            Side::Sell
        } else {
            Side::Buy
        }
    }

    fn prices_cross(taker_limit: Decimal, maker_price: Decimal, taker_side: Side) -> bool {
        match taker_side {
            Side::Buy => taker_limit >= maker_price, // Buyer willing to pay >= ask price
            Side::Sell => taker_limit <= maker_price, // Seller willing to accept <= bid price
        }
    }

    fn calculate_output(input: Uint128, price: Decimal) -> Uint128 {
        let input_dec = Decimal::from(input.u128());
        let output = input_dec * price;
        Uint128::new(output.to_string().parse::<u128>().unwrap_or(0))
    }

    fn add_to_book(
        &mut self,
        intent: &Intent,
        remaining: Uint128,
        side: Side,
        limit_price: Decimal,
        timestamp: u64,
    ) {
        self.sequence += 1;

        let entry = BookEntry {
            intent_id: intent.id.clone(),
            user: intent.user.clone(),
            side,
            original_amount: intent.input.amount,
            remaining_amount: remaining,
            limit_price,
            fill_config: intent.fill_config.clone(),
            timestamp,
            sequence: self.sequence,
        };

        let price_key = OrderedPrice {
            price: limit_price,
            is_bid: matches!(side, Side::Buy),
        };

        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        book.entry(price_key)
            .or_insert_with(VecDeque::new)
            .push_back(entry);
    }

    /// Get best bid price
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.keys().next().map(|k| k.price)
    }

    /// Get best ask price
    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.keys().next().map(|k| k.price)
    }

    /// Get mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::from(2)),
            _ => None,
        }
    }

    /// Get spread
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Total bid depth
    pub fn bid_depth(&self) -> Uint128 {
        self.bids
            .values()
            .flat_map(|entries| entries.iter())
            .map(|e| e.remaining_amount)
            .sum()
    }

    /// Total ask depth
    pub fn ask_depth(&self) -> Uint128 {
        self.asks
            .values()
            .flat_map(|entries| entries.iter())
            .map(|e| e.remaining_amount)
            .sum()
    }

    /// Cancel an order by intent ID
    pub fn cancel(&mut self, intent_id: &str) -> Option<BookEntry> {
        // Check bids
        for entries in self.bids.values_mut() {
            if let Some(idx) = entries.iter().position(|e| e.intent_id == intent_id) {
                return entries.remove(idx);
            }
        }

        // Check asks
        for entries in self.asks.values_mut() {
            if let Some(idx) = entries.iter().position(|e| e.intent_id == intent_id) {
                return entries.remove(idx);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, ExecutionConstraints, FillStrategy, OutputSpec};
    use cosmwasm_std::Binary;

    fn make_test_intent(
        id: &str,
        user: &str,
        input_denom: &str,
        input_amount: u128,
        output_denom: &str,
        min_output: u128,
        limit_price: &str,
    ) -> Intent {
        Intent {
            id: id.to_string(),
            version: "1.0".to_string(),
            nonce: 0,
            user: user.to_string(),
            input: Asset::new("cosmoshub-4", input_denom, input_amount),
            output: OutputSpec {
                chain_id: "noble-1".to_string(),
                denom: output_denom.to_string(),
                min_amount: Uint128::new(min_output),
                limit_price: limit_price.to_string(),
                recipient: user.to_string(),
            },
            fill_config: FillConfig {
                allow_partial: true,
                min_fill_amount: Uint128::zero(),
                min_fill_pct: "0.1".to_string(),
                aggregation_window_ms: 5000,
                strategy: FillStrategy::Eager,
            },
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::default(),
            public_key: Binary::default(),
            created_at: 0,
            expires_at: 9999999999,
        }
    }

    #[test]
    fn test_order_book_basic_match() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add a sell order (selling ATOM for USDC at 10.0)
        let sell_intent = make_test_intent(
            "sell-1",
            "seller",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "10.0",
        );
        let _ = book.process_intent(&sell_intent, 0);

        // Add a buy order that crosses (buying ATOM with USDC at 10.5)
        let buy_intent = make_test_intent(
            "buy-1",
            "buyer",
            "uusdc",
            10_500_000,
            "uatom",
            1_000_000,
            "10.5",
        );
        let result = book.process_intent(&buy_intent, 1).unwrap();

        // Should have matched
        assert!(!result.fills.is_empty());
    }
}
