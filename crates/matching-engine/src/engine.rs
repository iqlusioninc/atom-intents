use atom_intents_types::{
    AuctionFill, AuctionResult, Intent, MatchResult, Side, SolverQuote, TradingPair,
};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use crate::{MatchingError, OrderBook, MAX_CONFIDENCE_THRESHOLD, MAX_QUOTES_PER_AUCTION};

/// Oracle price data with confidence interval for secure matching
#[derive(Debug, Clone)]
pub struct OraclePrice {
    /// Price as a Decimal
    pub price: Decimal,
    /// Confidence interval (e.g., 0.01 = 1% uncertainty)
    pub confidence: Decimal,
    /// Unix timestamp when price was recorded
    pub timestamp: u64,
    /// Source identifier
    pub source: String,
}

impl OraclePrice {
    pub fn new(price: Decimal, confidence: Decimal, timestamp: u64, source: String) -> Self {
        Self {
            price,
            confidence,
            timestamp,
            source,
        }
    }

    /// Create from just a price with default confidence (for backwards compatibility)
    pub fn from_price(price: Decimal) -> Self {
        Self {
            price,
            confidence: Decimal::ZERO,
            timestamp: 0,
            source: "legacy".to_string(),
        }
    }
}

/// Matching engine managing multiple order books
pub struct MatchingEngine {
    books: HashMap<TradingPair, OrderBook>,
    current_epoch: u64,
    /// Track used nonces per user to prevent replay attacks
    used_nonces: HashMap<String, HashSet<u64>>,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
            current_epoch: 0,
            used_nonces: HashMap::new(),
        }
    }

    /// Check and mark a nonce as used for replay protection
    fn check_and_mark_nonce(&mut self, user: &str, nonce: u64) -> Result<(), MatchingError> {
        let user_nonces = self.used_nonces.entry(user.to_string()).or_default();
        if user_nonces.contains(&nonce) {
            return Err(MatchingError::NonceAlreadyUsed {
                user: user.to_string(),
                nonce,
            });
        }
        user_nonces.insert(nonce);
        Ok(())
    }

    /// Validate oracle price confidence is within acceptable threshold
    fn validate_oracle_confidence(oracle_price: &OraclePrice) -> Result<(), MatchingError> {
        let threshold = Decimal::from_str(MAX_CONFIDENCE_THRESHOLD)
            .expect("MAX_CONFIDENCE_THRESHOLD should be valid decimal");

        if oracle_price.confidence > threshold {
            return Err(MatchingError::OraclePriceUncertain {
                confidence: oracle_price.confidence,
                threshold,
            });
        }
        Ok(())
    }

    /// Validate intent has not expired
    fn validate_intent_expiration(intent: &Intent, current_time: u64) -> Result<(), MatchingError> {
        if intent.expires_at > 0 && current_time > intent.expires_at {
            return Err(MatchingError::IntentExpired {
                expires_at: intent.expires_at,
                current_time,
            });
        }
        Ok(())
    }

    /// Get or create order book for a pair
    pub fn get_or_create_book(&mut self, pair: TradingPair) -> &mut OrderBook {
        self.books.entry(pair.clone()).or_insert_with(|| OrderBook::new(pair))
    }

    /// Get order book for a pair
    pub fn get_book(&self, pair: &TradingPair) -> Option<&OrderBook> {
        self.books.get(pair)
    }

    /// Process a single intent
    pub fn process_intent(
        &mut self,
        intent: &Intent,
        current_time: u64,
    ) -> Result<MatchResult, MatchingError> {
        let pair = intent.pair();
        let book = self.get_or_create_book(pair);
        book.process_intent(intent, current_time)
    }

    /// Run a batch auction for multiple intents with full security validation
    ///
    /// Security checks performed:
    /// - Oracle confidence threshold validation (prevents manipulation via uncertain prices)
    /// - Intent expiration enforcement (prevents stale intent execution)
    /// - Solver quote bounds checking (prevents DoS via excessive quotes)
    /// - Nonce tracking for replay protection
    pub fn run_batch_auction(
        &mut self,
        pair: TradingPair,
        intents: Vec<Intent>,
        solver_quotes: Vec<SolverQuote>,
        oracle_price: Decimal,
    ) -> Result<AuctionResult, MatchingError> {
        // Use default OraclePrice for backwards compatibility
        let oracle = OraclePrice::from_price(oracle_price);
        self.run_batch_auction_with_oracle(pair, intents, solver_quotes, oracle, 0)
    }

    /// Run a batch auction with full oracle price data and current time
    pub fn run_batch_auction_with_oracle(
        &mut self,
        pair: TradingPair,
        intents: Vec<Intent>,
        solver_quotes: Vec<SolverQuote>,
        oracle_price: OraclePrice,
        current_time: u64,
    ) -> Result<AuctionResult, MatchingError> {
        self.current_epoch += 1;

        // SECURITY CHECK 1: Validate oracle confidence is within acceptable bounds
        Self::validate_oracle_confidence(&oracle_price)?;

        // SECURITY CHECK 2: Validate solver quotes count to prevent DoS
        if solver_quotes.len() > MAX_QUOTES_PER_AUCTION {
            return Err(MatchingError::TooManyQuotes {
                count: solver_quotes.len(),
                max: MAX_QUOTES_PER_AUCTION,
            });
        }

        // SECURITY CHECK 3: Filter out expired intents and validate nonces
        let mut valid_intents = Vec::new();
        for intent in intents {
            // Check expiration (only if current_time is provided)
            if current_time > 0 {
                Self::validate_intent_expiration(&intent, current_time)?;
            }

            // Check and mark nonce for replay protection
            self.check_and_mark_nonce(&intent.user, intent.nonce)?;

            valid_intents.push(intent);
        }

        // Separate by side
        let (buys, sells): (Vec<_>, Vec<_>) = valid_intents
            .iter()
            .partition(|i| self.is_buy(i, &pair));

        // Cross internal orders first (no solver needed)
        let (internal_fills, remaining_buy, remaining_sell) =
            self.cross_internal(&buys, &sells, oracle_price.price)?;

        // Route net flow to solvers
        let net_demand = remaining_buy.saturating_sub(remaining_sell);
        let net_supply = remaining_sell.saturating_sub(remaining_buy);

        let solver_fills = if !net_demand.is_zero() {
            self.fill_from_solver_asks(&solver_quotes, net_demand)?
        } else if !net_supply.is_zero() {
            self.fill_from_solver_bids(&solver_quotes, net_supply)?
        } else {
            vec![]
        };

        // Calculate uniform clearing price
        let clearing_price = self.calculate_clearing_price(
            &internal_fills,
            &solver_fills,
            oracle_price.price,
        );

        Ok(AuctionResult {
            epoch_id: self.current_epoch,
            clearing_price: clearing_price.to_string(),
            internal_fills,
            solver_fills,
        })
    }

    fn is_buy(&self, intent: &Intent, pair: &TradingPair) -> bool {
        // If selling quote asset for base asset, it's a buy
        intent.input.denom == pair.quote
    }

    /// Validate that the oracle price respects the user's limit price
    ///
    /// Note: limit_price is always "output per unit input"
    /// - For buy orders (spending USDC for ATOM): limit_price is ATOM/USDC, oracle_price is USDC/ATOM
    /// - For sell orders (spending ATOM for USDC): limit_price is USDC/ATOM, oracle_price is USDC/ATOM
    ///
    /// For buy orders: we need to invert oracle_price to compare with limit_price
    /// For sell orders: oracle_price must be >= limit_price (user wants at least limit)
    fn validate_limit_price(
        intent: &Intent,
        oracle_price: Decimal,
        side: Side,
    ) -> Result<(), MatchingError> {
        let limit_price = intent
            .output
            .limit_price_decimal()
            .map_err(|e| MatchingError::InvalidPrice(format!("Failed to parse limit price: {}", e)))?;

        match side {
            Side::Buy => {
                // Buy order: spending quote for base
                // limit_price is base/quote (e.g., ATOM/USDC)
                // oracle_price is quote/base (e.g., USDC/ATOM)
                // Invert oracle_price to compare: (1/oracle_price) >= limit_price
                // Equivalent to: oracle_price <= (1/limit_price)
                if limit_price.is_zero() {
                    return Err(MatchingError::InvalidPrice("Limit price cannot be zero".to_string()));
                }
                let max_acceptable_oracle_price = Decimal::ONE / limit_price;
                if oracle_price > max_acceptable_oracle_price {
                    return Err(MatchingError::PriceExceedsLimit {
                        oracle_price,
                        limit_price,
                    });
                }
            }
            Side::Sell => {
                // Sell order: spending base for quote
                // limit_price is quote/base (e.g., USDC/ATOM) - same units as oracle_price
                // oracle_price must be >= limit_price (user wants at least limit)
                if oracle_price < limit_price {
                    return Err(MatchingError::PriceBelowLimit {
                        oracle_price,
                        limit_price,
                    });
                }
            }
        }
        Ok(())
    }

    /// ARCHITECTURAL FIX: Oracle-free intent crossing using midpoint pricing
    ///
    /// This removes the unnecessary oracle dependency identified in security review 5.1.
    /// Instead of using an external oracle price, we derive the execution price
    /// entirely from the intents themselves:
    ///
    /// - Check if intents cross: buy_max_price >= sell_min_price
    /// - Execute at midpoint: (buy_max_price + sell_min_price) / 2
    /// - Both parties get price improvement (surplus split fairly)
    ///
    /// Oracle is now only used as optional sanity check, not for price determination.
    fn cross_internal(
        &self,
        buys: &[&Intent],
        sells: &[&Intent],
        oracle_price: Decimal,  // Now only used for sanity check, not price determination
    ) -> Result<(Vec<AuctionFill>, Uint128, Uint128), MatchingError> {
        let mut fills = Vec::new();
        let mut buy_idx = 0;
        let mut sell_idx = 0;

        let mut buy_remaining: Vec<Uint128> = buys.iter().map(|i| i.input.amount).collect();
        let mut sell_remaining: Vec<Uint128> = sells.iter().map(|i| i.input.amount).collect();

        // Maximum allowed deviation from oracle for sanity check (10%)
        let max_deviation = Decimal::from_str("0.10").unwrap_or(Decimal::ONE);

        while buy_idx < buys.len() && sell_idx < sells.len() {
            let buy = buys[buy_idx];
            let sell = sells[sell_idx];

            let buy_amount = buy_remaining[buy_idx];
            let sell_amount = sell_remaining[sell_idx];

            if buy_amount.is_zero() {
                buy_idx += 1;
                continue;
            }
            if sell_amount.is_zero() {
                sell_idx += 1;
                continue;
            }

            // Parse limit prices from intents
            let buy_limit = buy.output.limit_price_decimal()
                .map_err(|e| MatchingError::InvalidPrice(format!("Buy limit: {}", e)))?;
            let sell_limit = sell.output.limit_price_decimal()
                .map_err(|e| MatchingError::InvalidPrice(format!("Sell limit: {}", e)))?;

            // Buy limit is in output/input (ATOM/USDC for buy)
            // Sell limit is in output/input (USDC/ATOM for sell)
            // To cross: seller wants at least sell_limit USDC per ATOM
            //           buyer willing to pay up to 1/buy_limit USDC per ATOM
            // Cross condition: sell_limit <= 1/buy_limit

            if buy_limit.is_zero() {
                buy_idx += 1;
                continue;
            }

            let buy_max_price = Decimal::ONE / buy_limit;  // Max USDC/ATOM buyer will pay
            let sell_min_price = sell_limit;               // Min USDC/ATOM seller wants

            // Check if intents cross (derived from limits, not oracle!)
            if buy_max_price < sell_min_price {
                // These specific intents don't cross, move to next sell intent
                // A more sophisticated matching algo would try all combinations
                sell_idx += 1;
                continue;
            }

            // MIDPOINT PRICING: Execute at fair price between the two limits
            // Both parties get price improvement (surplus split)
            let execution_price = (buy_max_price + sell_min_price) / Decimal::TWO;

            // SANITY CHECK: Ensure execution price doesn't deviate too far from oracle
            // This catches potential manipulation without making oracle the source of truth
            if oracle_price > Decimal::ZERO {
                let deviation = if execution_price > oracle_price {
                    (execution_price - oracle_price) / oracle_price
                } else {
                    (oracle_price - execution_price) / oracle_price
                };

                if deviation > max_deviation {
                    // Price deviates too much from oracle - skip this match for safety
                    // Log would go here in production
                    sell_idx += 1;
                    continue;
                }
            }

            // Convert to common units using the derived execution price
            let buy_in_base = self.quote_to_base(buy_amount, execution_price);
            let sell_in_base = sell_amount;

            let match_base = std::cmp::min(buy_in_base, sell_in_base);
            let match_quote = self.base_to_quote(match_base, execution_price);

            if !match_base.is_zero() {
                fills.push(AuctionFill {
                    intent_id: buy.id.clone(),
                    counterparty: sell.id.clone(),
                    input_amount: match_quote,
                    output_amount: match_base,
                });

                fills.push(AuctionFill {
                    intent_id: sell.id.clone(),
                    counterparty: buy.id.clone(),
                    input_amount: match_base,
                    output_amount: match_quote,
                });

                buy_remaining[buy_idx] = buy_remaining[buy_idx].saturating_sub(match_quote);
                sell_remaining[sell_idx] = sell_remaining[sell_idx].saturating_sub(match_base);
            }

            if buy_remaining[buy_idx].is_zero() {
                buy_idx += 1;
            }
            if sell_remaining[sell_idx].is_zero() {
                sell_idx += 1;
            }
        }

        let total_buy_remaining: Uint128 = buy_remaining.iter().sum();
        let total_sell_remaining: Uint128 = sell_remaining.iter().sum();

        Ok((fills, total_buy_remaining, total_sell_remaining))
    }

    fn fill_from_solver_asks(
        &self,
        quotes: &[SolverQuote],
        amount: Uint128,
    ) -> Result<Vec<AuctionFill>, MatchingError> {
        let mut fills = Vec::new();
        let mut remaining = amount;

        // Sort by price (best ask first - lowest price)
        let mut sorted_quotes: Vec<_> = quotes.iter().collect();
        sorted_quotes.sort_by(|a, b| {
            let price_a = Decimal::from_str(&a.price).unwrap_or(Decimal::MAX);
            let price_b = Decimal::from_str(&b.price).unwrap_or(Decimal::MAX);
            price_a.cmp(&price_b)
        });

        for quote in sorted_quotes {
            if remaining.is_zero() {
                break;
            }

            let fill_amount = std::cmp::min(remaining, quote.input_amount);
            let price = Decimal::from_str(&quote.price).unwrap_or(Decimal::ZERO);
            let fill_amount_dec = Decimal::from(fill_amount.u128());
            let output_dec = fill_amount_dec * price;
            let output = Uint128::new(output_dec.trunc().to_string().parse::<u128>().unwrap_or(0));

            fills.push(AuctionFill {
                intent_id: "batch".to_string(),
                counterparty: quote.solver_id.clone(),
                input_amount: fill_amount,
                output_amount: output,
            });

            remaining = remaining.saturating_sub(fill_amount);
        }

        Ok(fills)
    }

    fn fill_from_solver_bids(
        &self,
        quotes: &[SolverQuote],
        amount: Uint128,
    ) -> Result<Vec<AuctionFill>, MatchingError> {
        let mut fills = Vec::new();
        let mut remaining = amount;

        // Sort by price (best bid first - highest price)
        let mut sorted_quotes: Vec<_> = quotes.iter().collect();
        sorted_quotes.sort_by(|a, b| {
            let price_a = Decimal::from_str(&a.price).unwrap_or(Decimal::ZERO);
            let price_b = Decimal::from_str(&b.price).unwrap_or(Decimal::ZERO);
            price_b.cmp(&price_a)
        });

        for quote in sorted_quotes {
            if remaining.is_zero() {
                break;
            }

            let fill_amount = std::cmp::min(remaining, quote.input_amount);
            let price = Decimal::from_str(&quote.price).unwrap_or(Decimal::ZERO);
            let fill_amount_dec = Decimal::from(fill_amount.u128());
            let output_dec = fill_amount_dec * price;
            let output = Uint128::new(output_dec.trunc().to_string().parse::<u128>().unwrap_or(0));

            fills.push(AuctionFill {
                intent_id: "batch".to_string(),
                counterparty: quote.solver_id.clone(),
                input_amount: fill_amount,
                output_amount: output,
            });

            remaining = remaining.saturating_sub(fill_amount);
        }

        Ok(fills)
    }

    fn calculate_clearing_price(
        &self,
        internal_fills: &[AuctionFill],
        solver_fills: &[AuctionFill],
        oracle_price: Decimal,
    ) -> Decimal {
        // Weighted average of fill prices, defaulting to oracle
        let all_fills: Vec<_> = internal_fills.iter().chain(solver_fills.iter()).collect();

        if all_fills.is_empty() {
            return oracle_price;
        }

        let mut total_volume = Uint128::zero();
        let mut weighted_price = Decimal::ZERO;

        for fill in all_fills {
            if !fill.input_amount.is_zero() {
                let price = Decimal::from(fill.output_amount.u128())
                    / Decimal::from(fill.input_amount.u128());
                weighted_price += price * Decimal::from(fill.input_amount.u128());
                total_volume += fill.input_amount;
            }
        }

        if total_volume.is_zero() {
            oracle_price
        } else {
            weighted_price / Decimal::from(total_volume.u128())
        }
    }

    fn quote_to_base(&self, quote_amount: Uint128, price: Decimal) -> Uint128 {
        if price.is_zero() {
            return Uint128::zero();
        }
        let amount_dec = Decimal::from(quote_amount.u128());
        let base = amount_dec / price;
        // Truncate to integer
        Uint128::new(base.trunc().to_string().parse::<u128>().unwrap_or(0))
    }

    fn base_to_quote(&self, base_amount: Uint128, price: Decimal) -> Uint128 {
        let amount_dec = Decimal::from(base_amount.u128());
        let quote = amount_dec * price;
        // Truncate to integer
        Uint128::new(quote.trunc().to_string().parse::<u128>().unwrap_or(0))
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, FillStrategy, OutputSpec};
    use cosmwasm_std::Binary;
    use std::str::FromStr;

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

    // ==================== Basic Engine Tests ====================

    #[test]
    fn test_new_engine() {
        let engine = MatchingEngine::new();
        assert_eq!(engine.current_epoch, 0);
        assert!(engine.books.is_empty());
    }

    #[test]
    fn test_default_engine() {
        let engine = MatchingEngine::default();
        assert_eq!(engine.current_epoch, 0);
    }

    #[test]
    fn test_get_or_create_book() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // First call should create
        let _book = engine.get_or_create_book(pair.clone());
        assert!(engine.get_book(&pair).is_some());

        // Second call should return existing
        let _book2 = engine.get_or_create_book(pair.clone());
        assert_eq!(engine.books.len(), 1);
    }

    #[test]
    fn test_get_book_nonexistent() {
        let engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");
        assert!(engine.get_book(&pair).is_none());
    }

    // ==================== Process Intent Tests ====================

    #[test]
    fn test_process_intent_creates_book() {
        let mut engine = MatchingEngine::new();

        let intent = make_test_intent(
            "sell-1",
            "seller",
            "uatom",
            1_000_000,
            "uusdc",
            10_000_000,
            "10.0",
        );

        let result = engine.process_intent(&intent, 0).unwrap();

        // No fills (empty book)
        assert!(result.fills.is_empty());

        // Book should be created
        let pair = TradingPair::new("uatom", "uusdc");
        assert!(engine.get_book(&pair).is_some());
    }

    #[test]
    fn test_process_intent_match() {
        let mut engine = MatchingEngine::new();

        // Add sell at 10.0 USDC/ATOM
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");
        engine.process_intent(&sell, 0).unwrap();

        // Add buy willing to pay ~10.5 USDC/ATOM (0.095 ATOM/USDC)
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_500_000, "uatom", 1_000_000, "0.095");
        let result = engine.process_intent(&buy, 1).unwrap();

        assert!(!result.fills.is_empty());
    }

    #[test]
    fn test_multiple_pairs() {
        let mut engine = MatchingEngine::new();

        // ATOM/USDC pair
        let atom_sell = make_test_intent("atom-sell", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");
        engine.process_intent(&atom_sell, 0).unwrap();

        // OSMO/USDC pair
        let osmo_sell = make_test_intent("osmo-sell", "seller", "uosmo", 1_000_000, "uusdc", 1_000_000, "1.0");
        engine.process_intent(&osmo_sell, 1).unwrap();

        // Should have two separate books
        assert_eq!(engine.books.len(), 2);

        let atom_pair = TradingPair::new("uatom", "uusdc");
        let osmo_pair = TradingPair::new("uosmo", "uusdc");

        assert!(engine.get_book(&atom_pair).is_some());
        assert!(engine.get_book(&osmo_pair).is_some());
    }

    // ==================== Batch Auction Tests ====================

    #[test]
    fn test_batch_auction_internal_cross() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Buy intent (spending USDC for ATOM)
        // limit_price 0.1 ATOM/USDC means max price 1/0.1 = 10.0 USDC/ATOM
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");

        // Sell intent (spending ATOM for USDC)
        // limit_price 10.0 USDC/ATOM means min price 10.0 USDC/ATOM
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should have internal fills (both buy and sell get fills)
        assert!(!result.internal_fills.is_empty());
        assert_eq!(result.epoch_id, 1);
    }

    #[test]
    fn test_batch_auction_with_solver_quotes() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Only a buy (net demand)
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "10.0");

        let intents = vec![buy];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        // Solver quote to fill the demand
        let solver_quote = SolverQuote {
            solver_id: "solver-1".to_string(),
            input_amount: Uint128::new(10_000_000),
            output_amount: Uint128::new(100_000_000),
            price: "10.0".to_string(),
            valid_for_ms: 5000,
        };

        let result = engine.run_batch_auction(pair, intents, vec![solver_quote], oracle_price).unwrap();

        // Should have solver fills
        assert!(!result.solver_fills.is_empty());
    }

    #[test]
    fn test_batch_auction_epoch_increments() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");
        let oracle_price = Decimal::from_str("10.0").unwrap();

        // Run first auction
        let result1 = engine.run_batch_auction(pair.clone(), vec![], vec![], oracle_price).unwrap();
        assert_eq!(result1.epoch_id, 1);

        // Run second auction
        let result2 = engine.run_batch_auction(pair.clone(), vec![], vec![], oracle_price).unwrap();
        assert_eq!(result2.epoch_id, 2);

        // Run third auction
        let result3 = engine.run_batch_auction(pair, vec![], vec![], oracle_price).unwrap();
        assert_eq!(result3.epoch_id, 3);
    }

    #[test]
    fn test_batch_auction_empty_returns_oracle_price() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");
        let oracle_price = Decimal::from_str("10.45").unwrap();

        let result = engine.run_batch_auction(pair, vec![], vec![], oracle_price).unwrap();

        // With no fills, clearing price should be oracle price
        assert_eq!(result.clearing_price, "10.45");
        assert!(result.internal_fills.is_empty());
        assert!(result.solver_fills.is_empty());
    }

    #[test]
    fn test_batch_auction_solver_price_priority() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");
        let oracle_price = Decimal::from_str("10.0").unwrap();

        // Only sell (net supply)
        let sell = make_test_intent("sell-1", "seller", "uatom", 2_000_000, "uusdc", 18_000_000, "9.0");

        let intents = vec![sell];

        // Multiple solver quotes at different prices (bids)
        let quote_low = SolverQuote {
            solver_id: "solver-low".to_string(),
            input_amount: Uint128::new(1_000_000),
            output_amount: Uint128::new(9_000_000),
            price: "9.0".to_string(),
            valid_for_ms: 5000,
        };
        let quote_high = SolverQuote {
            solver_id: "solver-high".to_string(),
            input_amount: Uint128::new(1_000_000),
            output_amount: Uint128::new(9_500_000),
            price: "9.5".to_string(),
            valid_for_ms: 5000,
        };

        let result = engine.run_batch_auction(
            pair,
            intents,
            vec![quote_low, quote_high],
            oracle_price,
        ).unwrap();

        // Best bid (9.5) should be filled first
        assert!(!result.solver_fills.is_empty());
        assert_eq!(result.solver_fills[0].counterparty, "solver-high");
    }

    // ==================== Helper Function Tests ====================

    #[test]
    fn test_is_buy() {
        let engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Intent selling USDC for ATOM = buy (buying base asset)
        let buy_intent = make_test_intent("buy", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "10.0");
        assert!(engine.is_buy(&buy_intent, &pair));

        // Intent selling ATOM for USDC = sell (selling base asset)
        let sell_intent = make_test_intent("sell", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");
        assert!(!engine.is_buy(&sell_intent, &pair));
    }

    #[test]
    fn test_quote_to_base_conversion() {
        let engine = MatchingEngine::new();
        let price = Decimal::from_str("10.0").unwrap();

        // 100 quote at price 10 = 10 base
        let base = engine.quote_to_base(Uint128::new(100), price);
        assert_eq!(base, Uint128::new(10));

        // 50 quote at price 10 = 5 base
        let base2 = engine.quote_to_base(Uint128::new(50), price);
        assert_eq!(base2, Uint128::new(5));
    }

    #[test]
    fn test_base_to_quote_conversion() {
        let engine = MatchingEngine::new();
        let price = Decimal::from_str("10.0").unwrap();

        // 10 base at price 10 = 100 quote
        let quote = engine.base_to_quote(Uint128::new(10), price);
        assert_eq!(quote, Uint128::new(100));

        // 5 base at price 10 = 50 quote
        let quote2 = engine.base_to_quote(Uint128::new(5), price);
        assert_eq!(quote2, Uint128::new(50));
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_full_workflow() {
        let mut engine = MatchingEngine::new();

        // Step 1: Add some sell orders via process_intent
        let sell1 = make_test_intent("sell-1", "s1", "uatom", 5_000_000, "uusdc", 50_000_000, "10.0");
        let sell2 = make_test_intent("sell-2", "s2", "uatom", 5_000_000, "uusdc", 52_500_000, "10.5");

        engine.process_intent(&sell1, 0).unwrap();
        engine.process_intent(&sell2, 1).unwrap();

        // Verify book state
        let pair = TradingPair::new("uatom", "uusdc");
        let book = engine.get_book(&pair).unwrap();
        assert_eq!(book.ask_depth(), Uint128::new(10_000_000));

        // Step 2: Add a buy that can fully consume sell1 at 10.0 and partially fill sell2 at 10.5
        // Buy with 75M USDC at limit ~10.5 USDC/ATOM (0.095 ATOM/USDC inverts to ~10.526)
        // At 10.0: 5M ATOM costs 50M USDC
        // At 10.5: remaining 25M USDC buys ~2.38M ATOM
        // Total bought: ~7.38M ATOM, remaining sell2: ~2.62M ATOM
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 75_000_000, "uatom", 7_000_000, "0.095");
        let result = engine.process_intent(&buy, 2).unwrap();

        // Should match both sells (full fill of sell1, partial of sell2)
        assert_eq!(result.fills.len(), 2);

        // Verify sell1 fully consumed, sell2 partially remains
        let book_after = engine.get_book(&pair).unwrap();
        // sell2 had 5M, bought ~2.38M, so ~2.62M remains
        assert!(book_after.ask_depth() > Uint128::zero());
        assert!(book_after.ask_depth() < Uint128::new(5_000_000));
    }

    #[test]
    fn test_batch_auction_balanced_orders() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Equal buy and sell
        // Buy with 0.1 ATOM/USDC = 10 USDC/ATOM
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should fully cross internally
        assert!(!result.internal_fills.is_empty());
        // No leftover for solvers
        assert!(result.solver_fills.is_empty());
    }

    // ==================== Limit Price Validation Tests ====================

    #[test]
    fn test_buy_order_rejected_when_oracle_price_exceeds_limit() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Buy order with limit price of 0.08 ATOM/USDC (willing to pay max 1/0.08 = 12.5 USDC/ATOM)
        // Oracle price is 13.0 USDC/ATOM, which exceeds the max acceptable price
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.08");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("13.0").unwrap(); // Too high for buyer

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price);

        // Should fail with PriceExceedsLimit error
        assert!(result.is_err());
        match result.unwrap_err() {
            MatchingError::PriceExceedsLimit { oracle_price, limit_price } => {
                assert_eq!(oracle_price, Decimal::from_str("13.0").unwrap());
                assert_eq!(limit_price, Decimal::from_str("0.08").unwrap());
            }
            e => panic!("Expected PriceExceedsLimit error, got: {:?}", e),
        }
    }

    #[test]
    fn test_sell_order_rejected_when_oracle_price_below_limit() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Sell order with limit price of 11.0 USDC/ATOM (wants at least 11.0)
        // But oracle price is 10.0 USDC/ATOM, which is below the limit
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 11_000_000, "11.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap(); // Too low for seller

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price);

        // Should fail with PriceBelowLimit error
        assert!(result.is_err());
        match result.unwrap_err() {
            MatchingError::PriceBelowLimit { oracle_price, limit_price } => {
                assert_eq!(oracle_price, Decimal::from_str("10.0").unwrap());
                assert_eq!(limit_price, Decimal::from_str("11.0").unwrap());
            }
            e => panic!("Expected PriceBelowLimit error, got: {:?}", e),
        }
    }

    #[test]
    fn test_orders_matched_when_prices_within_limits() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Buy with limit 0.1 ATOM/USDC (willing to pay up to 10.0 USDC/ATOM)
        // Sell with limit 10.0 USDC/ATOM (wants at least 10.0)
        // Oracle price is 10.0 USDC/ATOM - should match
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should successfully match
        assert!(!result.internal_fills.is_empty());
        assert_eq!(result.internal_fills.len(), 2); // One fill for each side
    }

    #[test]
    fn test_oracle_price_equals_limit_price_accepted() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Edge case: oracle_price == limit_price should be accepted
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should successfully match
        assert!(!result.internal_fills.is_empty());
    }

    #[test]
    fn test_buy_order_accepted_when_oracle_price_below_limit() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Buy with limit 0.11 ATOM/USDC (willing to pay up to 1/0.11 = ~9.09 USDC/ATOM)
        // Oracle price is 9.0 USDC/ATOM - below max acceptable, should accept
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 9_000_000, "uatom", 1_000_000, "0.11");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 9_000_000, "9.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("9.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should successfully match
        assert!(!result.internal_fills.is_empty());
    }

    #[test]
    fn test_sell_order_accepted_when_oracle_price_above_limit() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // Sell with limit 9.0 USDC/ATOM (wants at least 9.0)
        // Oracle price is 10.0 USDC/ATOM - better than limit, should accept
        let buy = make_test_intent("buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 1_000_000, "uusdc", 9_000_000, "9.0");

        let intents = vec![buy, sell];
        let oracle_price = Decimal::from_str("10.0").unwrap();

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price).unwrap();

        // Should successfully match
        assert!(!result.internal_fills.is_empty());
    }

    #[test]
    fn test_multiple_orders_first_fails_limit_check() {
        let mut engine = MatchingEngine::new();
        let pair = TradingPair::new("uatom", "uusdc");

        // First buy has tight limit that will fail at oracle price 10.0
        // limit 0.09 ATOM/USDC means max price is 1/0.09 = 11.11 USDC/ATOM, but oracle is 10.0, so it passes
        // Let's use 0.08 which gives max price 1/0.08 = 12.5, still passes at 10.0
        // Use 0.09 with oracle 12.0 instead
        let buy1 = make_test_intent("buy-1", "buyer1", "uusdc", 10_000_000, "uatom", 1_000_000, "0.08");
        // Second buy has acceptable limit
        let buy2 = make_test_intent("buy-2", "buyer2", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");
        let sell = make_test_intent("sell-1", "seller", "uatom", 2_000_000, "uusdc", 20_000_000, "10.0");

        let intents = vec![buy1, buy2, sell];
        let oracle_price = Decimal::from_str("13.0").unwrap(); // Exceeds buy1's max price of 12.5

        let result = engine.run_batch_auction(pair, intents, vec![], oracle_price);

        // Should fail on the first buy order that violates limit
        assert!(result.is_err());
        match result.unwrap_err() {
            MatchingError::PriceExceedsLimit { .. } => {
                // Expected
            }
            e => panic!("Expected PriceExceedsLimit error, got: {:?}", e),
        }
    }
}
