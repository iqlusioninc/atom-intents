use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub settlement_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Lock funds in escrow
    Lock {
        escrow_id: String,
        intent_id: String,
        expires_at: u64,
    },
    /// Release escrowed funds to a recipient
    Release {
        escrow_id: String,
        recipient: String,
    },
    /// Refund escrowed funds to owner (after expiry)
    Refund { escrow_id: String },
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
}

#[cw_serde]
pub struct EscrowsResponse {
    pub escrows: Vec<EscrowResponse>,
}
