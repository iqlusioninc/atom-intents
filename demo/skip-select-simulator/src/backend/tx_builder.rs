//! Cosmos transaction builder
//!
//! This module handles building and encoding Cosmos SDK transactions
//! for CosmWasm contract interactions.

use cosmos_sdk_proto::cosmos::base::v1beta1::Coin;
use cosmos_sdk_proto::cosmos::tx::v1beta1::{
    AuthInfo, Fee, ModeInfo, SignDoc, SignerInfo, TxBody, TxRaw,
};
use cosmos_sdk_proto::cosmwasm::wasm::v1::MsgExecuteContract;
use cosmos_sdk_proto::traits::Message;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::debug;

use super::wallet::CosmosWallet;

/// Transaction builder errors
#[derive(Debug, Error)]
pub enum TxBuilderError {
    #[error("encoding error: {0}")]
    EncodingError(String),

    #[error("signing error: {0}")]
    SigningError(String),

    #[error("invalid message: {0}")]
    InvalidMessage(String),
}

/// Account information needed for signing
#[derive(Debug, Clone)]
pub struct AccountInfo {
    /// Account number on chain
    pub account_number: u64,
    /// Current sequence number (nonce)
    pub sequence: u64,
}

/// Gas configuration
#[derive(Debug, Clone)]
pub struct GasConfig {
    /// Gas limit for the transaction
    pub gas_limit: u64,
    /// Gas price in the fee denom
    pub gas_price: f64,
    /// Fee denomination (e.g., "uatom", "uosmo")
    pub fee_denom: String,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            gas_limit: 500_000,
            gas_price: 0.025,
            fee_denom: "uatom".to_string(),
        }
    }
}

impl GasConfig {
    /// Create gas config for a specific chain
    pub fn for_chain(chain_id: &str) -> Self {
        match chain_id {
            id if id.contains("osmo") => Self {
                gas_limit: 500_000,
                gas_price: 0.025,
                fee_denom: "uosmo".to_string(),
            },
            id if id.contains("neutron") || id.contains("pion") => Self {
                gas_limit: 500_000,
                gas_price: 0.025,
                fee_denom: "untrn".to_string(),
            },
            _ => Self::default(),
        }
    }

    /// Calculate the fee amount
    pub fn fee_amount(&self) -> u64 {
        (self.gas_limit as f64 * self.gas_price).ceil() as u64
    }
}

/// Transaction builder for Cosmos SDK transactions
pub struct TxBuilder {
    /// Chain ID
    chain_id: String,
    /// Gas configuration
    gas_config: GasConfig,
    /// Transaction memo
    memo: String,
    /// Timeout height (0 = no timeout)
    timeout_height: u64,
}

impl TxBuilder {
    /// Create a new transaction builder
    pub fn new(chain_id: &str) -> Self {
        Self {
            chain_id: chain_id.to_string(),
            gas_config: GasConfig::for_chain(chain_id),
            memo: String::new(),
            timeout_height: 0,
        }
    }

    /// Set custom gas configuration
    pub fn with_gas(mut self, gas_config: GasConfig) -> Self {
        self.gas_config = gas_config;
        self
    }

    /// Set transaction memo
    pub fn with_memo(mut self, memo: &str) -> Self {
        self.memo = memo.to_string();
        self
    }

    /// Set timeout height
    pub fn with_timeout(mut self, timeout_height: u64) -> Self {
        self.timeout_height = timeout_height;
        self
    }

    /// Build a CosmWasm execute message
    pub fn build_execute_msg(
        &self,
        sender: &str,
        contract: &str,
        msg: &serde_json::Value,
        funds: Vec<(u128, &str)>,
    ) -> Result<MsgExecuteContract, TxBuilderError> {
        let msg_bytes = serde_json::to_vec(msg)
            .map_err(|e| TxBuilderError::InvalidMessage(format!("JSON serialization failed: {}", e)))?;

        let coins: Vec<Coin> = funds
            .into_iter()
            .map(|(amount, denom)| Coin {
                denom: denom.to_string(),
                amount: amount.to_string(),
            })
            .collect();

        Ok(MsgExecuteContract {
            sender: sender.to_string(),
            contract: contract.to_string(),
            msg: msg_bytes,
            funds: coins,
        })
    }

    /// Build and sign a transaction with a single message
    pub fn build_and_sign(
        &self,
        wallet: &CosmosWallet,
        account_info: &AccountInfo,
        msg: MsgExecuteContract,
    ) -> Result<TxRaw, TxBuilderError> {
        self.build_and_sign_multi(wallet, account_info, vec![msg])
    }

