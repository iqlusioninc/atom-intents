use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub settlement_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Lock funds in escrow (for local Hub users)
    Lock {
        escrow_id: String,
        intent_id: String,
        expires_at: u64,
    },
    /// Lock funds via IBC Hooks (for cross-chain users like Celestia)
    /// This is called by the IBC Hooks middleware when funds arrive via IBC
    /// with a wasm memo pointing to this contract.
    LockFromIbc {
        /// Intent ID this escrow is for
        intent_id: String,
        /// Expiration timestamp for the escrow
        expires_at: u64,
        /// User's address on the source chain (e.g., celestia1...)
        user_source_address: String,
        /// Source chain ID (e.g., "celestia")
        source_chain_id: String,
        /// IBC channel the funds came through (for refunds)
        source_channel: String,
    },
    /// Release escrowed funds to a recipient
    Release {
        escrow_id: String,
        recipient: String,
    },
    /// Refund escrowed funds to owner (after expiry)
    /// For cross-chain escrows, this initiates an IBC transfer back to source
    Refund { escrow_id: String },
    /// Retry a failed IBC refund
    RetryRefund { escrow_id: String },
    /// Update config (admin only)
    UpdateConfig {
        admin: Option<String>,
        settlement_contract: Option<String>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(EscrowResponse)]
    Escrow { escrow_id: String },

    #[returns(EscrowsResponse)]
    EscrowsByUser {
        user: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },

    #[returns(EscrowResponse)]
    EscrowByIntent { intent_id: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub admin: String,
    pub settlement_contract: String,
}

#[cw_serde]
pub struct EscrowResponse {
    pub id: String,
    pub owner: String,
    pub amount: Uint128,
    pub denom: String,
    pub intent_id: String,
    pub expires_at: u64,
    pub status: String,
    /// Chain ID where owner address exists (None = local Hub)
    pub owner_chain_id: Option<String>,
    /// User's address on source chain (for cross-chain escrows)
    pub owner_source_address: Option<String>,
    /// IBC channel for refunds (for cross-chain escrows)
    pub source_channel: Option<String>,
}

#[cw_serde]
pub struct EscrowsResponse {
    pub escrows: Vec<EscrowResponse>,
}
