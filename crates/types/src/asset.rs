use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;

/// Asset specification
#[cw_serde]
pub struct Asset {
    /// Source chain (e.g., "cosmoshub-4")
    pub chain_id: String,

    /// Token denomination
    pub denom: String,

    /// Amount in base units (e.g., uatom)
    pub amount: Uint128,
}

impl Asset {
    pub fn new(chain_id: impl Into<String>, denom: impl Into<String>, amount: u128) -> Self {
        Self {
            chain_id: chain_id.into(),
            denom: denom.into(),
            amount: Uint128::new(amount),
        }
    }
}

/// Output specification
#[cw_serde]
pub struct OutputSpec {
    /// Destination chain
    pub chain_id: String,

    /// Desired token denomination
    pub denom: String,

    /// Minimum acceptable amount
    pub min_amount: Uint128,

    /// Limit price (output per unit input) as string for serialization
    pub limit_price: String,

    /// Recipient address on destination chain
    pub recipient: String,
}

impl OutputSpec {
    pub fn limit_price_decimal(&self) -> Result<Decimal, rust_decimal::Error> {
        self.limit_price.parse()
    }
}

/// Trading pair representation
#[cw_serde]
#[derive(Eq, Hash)]
pub struct TradingPair {
    pub base: String,
    pub quote: String,
}

impl TradingPair {
    pub fn new(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
        }
    }

    pub fn to_symbol(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    pub fn from_symbol(symbol: &str) -> Option<Self> {
        let parts: Vec<&str> = symbol.split('/').collect();
        if parts.len() == 2 {
            Some(Self::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}
