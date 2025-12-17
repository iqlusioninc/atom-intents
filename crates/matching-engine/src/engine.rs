use atom_intents_types::{
    AuctionFill, AuctionResult, Intent, MatchResult, SolverQuote, TradingPair,
};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

use crate::{MatchingError, OrderBook};

/// Matching engine managing multiple order books
pub struct MatchingEngine {
    books: HashMap<TradingPair, OrderBook>,
    current_epoch: u64,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
            current_epoch: 0,
        }
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

    /// Run a batch auction for multiple intents
    pub fn run_batch_auction(
        &mut self,
        pair: TradingPair,
        intents: Vec<Intent>,
        solver_quotes: Vec<SolverQuote>,
        oracle_price: Decimal,
    ) -> Result<AuctionResult, MatchingError> {
        self.current_epoch += 1;

        // Separate by side
        let (buys, sells): (Vec<_>, Vec<_>) = intents
            .iter()
            .partition(|i| self.is_buy(i, &pair));

        // Cross internal orders first (no solver needed)
        let (internal_fills, remaining_buy, remaining_sell) =
            self.cross_internal(&buys, &sells, oracle_price)?;

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
            oracle_price,
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

    fn cross_internal(
        &self,
        buys: &[&Intent],
        sells: &[&Intent],
        oracle_price: Decimal,
    ) -> Result<(Vec<AuctionFill>, Uint128, Uint128), MatchingError> {
        let mut fills = Vec::new();
        let mut buy_idx = 0;
        let mut sell_idx = 0;

        let mut buy_remaining: Vec<Uint128> = buys.iter().map(|i| i.input.amount).collect();
        let mut sell_remaining: Vec<Uint128> = sells.iter().map(|i| i.input.amount).collect();

        // Match at oracle price
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

            // Convert to common units using oracle price
            let buy_in_base = self.quote_to_base(buy_amount, oracle_price);
            let sell_in_base = sell_amount;

            let match_base = std::cmp::min(buy_in_base, sell_in_base);
            let match_quote = self.base_to_quote(match_base, oracle_price);

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
            let price_a: f64 = a.price.parse().unwrap_or(f64::MAX);
            let price_b: f64 = b.price.parse().unwrap_or(f64::MAX);
            price_a.partial_cmp(&price_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        for quote in sorted_quotes {
            if remaining.is_zero() {
                break;
            }

            let fill_amount = std::cmp::min(remaining, quote.input_amount);
            let price: f64 = quote.price.parse().unwrap_or(0.0);
            let output = Uint128::new((fill_amount.u128() as f64 * price) as u128);

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
            let price_a: f64 = a.price.parse().unwrap_or(0.0);
            let price_b: f64 = b.price.parse().unwrap_or(0.0);
            price_b.partial_cmp(&price_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        for quote in sorted_quotes {
            if remaining.is_zero() {
                break;
            }

            let fill_amount = std::cmp::min(remaining, quote.input_amount);
            let price: f64 = quote.price.parse().unwrap_or(0.0);
            let output = Uint128::new((fill_amount.u128() as f64 * price) as u128);

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
        let amount_dec = Decimal::from(quote_amount.u128());
        let base = amount_dec / price;
        Uint128::new(base.to_string().parse::<u128>().unwrap_or(0))
    }

    fn base_to_quote(&self, base_amount: Uint128, price: Decimal) -> Uint128 {
        let amount_dec = Decimal::from(base_amount.u128());
        let quote = amount_dec * price;
        Uint128::new(quote.to_string().parse::<u128>().unwrap_or(0))
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}
