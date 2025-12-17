use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    /// Admin address
    pub admin: Addr,
    /// Settlement contract address
    pub settlement_contract: Addr,
}

#[cw_serde]
pub struct Escrow {
    /// Unique escrow ID
    pub id: String,
    /// User who deposited
    pub owner: Addr,
    /// Amount escrowed
    pub amount: Uint128,
    /// Token denomination
    pub denom: String,
    /// Intent ID this escrow is for
    pub intent_id: String,
    /// Expiration timestamp
    pub expires_at: u64,
    /// Whether released or refunded
    pub status: EscrowStatus,
}

#[cw_serde]
pub enum EscrowStatus {
    Locked,
    Released { recipient: String },
    Refunded,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ESCROWS: Map<&str, Escrow> = Map::new("escrows");
pub const USER_ESCROWS: Map<(&Addr, &str), bool> = Map::new("user_escrows");
