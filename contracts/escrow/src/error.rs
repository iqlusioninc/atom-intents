use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Escrow not found: {id}")]
    EscrowNotFound { id: String },

    #[error("Escrow already exists: {id}")]
    EscrowAlreadyExists { id: String },

    #[error("Escrow expired: {id}")]
    EscrowExpired { id: String },

    #[error("Escrow not expired: {id}")]
    EscrowNotExpired { id: String },

    #[error("Invalid funds: expected {expected}, got {got}")]
    InvalidFunds { expected: String, got: String },

    #[error("Insufficient funds")]
    InsufficientFunds {},

    #[error("Intent already has escrow: {intent_id}")]
    IntentAlreadyEscrowed { intent_id: String },

    #[error("Not IBC funds: LockFromIbc requires funds with ibc/ denom prefix")]
    NotIbcFunds {},

    #[error("Invalid escrow status for this operation")]
    InvalidStatus {},

    #[error("Missing required field for cross-chain escrow: {field}")]
    MissingCrossChainField { field: String },

    #[error("IBC refund failed for escrow: {id}")]
    IbcRefundFailed { id: String },

    #[error("Feature not implemented: {feature}")]
    NotImplemented { feature: String },

    #[error("Ethereum escrow not found for intent: {intent_id}")]
    EthereumEscrowNotFound { intent_id: String },

    #[error("Invalid Ethereum escrow status: expected {expected}, got {actual}")]
    InvalidEthereumEscrowStatus { expected: String, actual: String },

    #[error("Ethereum sender mismatch: expected {expected}, got {actual}")]
    EthereumSenderMismatch { expected: String, actual: String },

    #[error("Eureka timeout for intent: {intent_id}")]
    EurekaTimeout { intent_id: String },

    #[error("Packet ID mismatch: expected {expected}, got {actual}")]
    PacketIdMismatch { expected: String, actual: String },

    #[error("Escrow not fronted for intent: {intent_id}")]
    NotFronted { intent_id: String },

    #[error("Insufficient bond: required {required}, provided {provided}")]
    InsufficientBond {
        required: cosmwasm_std::Uint128,
        provided: cosmwasm_std::Uint128,
    },
}
