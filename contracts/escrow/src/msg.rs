use atom_intents_types::Intent;
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

    // ═══════════════════════════════════════════════════════════════════════════════
    // ETHEREUM ESCROW MESSAGES (via Eureka)
    // ═══════════════════════════════════════════════════════════════════════════════
    /// Register an intent with pending Ethereum escrow via Eureka
    RegisterEthereumEscrowIntent {
        intent: Box<Intent>,
        eth_sender: String,
        expected_amount: Uint128,
        eureka_timeout_secs: u64,
    },

    /// Notify that an Eureka packet has been received (called by relayer)
    NotifyEurekaPacketReceived {
        intent_id: String,
        packet_id: String,
        amount: Uint128,
        sender: String,
    },

    /// Notify that an Eureka packet has been finalized with ZK proof (called by relayer)
    NotifyEurekaFinalized {
        intent_id: String,
        packet_id: String,
        proof_block: u64,
    },

    /// Solver fronts settlement before Eureka finality
    FrontSettlement {
        intent_id: String,
        /// The solver's ID
        solver_id: String,
        output_amount: Uint128,
        /// Additional bond for settlement risk
        risk_bond: Uint128,
    },

    /// Claim escrowed funds after Eureka finality (called by solver)
    ClaimEurekaEscrow {
        intent_id: String,
        packet_id: String,
    },

    /// Handle Eureka escrow timeout/failure
    HandleEurekaEscrowFailure {
        intent_id: String,
        reason: String,
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

    /// Query Ethereum escrow status
    #[returns(EthereumEscrowStatusResponse)]
    EthereumEscrowStatus { intent_id: String },
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

#[cw_serde]
pub struct EthereumEscrowStatusResponse {
    pub intent_id: String,
    pub eth_sender: String,
    pub expected_amount: Uint128,
    /// Status: "pending", "received", "finalized", "failed"
    pub escrow_status: String,
    pub packet_id: Option<String>,
    pub fronted_by: Option<String>,
    pub fronted_at: Option<u64>,
}
