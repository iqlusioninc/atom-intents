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
    /// User who deposited (on Hub for local, or source address for cross-chain)
    pub owner: Addr,
    /// Amount escrowed
    pub amount: Uint128,
    /// Token denomination (ibc/... for cross-chain tokens)
    pub denom: String,
    /// Intent ID this escrow is for
    pub intent_id: String,
    /// Expiration timestamp
    pub expires_at: u64,
    /// Whether released or refunded
    pub status: EscrowStatus,

    // Cross-chain escrow fields (for IBC Hooks support)
    /// Chain ID where the owner address exists (None = local Hub address)
    pub owner_chain_id: Option<String>,
    /// User's address on the source chain (for cross-chain refunds)
    pub owner_source_address: Option<String>,
    /// IBC channel used for inbound transfer (for refunds via IBC)
    pub source_channel: Option<String>,
    /// Original denom on source chain (for refund routing)
    pub source_denom: Option<String>,
}

#[cw_serde]
pub enum EscrowStatus {
    Locked,
    Released { recipient: String },
    Refunded,
    /// Refund initiated via IBC (waiting for ack)
    Refunding,
    /// IBC refund failed, can be retried
    RefundFailed,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const ESCROWS: Map<&str, Escrow> = Map::new("escrows");
pub const USER_ESCROWS: Map<(&Addr, &str), bool> = Map::new("user_escrows");
/// Index: intent_id -> escrow_id (prevents duplicate escrows per intent)
pub const ESCROWS_BY_INTENT: Map<&str, String> = Map::new("escrows_by_intent");
