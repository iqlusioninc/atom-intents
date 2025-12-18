use atom_intents_types::{Fill, FillConfig, FillSource, Intent, MatchResult, Side, TradingPair};
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
    pub fn process_intent(
        &mut self,
        intent: &Intent,
        current_time: u64,
    ) -> Result<MatchResult, MatchingError> {
        let side = self.determine_side(intent);
        let raw_price = Decimal::from_str(&intent.output.limit_price)
            .map_err(|e| MatchingError::InvalidPrice(e.to_string()))?;

        // For uniform comparison, convert buy prices to quote/base (USDC/ATOM) format
        // Buy limit_price is in output/input (ATOM/USDC), so invert it
        let limit_price = if matches!(side, Side::Buy) && raw_price > Decimal::ZERO {
            Decimal::ONE / raw_price
        } else {
            raw_price
        };

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

                // Convert amounts to base units (ATOM) for proper comparison
                // Buy taker (input=USDC) vs Sell maker (amount in ATOM): convert USDC to ATOM
                // Sell taker (input=ATOM) vs Buy maker (amount in USDC): convert USDC to ATOM
                let (taker_base, maker_base) = match side {
                    Side::Buy => {
                        // Taker input is USDC, maker amount is ATOM
                        let taker = Self::quote_to_base(remaining, price_level.price);
                        (taker, entry.remaining_amount)
                    }
                    Side::Sell => {
                        // Taker input is ATOM, maker amount is USDC
                        let maker = Self::quote_to_base(entry.remaining_amount, price_level.price);
                        (remaining, maker)
                    }
                };

                let match_amount_base = std::cmp::min(taker_base, maker_base);

                if !match_amount_base.is_zero() {
                    // Check if partial fill is allowed (compare in maker's original units)
                    let maker_fully_filled = match side {
                        Side::Buy => match_amount_base >= entry.remaining_amount,
                        Side::Sell => match_amount_base >= maker_base,
                    };
                    if !maker_fully_filled && !entry.fill_config.allow_partial {
                        continue;
                    }

                    // Calculate amounts in original units
                    let (taker_consumed, maker_consumed, fill_input, fill_output) = match side {
                        Side::Buy => {
                            // Taker gives USDC, gets ATOM
                            let taker_usdc =
                                Self::base_to_quote(match_amount_base, price_level.price);
                            let maker_atom = match_amount_base;
                            (taker_usdc, maker_atom, taker_usdc, match_amount_base)
                        }
                        Side::Sell => {
                            // Taker gives ATOM, gets USDC
                            let taker_atom = match_amount_base;
                            let maker_usdc =
                                Self::base_to_quote(match_amount_base, price_level.price);
                            (taker_atom, maker_usdc, match_amount_base, maker_usdc)
                        }
                    };

                    fills.push(Fill {
                        input_amount: fill_input,
                        output_amount: fill_output,
                        price: price_level.price.to_string(),
                        source: FillSource::IntentMatch {
                            counterparty: entry.intent_id.clone(),
                        },
                    });

                    remaining = remaining.saturating_sub(taker_consumed);
                    entry.remaining_amount = entry.remaining_amount.saturating_sub(maker_consumed);

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

        // Add remainder to book (AON orders are added too - they just can't be partially filled)
        if !remaining.is_zero() {
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

    /// Convert quote amount (USDC) to base amount (ATOM) at given price
    fn quote_to_base(quote: Uint128, price: Decimal) -> Uint128 {
        if price.is_zero() {
            return Uint128::zero();
        }
        let quote_dec = Decimal::from(quote.u128());
        let base = quote_dec / price;
        Uint128::new(base.trunc().to_string().parse::<u128>().unwrap_or(0))
    }

    /// Convert base amount (ATOM) to quote amount (USDC) at given price
    fn base_to_quote(base: Uint128, price: Decimal) -> Uint128 {
        let base_dec = Decimal::from(base.u128());
        let quote = base_dec * price;
        Uint128::new(quote.trunc().to_string().parse::<u128>().unwrap_or(0))
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
        let mut bid_key_to_remove = None;
        for (key, entries) in self.bids.iter_mut() {
            if let Some(idx) = entries.iter().position(|e| e.intent_id == intent_id) {
                let entry = entries.remove(idx);
                if entries.is_empty() {
                    bid_key_to_remove = Some(key.clone());
                }
                if let Some(key) = bid_key_to_remove {
                    self.bids.remove(&key);
                }
                return entry;
            }
        }

        // Check asks
        let mut ask_key_to_remove = None;
        for (key, entries) in self.asks.iter_mut() {
            if let Some(idx) = entries.iter().position(|e| e.intent_id == intent_id) {
                let entry = entries.remove(idx);
                if entries.is_empty() {
                    ask_key_to_remove = Some(key.clone());
                }
                if let Some(key) = ask_key_to_remove {
                    self.asks.remove(&key);
                }
                return entry;
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
        make_test_intent_with_partial(
            id,
            user,
            input_denom,
            input_amount,
            output_denom,
            min_output,
            limit_price,
            true,
        )
    }

    fn make_test_intent_with_partial(
        id: &str,
        user: &str,
        input_denom: &str,
        input_amount: u128,
        output_denom: &str,
        min_output: u128,
        limit_price: &str,
        allow_partial: bool,
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
                allow_partial,
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

    // ==================== Basic Operations ====================

    #[test]
    fn test_new_order_book_is_empty() {
        let pair = TradingPair::new("uatom", "uusdc");
        let book = OrderBook::new(pair.clone());

        assert_eq!(book.pair, pair);
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
        assert!(book.mid_price().is_none());
        assert!(book.spread().is_none());
        assert_eq!(book.bid_depth(), Uint128::zero());
        assert_eq!(book.ask_depth(), Uint128::zero());
    }

    #[test]
    fn test_add_sell_order_to_empty_book() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Sell 1000 ATOM at price 10.0 (expect 10000 USDC)
        let sell_intent = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );
        let result = book.process_intent(&sell_intent, 0).unwrap();

        // No fills (no bids to match against)
        assert!(result.fills.is_empty());
        // Remaining amount added to book
        assert_eq!(result.remaining, Uint128::new(1_000_000));
        // Book should have the ask
        assert_eq!(book.best_ask(), Some(Decimal::from_str("10.0").unwrap()));
        assert!(book.best_bid().is_none());
        assert_eq!(book.ask_depth(), Uint128::new(1_000_000));
    }

    #[test]
    fn test_add_buy_order_to_empty_book() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Buy ATOM with 10000 USDC, willing to pay up to 10 USDC/ATOM
        // limit_price "0.1" ATOM/USDC inverts to 10.0 USDC/ATOM
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000,
            "0.1", // 0.1 ATOM/USDC = willing to pay 10 USDC/ATOM
        );
        let result = book.process_intent(&buy_intent, 0).unwrap();

        // No fills (no asks to match against)
        assert!(result.fills.is_empty());
        assert_eq!(result.remaining, Uint128::new(10_000_000));
        // Book should have the bid - price stored as inverted (USDC/ATOM)
        assert_eq!(book.best_bid(), Some(Decimal::from_str("10.0").unwrap()));
        assert!(book.best_ask().is_none());
        assert_eq!(book.bid_depth(), Uint128::new(10_000_000));
    }

    // ==================== Matching Tests ====================

    #[test]
    fn test_basic_match_sell_then_buy() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add a sell order at 10.0 USDC/ATOM
        let sell_intent = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000,
            "10.0", // Want at least 10.0 USDC per ATOM
        );
        book.process_intent(&sell_intent, 0).unwrap();

        // Add a buy order willing to pay up to ~10.5 USDC/ATOM
        // limit_price for buy is ATOM/USDC, so 1/10.5 ≈ 0.095
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 10_500_000, "uatom", 1_000_000,
            "0.095", // Expect at least 0.095 ATOM per USDC = willing to pay ~10.5 USDC/ATOM
        );
        let result = book.process_intent(&buy_intent, 1).unwrap();

        // Should have one fill
        assert_eq!(result.fills.len(), 1);
        let fill = &result.fills[0];
        // Buyer spent 10M USDC to get 1M ATOM at price 10.0
        assert_eq!(fill.input_amount, Uint128::new(10_000_000)); // USDC spent
        assert_eq!(fill.output_amount, Uint128::new(1_000_000)); // ATOM received
                                                                 // Executed at maker's price (10.0)
        assert_eq!(fill.price, "10.0");

        // Verify counterparty
        if let FillSource::IntentMatch { counterparty } = &fill.source {
            assert_eq!(counterparty, "sell-1");
        } else {
            panic!("Expected IntentMatch source");
        }

        // Sell order fully consumed, buy order partially filled (500K USDC remaining)
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn test_no_match_when_prices_dont_cross() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add a sell at 11.0 USDC/ATOM
        let sell_intent = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 11_000_000, "11.0",
        );
        book.process_intent(&sell_intent, 0).unwrap();

        // Add a buy willing to pay only 10.0 USDC/ATOM (1/10.0 = 0.1 ATOM/USDC)
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000,
            "0.1", // 0.1 ATOM/USDC = 10 USDC/ATOM max
        );
        let result = book.process_intent(&buy_intent, 1).unwrap();

        // No match (buyer max 10 < seller min 11)
        assert!(result.fills.is_empty());
        // Both orders remain in book
        assert_eq!(book.best_ask(), Some(Decimal::from_str("11.0").unwrap()));
        // Buy price is stored inverted: 1/0.1 = 10.0
        assert_eq!(book.best_bid(), Some(Decimal::from_str("10.0").unwrap()));
    }

    #[test]
    fn test_partial_fill() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add a large sell order at 10.0 USDC/ATOM
        let sell_intent = make_test_intent(
            "sell-1",
            "seller",
            "uatom",
            10_000_000, // 10 ATOM
            "uusdc",
            100_000_000,
            "10.0",
        );
        book.process_intent(&sell_intent, 0).unwrap();

        // Add a smaller buy order at 10.0 USDC/ATOM (0.1 ATOM/USDC)
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 30_000_000, // 30 USDC = 3 ATOM worth at 10.0
            "uatom", 3_000_000, "0.1", // 0.1 ATOM/USDC = 10 USDC/ATOM
        );
        let result = book.process_intent(&buy_intent, 1).unwrap();

        // Buyer spends 30M USDC to get 3M ATOM at price 10.0
        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].input_amount, Uint128::new(30_000_000)); // USDC spent
        assert_eq!(result.fills[0].output_amount, Uint128::new(3_000_000)); // ATOM received

        // Sell order partially remains (10 - 3 = 7)
        assert_eq!(book.ask_depth(), Uint128::new(7_000_000));
    }

    // ==================== Price Priority Tests ====================

    #[test]
    fn test_price_priority_best_price_first() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add sells at different prices (worst to best)
        let sell_high = make_test_intent(
            "sell-high",
            "seller1",
            "uatom",
            1_000_000,
            "uusdc",
            11_000_000,
            "11.0",
        );
        let sell_low = make_test_intent(
            "sell-low", "seller2", "uatom", 1_000_000, "uusdc", 9_000_000, "9.0",
        );
        let sell_mid = make_test_intent(
            "sell-mid", "seller3", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );

        book.process_intent(&sell_high, 0).unwrap();
        book.process_intent(&sell_low, 1).unwrap();
        book.process_intent(&sell_mid, 2).unwrap();

        // Best ask should be the lowest price
        assert_eq!(book.best_ask(), Some(Decimal::from_str("9.0").unwrap()));

        // Buy order that can fill 2 orders, willing to pay up to 10.5 (0.095 ATOM/USDC)
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 20_000_000, // Enough for 2 ATOM at ~10.0
            "uatom", 2_000_000, "0.095", // ~10.5 USDC/ATOM max
        );
        let result = book.process_intent(&buy_intent, 3).unwrap();

        // Should match best prices first: 9.0 then 10.0
        assert_eq!(result.fills.len(), 2);
        assert_eq!(result.fills[0].price, "9.0");
        assert_eq!(result.fills[1].price, "10.0");

        // Only the 11.0 order remains
        assert_eq!(book.best_ask(), Some(Decimal::from_str("11.0").unwrap()));
    }

    // ==================== Time Priority Tests ====================

    #[test]
    fn test_time_priority_fifo_at_same_price() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add multiple sells at the same price
        let sell1 = make_test_intent(
            "sell-first",
            "seller1",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "10.0",
        );
        let sell2 = make_test_intent(
            "sell-second",
            "seller2",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "10.0",
        );
        let sell3 = make_test_intent(
            "sell-third",
            "seller3",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "10.0",
        );

        book.process_intent(&sell1, 0).unwrap();
        book.process_intent(&sell2, 1).unwrap();
        book.process_intent(&sell3, 2).unwrap();

        // Buy enough for 2 orders at 10 USDC/ATOM (0.1 ATOM/USDC)
        let buy_intent = make_test_intent(
            "buy-1", "buyer", "uusdc", 20_000_000, "uatom", 2_000_000, "0.1", // 10 USDC/ATOM
        );
        let result = book.process_intent(&buy_intent, 3).unwrap();

        // Should match in FIFO order
        assert_eq!(result.fills.len(), 2);

        if let FillSource::IntentMatch { counterparty } = &result.fills[0].source {
            assert_eq!(counterparty, "sell-first");
        }
        if let FillSource::IntentMatch { counterparty } = &result.fills[1].source {
            assert_eq!(counterparty, "sell-second");
        }

        // Third order should remain
        assert_eq!(book.ask_depth(), Uint128::new(1_000_000));
    }

    // ==================== All-or-Nothing Tests ====================

    #[test]
    fn test_all_or_nothing_skipped_on_partial() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add an all-or-nothing sell order
        let sell_aon = make_test_intent_with_partial(
            "sell-aon",
            "seller",
            "uatom",
            10_000_000, // 10 ATOM
            "uusdc",
            100_000_000,
            "10.0",
            false, // all-or-nothing
        );
        book.process_intent(&sell_aon, 0).unwrap();

        // Add a small buy that can't fully fill the AON order (0.1 = 10 USDC/ATOM)
        let buy_small = make_test_intent(
            "buy-small",
            "buyer",
            "uusdc",
            50_000_000, // Only 5 ATOM worth
            "uatom",
            5_000_000,
            "0.1",
        );
        let result = book.process_intent(&buy_small, 1).unwrap();

        // Should not match (AON requires full fill)
        assert!(result.fills.is_empty());

        // Both orders remain in book
        assert_eq!(book.ask_depth(), Uint128::new(10_000_000));
        assert_eq!(book.bid_depth(), Uint128::new(50_000_000));
    }

    #[test]
    fn test_all_or_nothing_matches_when_fully_filled() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add an all-or-nothing sell order
        let sell_aon = make_test_intent_with_partial(
            "sell-aon", "seller", "uatom", 5_000_000, // 5 ATOM
            "uusdc", 50_000_000, "10.0", false, // all-or-nothing
        );
        book.process_intent(&sell_aon, 0).unwrap();

        // Add a buy that can fully fill the AON order (0.1 = 10 USDC/ATOM)
        let buy_large = make_test_intent(
            "buy-large",
            "buyer",
            "uusdc",
            100_000_000, // 10 ATOM worth
            "uatom",
            10_000_000,
            "0.1",
        );
        let result = book.process_intent(&buy_large, 1).unwrap();

        // Should match - buyer spent 50M USDC to get 5M ATOM
        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].input_amount, Uint128::new(50_000_000)); // USDC spent
        assert_eq!(result.fills[0].output_amount, Uint128::new(5_000_000)); // ATOM received

        // AON sell consumed
        assert_eq!(book.ask_depth(), Uint128::zero());
    }

    // ==================== Book State Tests ====================

    #[test]
    fn test_spread_calculation() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Sell at 10.5 USDC/ATOM
        let sell = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_500_000, "10.5",
        );
        // Buy at 10.0 USDC/ATOM (0.1 ATOM/USDC)
        let buy = make_test_intent(
            "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
        );

        book.process_intent(&sell, 0).unwrap();
        book.process_intent(&buy, 1).unwrap();

        assert_eq!(book.best_ask(), Some(Decimal::from_str("10.5").unwrap()));
        assert_eq!(book.best_bid(), Some(Decimal::from_str("10.0").unwrap()));
        assert_eq!(book.spread(), Some(Decimal::from_str("0.5").unwrap()));
        assert_eq!(book.mid_price(), Some(Decimal::from_str("10.25").unwrap()));
    }

    #[test]
    fn test_depth_calculation() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add multiple sells at different prices
        let sell1 = make_test_intent(
            "sell-1", "seller1", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );
        let sell2 = make_test_intent(
            "sell-2", "seller2", "uatom", 2_000_000, "uusdc", 22_000_000, "11.0",
        );

        book.process_intent(&sell1, 0).unwrap();
        book.process_intent(&sell2, 1).unwrap();

        assert_eq!(book.ask_depth(), Uint128::new(3_000_000));

        // Add buys at prices below sells (so no match)
        // Buy at 9 USDC/ATOM (1/9 ≈ 0.111 ATOM/USDC)
        let buy1 = make_test_intent(
            "buy-1", "buyer1", "uusdc", 9_000_000, "uatom", 1_000_000, "0.111",
        );
        // Buy at 8 USDC/ATOM (0.125 ATOM/USDC)
        let buy2 = make_test_intent(
            "buy-2", "buyer2", "uusdc", 8_000_000, "uatom", 1_000_000, "0.125",
        );

        book.process_intent(&buy1, 2).unwrap();
        book.process_intent(&buy2, 3).unwrap();

        assert_eq!(book.bid_depth(), Uint128::new(17_000_000));
    }

    // ==================== Cancel Tests ====================

    #[test]
    fn test_cancel_existing_order() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        let sell = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );
        book.process_intent(&sell, 0).unwrap();

        assert_eq!(book.ask_depth(), Uint128::new(1_000_000));

        // Cancel the order
        let cancelled = book.cancel("sell-1");
        assert!(cancelled.is_some());
        assert_eq!(cancelled.unwrap().intent_id, "sell-1");

        // Book should be empty
        assert_eq!(book.ask_depth(), Uint128::zero());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn test_cancel_nonexistent_order() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        let cancelled = book.cancel("nonexistent");
        assert!(cancelled.is_none());
    }

    // ==================== Multiple Price Levels ====================

    #[test]
    fn test_walking_multiple_price_levels() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        // Add sells at three price levels
        let sell1 = make_test_intent(
            "sell-1", "s1", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );
        let sell2 = make_test_intent(
            "sell-2", "s2", "uatom", 1_000_000, "uusdc", 10_500_000, "10.5",
        );
        let sell3 = make_test_intent(
            "sell-3", "s3", "uatom", 1_000_000, "uusdc", 11_000_000, "11.0",
        );

        book.process_intent(&sell1, 0).unwrap();
        book.process_intent(&sell2, 1).unwrap();
        book.process_intent(&sell3, 2).unwrap();

        // Large buy that should sweep all three levels, willing to pay up to 11.0 (1/11 ≈ 0.0909)
        let buy_large = make_test_intent(
            "buy-large",
            "buyer",
            "uusdc",
            35_000_000, // Enough for 3+ ATOM at these prices
            "uatom",
            3_000_000,
            "0.0909", // Willing to pay up to ~11.0 USDC/ATOM (1/0.0909 ≈ 11.0)
        );
        let result = book.process_intent(&buy_large, 3).unwrap();

        // Should get fills at all three price levels
        assert_eq!(result.fills.len(), 3);

        let prices: Vec<&str> = result.fills.iter().map(|f| f.price.as_str()).collect();
        assert!(prices.contains(&"10.0"));
        assert!(prices.contains(&"10.5"));
        assert!(prices.contains(&"11.0"));

        // All asks consumed
        assert_eq!(book.ask_depth(), Uint128::zero());
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_exact_match_amount() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        let sell = make_test_intent(
            "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
        );
        book.process_intent(&sell, 0).unwrap();

        // Buy at exactly 10.0 USDC/ATOM (0.1 ATOM/USDC)
        let buy = make_test_intent(
            "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
        );
        let result = book.process_intent(&buy, 1).unwrap();

        // Buyer spends 10M USDC to get 1M ATOM
        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].input_amount, Uint128::new(10_000_000)); // USDC spent
        assert_eq!(result.fills[0].output_amount, Uint128::new(1_000_000)); // ATOM received

        // Book should be empty (sell consumed)
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn test_invalid_price_returns_error() {
        let pair = TradingPair::new("uatom", "uusdc");
        let mut book = OrderBook::new(pair);

        let invalid_intent = make_test_intent(
            "invalid",
            "user",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "not_a_number",
        );

        let result = book.process_intent(&invalid_intent, 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchingError::InvalidPrice(_)
        ));
    }
}