    /// Build and sign a transaction with multiple messages
    pub fn build_and_sign_multi(
        &self,
        wallet: &CosmosWallet,
        account_info: &AccountInfo,
        messages: Vec<MsgExecuteContract>,
    ) -> Result<TxRaw, TxBuilderError> {
        // Encode messages as Any types
        let any_messages: Vec<cosmos_sdk_proto::Any> = messages
            .into_iter()
            .map(|msg| cosmos_sdk_proto::Any {
                type_url: "/cosmwasm.wasm.v1.MsgExecuteContract".to_string(),
                value: msg.encode_to_vec(),
            })
            .collect();

        // Build TxBody
        let tx_body = TxBody {
            messages: any_messages,
            memo: self.memo.clone(),
            timeout_height: self.timeout_height,
            extension_options: vec![],
            non_critical_extension_options: vec![],
        };

        // Build AuthInfo
        let public_key = wallet.public_key_bytes();
        let pubkey_any = cosmos_sdk_proto::Any {
            type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
            value: encode_pubkey(&public_key),
        };

        let signer_info = SignerInfo {
            public_key: Some(pubkey_any),
            mode_info: Some(ModeInfo {
                sum: Some(cosmos_sdk_proto::cosmos::tx::v1beta1::mode_info::Sum::Single(
                    cosmos_sdk_proto::cosmos::tx::v1beta1::mode_info::Single {
                        mode: 1, // SIGN_MODE_DIRECT
                    },
                )),
            }),
            sequence: account_info.sequence,
        };

        let fee = Fee {
            amount: vec![Coin {
                denom: self.gas_config.fee_denom.clone(),
                amount: self.gas_config.fee_amount().to_string(),
            }],
            gas_limit: self.gas_config.gas_limit,
            payer: String::new(),
            granter: String::new(),
        };

        let auth_info = AuthInfo {
            signer_infos: vec![signer_info],
            fee: Some(fee),
            tip: None,
        };

        // Encode body and auth_info
        let body_bytes = tx_body.encode_to_vec();
        let auth_info_bytes = auth_info.encode_to_vec();

        // Create SignDoc
        let sign_doc = SignDoc {
            body_bytes: body_bytes.clone(),
            auth_info_bytes: auth_info_bytes.clone(),
            chain_id: self.chain_id.clone(),
            account_number: account_info.account_number,
        };

        // Hash and sign
        let sign_doc_bytes = sign_doc.encode_to_vec();
        let sign_doc_hash = Sha256::digest(&sign_doc_bytes);

        let signature = wallet
            .sign_prehashed(&sign_doc_hash)
            .map_err(|e| TxBuilderError::SigningError(e.to_string()))?;

        debug!(
            chain_id = %self.chain_id,
            account_number = account_info.account_number,
            sequence = account_info.sequence,
            "Transaction signed"
        );

        Ok(TxRaw {
            body_bytes,
            auth_info_bytes,
            signatures: vec![signature],
        })
    }

    /// Encode a signed transaction to bytes for broadcast
    pub fn encode_tx(tx: &TxRaw) -> Vec<u8> {
        tx.encode_to_vec()
    }

    /// Encode a signed transaction to base64 for broadcast
    pub fn encode_tx_base64(tx: &TxRaw) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, tx.encode_to_vec())
    }
}

/// Encode a secp256k1 public key for protobuf
fn encode_pubkey(pubkey_bytes: &[u8]) -> Vec<u8> {
    // The cosmos.crypto.secp256k1.PubKey message is just:
    // message PubKey { bytes key = 1; }
    let mut buf = Vec::new();
    // Field 1, wire type 2 (length-delimited)
    buf.push(0x0a);
    // Length of the key
    buf.push(pubkey_bytes.len() as u8);
    // The key bytes
    buf.extend_from_slice(pubkey_bytes);
    buf
}

/// Escrow contract messages
pub mod escrow_msgs {
    use serde::{Deserialize, Serialize};

    /// Lock funds in escrow
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LockMsg {
        /// Settlement ID this escrow is for
        pub settlement_id: String,
        /// Timeout in seconds
        pub timeout_seconds: u64,
    }

    /// Release escrow to recipient
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ReleaseMsg {
        /// Escrow ID to release
        pub escrow_id: String,
        /// Recipient address
        pub recipient: String,
    }

    /// Refund escrow to original owner
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RefundMsg {
        /// Escrow ID to refund
        pub escrow_id: String,
    }

    impl LockMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "lock": {
                    "settlement_id": self.settlement_id,
                    "timeout_seconds": self.timeout_seconds
                }
            })
        }
    }

    impl ReleaseMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "release": {
                    "escrow_id": self.escrow_id,
                    "recipient": self.recipient
                }
            })
        }
    }

    impl RefundMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "refund": {
                    "escrow_id": self.escrow_id
                }
            })
        }
    }
}

