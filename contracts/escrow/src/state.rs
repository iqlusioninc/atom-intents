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
    Released {
        recipient: String,
    },
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

// ═══════════════════════════════════════════════════════════════════════════════
// ETHEREUM ESCROW STATE (via Eureka)
// ═══════════════════════════════════════════════════════════════════════════════

/// Ethereum escrow waiting for Eureka IBC transfer to finalize
#[cw_serde]
pub struct EthereumEscrow {
    /// Intent ID this escrow is for
    pub intent_id: String,
    /// Ethereum sender address (0x...)
    pub eth_sender: String,
    /// Expected amount from Ethereum
    pub expected_amount: Uint128,
    /// Actual denom that will arrive (e.g., ibc/...)
    pub expected_denom: String,
    /// When this escrow was registered
    pub registered_at: u64,
    /// Timeout for Eureka transfer (absolute timestamp)
    pub eureka_timeout: u64,
    /// Current status of the escrow
    pub status: EthereumEscrowStatus,
    /// Eureka packet ID once received
    pub packet_id: Option<String>,
    /// Actual amount received (may differ from expected)
    pub received_amount: Option<Uint128>,
    /// Block number when ZK proof was verified
    pub finalized_at_block: Option<u64>,
    /// Fronting information (if solver fronted before finality)
    pub fronting: Option<FrontingInfo>,
}

/// Status of an Ethereum escrow
#[cw_serde]
pub enum EthereumEscrowStatus {
    /// Waiting for Eureka packet to arrive
    Pending,
    /// Eureka packet received, waiting for ZK finality
    Received,
    /// ZK proof verified, escrow finalized
    Finalized,
    /// Solver fronted settlement before finality
    Fronted,
    /// Settlement claimed by solver after finality
    Claimed,
    /// Escrow failed (timeout, invalid packet, etc.)
    Failed { reason: String },
}

/// Information about solver fronting
#[cw_serde]
pub struct FrontingInfo {
    /// Solver who fronted the settlement
    pub solver_id: String,
    /// Solver's address (for claiming escrow)
    pub solver_addr: Addr,
    /// When fronting occurred
    pub fronted_at: u64,
    /// Risk bond posted by solver
    pub bond_amount: Uint128,
    /// Amount solver paid to user
    pub output_amount: Uint128,
}

/// Storage for Ethereum escrows (keyed by intent_id)
pub const ETHEREUM_ESCROWS: Map<&str, EthereumEscrow> = Map::new("eth_escrows");