/// Settlement contract messages
pub mod settlement_msgs {
    use serde::{Deserialize, Serialize};

    /// Create a new settlement
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CreateSettlementMsg {
        /// Input chain ID
        pub input_chain: String,
        /// Output chain ID
        pub output_chain: String,
        /// Input amount
        pub input_amount: String,
        /// Input denomination
        pub input_denom: String,
        /// Expected output amount
        pub output_amount: String,
        /// Output denomination
        pub output_denom: String,
        /// User address on output chain
        pub user_output_address: String,
        /// Solver ID
        pub solver_id: String,
    }

    /// Execute a settlement (after escrow is locked)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ExecuteSettlementMsg {
        /// Settlement ID
        pub settlement_id: String,
        /// Escrow ID on the input chain
        pub escrow_id: String,
    }

    /// Complete a settlement (after IBC transfer)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CompleteSettlementMsg {
        /// Settlement ID
        pub settlement_id: String,
        /// IBC packet sequence for verification
        pub ibc_sequence: Option<u64>,
    }

    impl CreateSettlementMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "create_settlement": {
                    "input_chain": self.input_chain,
                    "output_chain": self.output_chain,
                    "input_amount": self.input_amount,
                    "input_denom": self.input_denom,
                    "output_amount": self.output_amount,
                    "output_denom": self.output_denom,
                    "user_output_address": self.user_output_address,
                    "solver_id": self.solver_id
                }
            })
        }
    }

    impl ExecuteSettlementMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "execute_settlement": {
                    "settlement_id": self.settlement_id,
                    "escrow_id": self.escrow_id
                }
            })
        }
    }

    impl CompleteSettlementMsg {
        pub fn to_execute_msg(&self) -> serde_json::Value {
            serde_json::json!({
                "complete_settlement": {
                    "settlement_id": self.settlement_id,
                    "ibc_sequence": self.ibc_sequence
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::wallet::CosmosWallet;

    #[test]
    fn test_build_execute_msg() {
        let builder = TxBuilder::new("theta-testnet-001");

        let msg = builder
            .build_execute_msg(
                "cosmos1abc...",
                "cosmos1contract...",
                &serde_json::json!({"lock": {"settlement_id": "123"}}),
                vec![(1000000, "uatom")],
            )
            .unwrap();

        assert_eq!(msg.sender, "cosmos1abc...");
        assert_eq!(msg.contract, "cosmos1contract...");
        assert_eq!(msg.funds.len(), 1);
        assert_eq!(msg.funds[0].amount, "1000000");
    }

    #[test]
    fn test_gas_config_for_chain() {
        let cosmos_gas = GasConfig::for_chain("theta-testnet-001");
        assert_eq!(cosmos_gas.fee_denom, "uatom");

        let osmo_gas = GasConfig::for_chain("osmo-test-5");
        assert_eq!(osmo_gas.fee_denom, "uosmo");

        let neutron_gas = GasConfig::for_chain("pion-1");
        assert_eq!(neutron_gas.fee_denom, "untrn");
    }

    #[test]
    fn test_escrow_messages() {
        let lock = escrow_msgs::LockMsg {
            settlement_id: "settlement_123".to_string(),
            timeout_seconds: 600,
        };

        let msg = lock.to_execute_msg();
        assert!(msg["lock"]["settlement_id"].as_str().unwrap() == "settlement_123");

        let release = escrow_msgs::ReleaseMsg {
            escrow_id: "escrow_456".to_string(),
            recipient: "cosmos1recipient...".to_string(),
        };

        let msg = release.to_execute_msg();
        assert!(msg["release"]["escrow_id"].as_str().unwrap() == "escrow_456");
    }

    #[test]
    fn test_build_and_sign_tx() {
        let wallet = CosmosWallet::generate("cosmos");
        let builder = TxBuilder::new("theta-testnet-001")
            .with_memo("test transaction");

        let account_info = AccountInfo {
            account_number: 12345,
            sequence: 0,
        };

        let msg = builder
            .build_execute_msg(
                &wallet.address().unwrap(),
                "cosmos1contract...",
                &serde_json::json!({"test": {}}),
                vec![],
            )
            .unwrap();

        let tx = builder.build_and_sign(&wallet, &account_info, msg).unwrap();

        // Verify the transaction is properly formed
        assert!(!tx.body_bytes.is_empty());
        assert!(!tx.auth_info_bytes.is_empty());
        assert_eq!(tx.signatures.len(), 1);
        assert_eq!(tx.signatures[0].len(), 64); // secp256k1 signature
    }
}
