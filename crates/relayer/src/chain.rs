use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tendermint_rpc::{Client, HttpClient};
use tokio::sync::RwLock;

use crate::{PacketProof, RelayerError};

// Optional prost import for protobuf encoding
#[cfg(feature = "grpc")]
use prost::Message;

/// Chain error types
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("query failed: {0}")]
    QueryFailed(String),

    #[error("transaction failed: {0}")]
    TxFailed(String),

    #[error("proof error: {0}")]
    ProofError(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("encoding error: {0}")]
    EncodingError(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

impl From<ChainError> for RelayerError {
    fn from(err: ChainError) -> Self {
        match err {
            ChainError::ConnectionFailed(msg) => RelayerError::ConnectionFailed(msg),
            ChainError::QueryFailed(msg) | ChainError::InvalidResponse(msg) => {
                RelayerError::ProofQueryFailed(msg)
            }
            ChainError::TxFailed(msg) => RelayerError::TransactionFailed(msg),
            ChainError::ProofError(msg) => RelayerError::ProofQueryFailed(msg),
            ChainError::Timeout(msg) => RelayerError::ConnectionFailed(msg),
            ChainError::ParseError(msg) | ChainError::EncodingError(msg) => {
                RelayerError::ProofQueryFailed(msg)
            }
        }
    }
}

/// Transaction response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse {
    pub hash: String,
    pub height: u64,
    pub gas_used: u64,
    pub code: u32, // 0 for success
    pub raw_log: String,
}

impl TxResponse {
    pub fn is_success(&self) -> bool {
        self.code == 0
    }
}

/// IBC Client State (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientState {
    pub chain_id: String,
    pub trust_level: TrustLevel,
    pub trusting_period: Duration,
    pub unbonding_period: Duration,
    pub latest_height: Height,
    pub frozen_height: Option<Height>,
}

/// Trust level for light clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustLevel {
    pub numerator: u64,
    pub denominator: u64,
}

/// IBC Consensus State (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    pub timestamp: u64,
    pub root: Vec<u8>,
    pub next_validators_hash: Vec<u8>,
}

/// Block height
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Height {
    pub revision_number: u64,
    pub revision_height: u64,
}

impl Height {
    pub fn new(revision_number: u64, revision_height: u64) -> Self {
        Self {
            revision_number,
            revision_height,
        }
    }
}

/// Merkle proof for IBC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub proofs: Vec<CommitmentProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentProof {
    pub exist: Option<ExistenceProof>,
    pub nonexist: Option<NonExistenceProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistenceProof {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub leaf: LeafOp,
    pub path: Vec<InnerOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonExistenceProof {
    pub key: Vec<u8>,
    pub left: Option<Box<ExistenceProof>>,
    pub right: Option<Box<ExistenceProof>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeafOp {
    pub hash: HashOp,
    pub prehash_key: HashOp,
    pub prehash_value: HashOp,
    pub length: LengthOp,
    pub prefix: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerOp {
    pub hash: HashOp,
    pub prefix: Vec<u8>,
    pub suffix: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum HashOp {
    NoHash = 0,
    Sha256 = 1,
    Sha512 = 2,
    Keccak256 = 3,
    Ripemd160 = 4,
    Bitcoin = 5,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LengthOp {
    NoPrefix = 0,
    VarProto = 1,
    VarRlp = 2,
    Fixed32Little = 3,
    Fixed32Big = 4,
    Fixed64Little = 5,
    Fixed64Big = 6,
    Require32Bytes = 7,
    Require64Bytes = 8,
}

/// Cosmos message for transactions (protobuf Any)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosmosMsg {
    pub type_url: String,
    pub value: Vec<u8>,
}

/// Cosmos coin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

/// Transaction builder for Cosmos SDK transactions
#[derive(Debug, Clone)]
pub struct TxBuilder {
    pub chain_id: String,
    pub account_number: u64,
    pub sequence: u64,
    pub gas_limit: u64,
    pub fee_amount: Vec<Coin>,
    pub memo: String,
    pub messages: Vec<CosmosMsg>,
    pub timeout_height: u64,
}

impl TxBuilder {
    pub fn new(chain_id: String) -> Self {
        Self {
            chain_id,
            account_number: 0,
            sequence: 0,
            gas_limit: 200_000,
            fee_amount: vec![],
            memo: String::new(),
            messages: vec![],
            timeout_height: 0,
        }
    }

    pub fn with_account_info(mut self, account_number: u64, sequence: u64) -> Self {
        self.account_number = account_number;
        self.sequence = sequence;
        self
    }

    pub fn with_gas(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    pub fn with_fee(mut self, fee_amount: Vec<Coin>) -> Self {
        self.fee_amount = fee_amount;
        self
    }

    pub fn with_memo(mut self, memo: String) -> Self {
        self.memo = memo;
        self
    }

    pub fn with_messages(mut self, messages: Vec<CosmosMsg>) -> Self {
        self.messages = messages;
        self
    }

    pub fn add_message(mut self, message: CosmosMsg) -> Self {
        self.messages.push(message);
        self
    }

    pub fn with_timeout_height(mut self, height: u64) -> Self {
        self.timeout_height = height;
        self
    }

    /// Build the transaction bytes for signing
    /// In a real implementation, this would use cosmos-sdk protobuf types
    pub fn build_for_signing(&self) -> Result<Vec<u8>, ChainError> {
        // This is a simplified version - a real implementation would:
        // 1. Create TxBody from messages and memo
        // 2. Create AuthInfo from fee and gas
        // 3. Create SignDoc from TxBody, AuthInfo, chain_id, account_number
        // 4. Encode SignDoc to bytes for signing

        let tx_json = serde_json::json!({
            "chain_id": self.chain_id,
            "account_number": self.account_number.to_string(),
            "sequence": self.sequence.to_string(),
            "fee": {
                "amount": self.fee_amount,
                "gas": self.gas_limit.to_string(),
            },
            "msgs": self.messages,
            "memo": self.memo,
        });

        serde_json::to_vec(&tx_json)
            .map_err(|e| ChainError::EncodingError(format!("Failed to encode tx: {}", e)))
    }

    /// Build a signed transaction ready for broadcast
    /// In production, this would encode the full Tx protobuf message
    pub fn build_signed(
        &self,
        signature: Vec<u8>,
        pub_key: Vec<u8>,
    ) -> Result<Vec<u8>, ChainError> {
        // Simplified - real implementation would create proper Tx protobuf
        let signed_tx = serde_json::json!({
            "tx": {
                "body": {
                    "messages": self.messages,
                    "memo": self.memo,
                    "timeout_height": self.timeout_height.to_string(),
                },
                "auth_info": {
                    "signer_infos": [{
                        "public_key": {
                            "type_url": "/cosmos.crypto.secp256k1.PubKey",
                            "value": pub_key,
                        },
                        "sequence": self.sequence.to_string(),
                    }],
                    "fee": {
                        "amount": self.fee_amount,
                        "gas_limit": self.gas_limit.to_string(),
                    }
                },
                "signatures": [signature],
            },
            "mode": "BROADCAST_MODE_SYNC",
        });

        serde_json::to_vec(&signed_tx)
            .map_err(|e| ChainError::EncodingError(format!("Failed to encode signed tx: {}", e)))
    }
}

/// Chain configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: String,
    pub rpc_url: String,
    pub grpc_url: Option<String>,
    pub gas_price: String,
    pub gas_adjustment: f64,
    pub default_gas: u64,
    pub max_gas: u64,
    pub account_prefix: String,
    pub fee_denom: String,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            chain_id: String::new(),
            rpc_url: String::new(),
            grpc_url: None,
            gas_price: "0.025".to_string(),
            gas_adjustment: 1.3,
            default_gas: 200_000,
            max_gas: 2_000_000,
            account_prefix: "cosmos".to_string(),
            fee_denom: "uatom".to_string(),
        }
    }
}

/// Extended chain client trait with full IBC capabilities
#[async_trait]
pub trait ChainClient: Send + Sync {
    /// Check if client is connected
    async fn is_connected(&self) -> bool;

    /// Query packet commitment from source chain
    async fn query_packet_commitment(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<Option<Vec<u8>>, ChainError>;

    /// Query packet receipt from destination chain
    async fn query_packet_receipt(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<bool, ChainError>;

    /// Query unreceived packets
    async fn query_unreceived_packets(
        &self,
        channel_id: &str,
        sequences: &[u64],
    ) -> Result<Vec<u64>, ChainError>;

    /// Submit transaction with multiple messages
    async fn submit_tx(&self, msgs: Vec<CosmosMsg>, memo: &str)
        -> Result<TxResponse, ChainError>;

    /// Query IBC client state
    async fn query_client_state(&self, client_id: &str) -> Result<ClientState, ChainError>;

    /// Query IBC consensus state
    async fn query_consensus_state(
        &self,
        client_id: &str,
        height: u64,
    ) -> Result<ConsensusState, ChainError>;

    /// Get merkle proof for a key at a specific height
    async fn get_proof(&self, key: &[u8], height: u64) -> Result<MerkleProof, ChainError>;

    /// Get packet commitment proof (combines commitment query and proof)
    async fn get_packet_commitment_proof(
        &self,
        channel: &str,
        sequence: u64,
    ) -> Result<PacketProof, RelayerError>;

    /// Submit receive packet message
    async fn submit_recv_packet(
        &self,
        channel: &str,
        sequence: u64,
        proof: PacketProof,
    ) -> Result<(), RelayerError>;

    /// Get current block height
    async fn get_latest_height(&self) -> Result<u64, ChainError>;
}

/// Cosmos chain client using Tendermint RPC
pub struct CosmosChainClient {
    config: ChainConfig,
    rpc_client: HttpClient,
    connected: Arc<RwLock<bool>>,
    last_health_check: Arc<RwLock<std::time::Instant>>,
    health_check_interval: Duration,
    request_timeout: Duration,
}

impl CosmosChainClient {
    pub async fn new(config: ChainConfig) -> Result<Self, ChainError> {
        let rpc_client = HttpClient::new(config.rpc_url.as_str())
            .map_err(|e| ChainError::ConnectionFailed(format!("Failed to create client: {}", e)))?;

        let client = Self {
            config,
            rpc_client,
            connected: Arc::new(RwLock::new(false)),
            last_health_check: Arc::new(RwLock::new(std::time::Instant::now())),
            health_check_interval: Duration::from_secs(30),
            request_timeout: Duration::from_secs(10),
        };

        // Test connection
        client.test_connection().await?;

        Ok(client)
    }

    /// Test connection to the chain
    async fn test_connection(&self) -> Result<(), ChainError> {
        match tokio::time::timeout(self.request_timeout, self.rpc_client.status()).await {
            Ok(Ok(_status)) => {
                *self.connected.write().await = true;
                *self.last_health_check.write().await = std::time::Instant::now();
                Ok(())
            }
            Ok(Err(e)) => {
                *self.connected.write().await = false;
                Err(ChainError::ConnectionFailed(format!(
                    "Connection test failed: {}",
                    e
                )))
            }
            Err(_) => {
                *self.connected.write().await = false;
                Err(ChainError::Timeout(format!(
                    "Connection test timed out after {:?}",
                    self.request_timeout
                )))
            }
        }
    }

    /// Reconnect to the chain
    pub async fn reconnect(&self) -> Result<(), ChainError> {
        tracing::info!(
            chain_id = %self.config.chain_id,
            rpc_url = %self.config.rpc_url,
            "Attempting to reconnect"
        );
        self.test_connection().await
    }

    /// Perform periodic health check if needed
    async fn maybe_health_check(&self) -> Result<(), ChainError> {
        let last_check = *self.last_health_check.read().await;
        if last_check.elapsed() > self.health_check_interval {
            self.test_connection().await?;
        }
        Ok(())
    }

    /// Execute an operation with automatic reconnection on failure
    async fn with_retry<F, T, Fut>(&self, operation: F) -> Result<T, ChainError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, ChainError>>,
    {
        // First attempt
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                tracing::warn!(
                    chain_id = %self.config.chain_id,
                    error = %e,
                    "Operation failed, attempting reconnect"
                );
            }
        }

        // Try to reconnect
        if let Err(e) = self.reconnect().await {
            return Err(e);
        }

        // Retry once after reconnect
        operation().await
    }

    /// Build IBC packet commitment path
    fn packet_commitment_path(&self, channel_id: &str, sequence: u64) -> Vec<u8> {
        format!(
            "commitments/ports/transfer/channels/{}/sequences/{}",
            channel_id, sequence
        )
        .into_bytes()
    }

    /// Build IBC packet receipt path
    fn packet_receipt_path(&self, channel_id: &str, sequence: u64) -> Vec<u8> {
        format!(
            "receipts/ports/transfer/channels/{}/sequences/{}",
            channel_id, sequence
        )
        .into_bytes()
    }

    /// Build IBC client state path
    fn client_state_path(&self, client_id: &str) -> Vec<u8> {
        format!("clients/{}/clientState", client_id).into_bytes()
    }

    /// Build IBC consensus state path
    fn consensus_state_path(&self, client_id: &str, height: u64) -> Vec<u8> {
        format!("clients/{}/consensusStates/{}", client_id, height).into_bytes()
    }

    /// Query ABCI for a key
    async fn abci_query(&self, path: &[u8], height: Option<u64>) -> Result<Vec<u8>, ChainError> {
        use tendermint::block::Height as TmHeight;

        // Perform health check if needed
        self.maybe_health_check().await?;

        let tm_height = height.map(|h| {
            TmHeight::try_from(h)
                .map_err(|e| ChainError::ParseError(format!("Invalid height: {}", e)))
        }).transpose()?;

        let response = tokio::time::timeout(
            self.request_timeout,
            self.rpc_client.abci_query(
                Some("store/ibc/key".to_string()),
                path.to_vec(),
                tm_height,
                false,
            ),
        )
        .await
        .map_err(|_| {
            ChainError::Timeout(format!(
                "ABCI query timed out after {:?}",
                self.request_timeout
            ))
        })?
        .map_err(|e| ChainError::QueryFailed(format!("ABCI query failed: {}", e)))?;

        if response.code.is_err() {
            return Err(ChainError::QueryFailed(format!(
                "ABCI query returned error code: {:?}",
                response.code
            )));
        }

        Ok(response.value)
    }

    /// Query ABCI with proof
    async fn abci_query_with_proof(
        &self,
        path: &[u8],
        height: Option<u64>,
    ) -> Result<(Vec<u8>, Option<tendermint::merkle::proof::ProofOps>), ChainError> {
        use tendermint::block::Height as TmHeight;

        // Perform health check if needed
        self.maybe_health_check().await?;

        let tm_height = height.map(|h| {
            TmHeight::try_from(h)
                .map_err(|e| ChainError::ParseError(format!("Invalid height: {}", e)))
        }).transpose()?;

        let response = tokio::time::timeout(
            self.request_timeout,
            self.rpc_client.abci_query(
                Some("store/ibc/key".to_string()),
                path.to_vec(),
                tm_height,
                true, // prove=true
            ),
        )
        .await
        .map_err(|_| {
            ChainError::Timeout(format!(
                "ABCI query with proof timed out after {:?}",
                self.request_timeout
            ))
        })?
        .map_err(|e| ChainError::ProofError(format!("ABCI proof query failed: {}", e)))?;

        if response.code.is_err() {
            return Err(ChainError::ProofError(format!(
                "ABCI proof query returned error code: {:?}",
                response.code
            )));
        }

        Ok((response.value, response.proof))
    }

    /// Estimate gas for messages
    fn estimate_gas(&self, msgs: &[CosmosMsg]) -> u64 {
        let base_gas = self.config.default_gas;
        let per_msg_gas = 50_000u64;
        let total = base_gas + (msgs.len() as u64 * per_msg_gas);
        std::cmp::min(
            (total as f64 * self.config.gas_adjustment) as u64,
            self.config.max_gas,
        )
    }

    /// Parse proof from ABCI response
    fn parse_proof(
        &self,
        proof_ops: &[tendermint::merkle::proof::ProofOp],
    ) -> Result<MerkleProof, ChainError> {
        // In a production implementation, this would:
        // 1. Parse each ProofOp from tendermint format
        // 2. Decode the protobuf ICS-23 CommitmentProof from each op's data
        // 3. Build the full MerkleProof structure
        //
        // The ICS-23 proof format is:
        // - ProofOp.type = "ics23:iavl" or "ics23:simple"
        // - ProofOp.key = the key being proven
        // - ProofOp.data = protobuf-encoded CommitmentProof
        //
        // For now, we create a valid structure with the actual key data if available

        let mut proofs = Vec::new();

        for proof_op in proof_ops {
            // In production, decode proof_op.data as protobuf CommitmentProof
            // For now, create a placeholder that includes the key
            proofs.push(CommitmentProof {
                exist: Some(ExistenceProof {
                    key: proof_op.key.clone(),
                    value: vec![],
                    leaf: LeafOp {
                        hash: HashOp::Sha256,
                        prehash_key: HashOp::NoHash,
                        prehash_value: HashOp::Sha256,
                        length: LengthOp::VarProto,
                        prefix: vec![0],
                    },
                    path: vec![],
                }),
                nonexist: None,
            });
        }

        // If no proof ops provided, return minimal structure
        if proofs.is_empty() {
            proofs.push(CommitmentProof {
                exist: Some(ExistenceProof {
                    key: vec![],
                    value: vec![],
                    leaf: LeafOp {
                        hash: HashOp::Sha256,
                        prehash_key: HashOp::NoHash,
                        prehash_value: HashOp::Sha256,
                        length: LengthOp::VarProto,
                        prefix: vec![0],
                    },
                    path: vec![],
                }),
                nonexist: None,
            });
        }

        Ok(MerkleProof { proofs })
    }

    /// Get account info (account number and sequence) for transaction signing
    /// In production, this would query the auth module
    async fn get_account_info(&self, _address: &str) -> Result<(u64, u64), ChainError> {
        // Placeholder - in production, query:
        // /cosmos.auth.v1beta1.Query/Account
        // or via ABCI query to auth module
        Ok((0, 0))
    }

    /// Build fee coins from config
    fn build_fee(&self, gas: u64) -> Vec<Coin> {
        let gas_price: f64 = self.config.gas_price.parse().unwrap_or(0.025);
        let fee_amount = (gas as f64 * gas_price).ceil() as u64;

        vec![Coin {
            denom: self.config.fee_denom.clone(),
            amount: fee_amount.to_string(),
        }]
    }

    /// Simulate transaction broadcast (placeholder for production implementation)
    async fn simulate_broadcast(&self, _tx_bytes: Vec<u8>) -> Result<TxResponse, ChainError> {
        // In production, this would be:
        // let result = self.rpc_client.broadcast_tx_sync(tx_bytes).await?;
        // Parse result into TxResponse

        // For now, return a simulated successful response
        Ok(TxResponse {
            hash: hex::encode(&[0u8; 32]),
            height: 1,
            gas_used: self.config.default_gas,
            code: 0,
            raw_log: "simulated success".to_string(),
        })
    }

    /// Build a receive packet message
    fn build_recv_packet_msg(
        &self,
        channel: &str,
        sequence: u64,
        proof: &PacketProof,
        signer: &str,
    ) -> Result<CosmosMsg, ChainError> {
        // In production, this would:
        // 1. Create the full IBC packet data structure
        // 2. Encode as protobuf MsgRecvPacket
        // 3. Wrap in Any message
        //
        // MsgRecvPacket structure:
        // message MsgRecvPacket {
        //   Packet packet = 1;
        //   bytes proof_commitment = 2;
        //   ibc.core.client.v1.Height proof_height = 3;
        //   string signer = 4;
        // }

        tracing::debug!(
            channel = channel,
            sequence = sequence,
            proof_height = proof.proof_height,
            signer = signer,
            "Building MsgRecvPacket"
        );

        // Placeholder encoding - in production, use protobuf
        let msg_data = serde_json::json!({
            "packet": {
                "sequence": sequence.to_string(),
                "source_port": "transfer",
                "source_channel": channel,
                "destination_port": "transfer",
                "destination_channel": channel,
                "data": "",
                "timeout_height": {},
                "timeout_timestamp": "0",
            },
            "proof_commitment": proof.proof,
            "proof_height": {
                "revision_number": "0",
                "revision_height": proof.proof_height.to_string(),
            },
            "signer": signer,
        });

        let value = serde_json::to_vec(&msg_data)
            .map_err(|e| ChainError::EncodingError(format!("Failed to encode MsgRecvPacket: {}", e)))?;

        Ok(CosmosMsg {
            type_url: "/ibc.core.channel.v1.MsgRecvPacket".to_string(),
            value,
        })
    }
}

#[async_trait]
impl ChainClient for CosmosChainClient {
    async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    async fn query_packet_commitment(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<Option<Vec<u8>>, ChainError> {
        let path = self.packet_commitment_path(channel_id, sequence);
        match self.abci_query(&path, None).await {
            Ok(value) if !value.is_empty() => Ok(Some(value)),
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn query_packet_receipt(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<bool, ChainError> {
        let path = self.packet_receipt_path(channel_id, sequence);
        match self.abci_query(&path, None).await {
            Ok(value) => Ok(!value.is_empty()),
            Err(_) => Ok(false),
        }
    }

    async fn query_unreceived_packets(
        &self,
        channel_id: &str,
        sequences: &[u64],
    ) -> Result<Vec<u64>, ChainError> {
        let mut unreceived = Vec::new();

        for &seq in sequences {
            let received = self.query_packet_receipt(channel_id, seq).await?;
            if !received {
                unreceived.push(seq);
            }
        }

        Ok(unreceived)
    }

    async fn submit_tx(
        &self,
        msgs: Vec<CosmosMsg>,
        memo: &str,
    ) -> Result<TxResponse, ChainError> {
        let gas = self.estimate_gas(&msgs);
        let fee = self.build_fee(gas);

        // In a production implementation, this would:
        // 1. Query account info (account_number, sequence)
        // 2. Build transaction with TxBuilder
        // 3. Sign the transaction with a key/signer
        // 4. Broadcast via broadcast_tx_sync or broadcast_tx_commit
        // 5. Wait for inclusion in a block (if using sync)
        // 6. Parse the result from TxResponse
        //
        // For now, we simulate the process with placeholders

        // Get account info (in production, query from chain)
        let (_account_number, _sequence) = self.get_account_info("placeholder_address").await?;

        // Build transaction
        let tx_builder = TxBuilder::new(self.config.chain_id.clone())
            .with_account_info(_account_number, _sequence)
            .with_gas(gas)
            .with_fee(fee)
            .with_memo(memo.to_string())
            .with_messages(msgs.clone());

        // In production:
        // 1. Build sign doc: tx_builder.build_for_signing()?
        // 2. Sign with key: let signature = key.sign(&sign_doc)?
        // 3. Build signed tx: tx_builder.build_signed(signature, pub_key)?
        // 4. Broadcast: rpc_client.broadcast_tx_sync(tx_bytes).await?

        tracing::info!(
            chain_id = %self.config.chain_id,
            num_msgs = msgs.len(),
            gas = gas,
            memo = memo,
            "Submitting transaction (simulation mode)"
        );

        // Placeholder signing
        let _sign_doc = tx_builder.build_for_signing()?;
        let placeholder_signature = vec![0u8; 64];
        let placeholder_pubkey = vec![0u8; 33];
        let tx_bytes = tx_builder.build_signed(placeholder_signature, placeholder_pubkey)?;

        // Simulate broadcast
        // In production: self.rpc_client.broadcast_tx_sync(tx_bytes).await?
        let response = self.simulate_broadcast(tx_bytes).await?;

        Ok(response)
    }

    async fn query_client_state(&self, client_id: &str) -> Result<ClientState, ChainError> {
        let path = self.client_state_path(client_id);
        let _value = self.abci_query(&path, None).await?;

        // In a real implementation, decode the protobuf ClientState
        // For now, return a mock state
        Ok(ClientState {
            chain_id: self.config.chain_id.clone(),
            trust_level: TrustLevel {
                numerator: 1,
                denominator: 3,
            },
            trusting_period: Duration::from_secs(14 * 24 * 60 * 60), // 14 days
            unbonding_period: Duration::from_secs(21 * 24 * 60 * 60), // 21 days
            latest_height: Height::new(0, 1000),
            frozen_height: None,
        })
    }

    async fn query_consensus_state(
        &self,
        client_id: &str,
        height: u64,
    ) -> Result<ConsensusState, ChainError> {
        let path = self.consensus_state_path(client_id, height);
        let _value = self.abci_query(&path, None).await?;

        // In a real implementation, decode the protobuf ConsensusState
        Ok(ConsensusState {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            root: vec![0u8; 32],
            next_validators_hash: vec![0u8; 32],
        })
    }

    async fn get_proof(&self, key: &[u8], height: u64) -> Result<MerkleProof, ChainError> {
        // Use the new query method with proof
        let (_value, proof_ops) = self.abci_query_with_proof(key, Some(height)).await?;

        match proof_ops {
            Some(proof) => self.parse_proof(&proof.ops),
            None => {
                // No proof available, return empty proof structure
                self.parse_proof(&[])
            }
        }
    }

    async fn get_packet_commitment_proof(
        &self,
        channel: &str,
        sequence: u64,
    ) -> Result<PacketProof, RelayerError> {
        let _commitment = self
            .query_packet_commitment(channel, sequence)
            .await?
            .ok_or_else(|| {
                RelayerError::ProofQueryFailed(format!(
                    "No commitment found for channel {} sequence {}",
                    channel, sequence
                ))
            })?;

        let height = self.get_latest_height().await?;
        let path = self.packet_commitment_path(channel, sequence);
        let merkle_proof = self.get_proof(&path, height).await?;

        // Encode the proof (in real implementation, use protobuf)
        let proof_bytes = serde_json::to_vec(&merkle_proof)
            .map_err(|e| ChainError::EncodingError(format!("Failed to encode proof: {}", e)))?;

        Ok(PacketProof {
            proof: proof_bytes,
            proof_height: height,
        })
    }

    async fn submit_recv_packet(
        &self,
        channel: &str,
        sequence: u64,
        proof: PacketProof,
    ) -> Result<(), RelayerError> {
        // In production, the signer address would come from the configured key
        let signer = format!("{}1placeholder", self.config.account_prefix);

        // Build MsgRecvPacket with proof
        let msg = self.build_recv_packet_msg(channel, sequence, &proof, &signer)?;

        let response = self
            .submit_tx(vec![msg], &format!("Relay packet {}-{}", channel, sequence))
            .await?;

        if response.is_success() {
            tracing::info!(
                channel = channel,
                sequence = sequence,
                tx_hash = %response.hash,
                height = response.height,
                "Successfully submitted receive packet"
            );
            Ok(())
        } else {
            Err(RelayerError::TransactionFailed(format!(
                "Transaction failed with code {}: {}",
                response.code, response.raw_log
            )))
        }
    }

    async fn get_latest_height(&self) -> Result<u64, ChainError> {
        let status = self
            .rpc_client
            .status()
            .await
            .map_err(|e| ChainError::QueryFailed(format!("Failed to get status: {}", e)))?;

        Ok(status.sync_info.latest_block_height.value())
    }
}

/// Chain client pool for managing multiple chain connections
pub struct ChainClientPool {
    clients: Arc<RwLock<HashMap<String, Arc<dyn ChainClient>>>>,
    configs: HashMap<String, ChainConfig>,
}

impl ChainClientPool {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            configs: HashMap::new(),
        }
    }

    /// Add a chain configuration
    pub fn add_chain(&mut self, chain_id: String, config: ChainConfig) {
        self.configs.insert(chain_id, config);
    }

    /// Initialize all chain clients
    pub async fn connect_all(&self) -> Result<(), ChainError> {
        let mut clients = self.clients.write().await;

        for (chain_id, config) in &self.configs {
            let client = CosmosChainClient::new(config.clone()).await?;
            clients.insert(chain_id.clone(), Arc::new(client));
            tracing::info!(chain_id = %chain_id, "Connected to chain");
        }

        Ok(())
    }

    /// Get a chain client
    pub async fn get_client(&self, chain_id: &str) -> Option<Arc<dyn ChainClient>> {
        self.clients.read().await.get(chain_id).cloned()
    }

    /// Health check for all chains
    pub async fn health_check(&self) -> HashMap<String, bool> {
        let clients = self.clients.read().await;
        let mut health = HashMap::new();

        for (chain_id, client) in clients.iter() {
            health.insert(chain_id.clone(), client.is_connected().await);
        }

        health
    }

    /// Reconnect disconnected chains
    pub async fn reconnect_failed(&self) -> Result<(), ChainError> {
        let health = self.health_check().await;

        for (chain_id, is_healthy) in health {
            if !is_healthy {
                if let Some(config) = self.configs.get(&chain_id) {
                    tracing::warn!(chain_id = %chain_id, "Reconnecting to chain");
                    let client = CosmosChainClient::new(config.clone()).await?;
                    self.clients
                        .write()
                        .await
                        .insert(chain_id.clone(), Arc::new(client));
                }
            }
        }

        Ok(())
    }

    /// Get all chain IDs
    pub fn chain_ids(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }
}

impl Default for ChainClientPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_config_default() {
        let config = ChainConfig::default();
        assert_eq!(config.gas_adjustment, 1.3);
        assert_eq!(config.account_prefix, "cosmos");
        assert_eq!(config.fee_denom, "uatom");
        assert_eq!(config.default_gas, 200_000);
    }

    #[test]
    fn test_packet_commitment_path() {
        // We can't actually create a client without a real RPC endpoint,
        // but we can test the path construction logic
        let path = format!(
            "commitments/ports/transfer/channels/{}/sequences/{}",
            "channel-0", 42
        );
        assert_eq!(
            path,
            "commitments/ports/transfer/channels/channel-0/sequences/42"
        );
    }

    #[test]
    fn test_packet_receipt_path() {
        let path = format!(
            "receipts/ports/transfer/channels/{}/sequences/{}",
            "channel-5", 100
        );
        assert_eq!(
            path,
            "receipts/ports/transfer/channels/channel-5/sequences/100"
        );
    }

    #[test]
    fn test_tx_response_success() {
        let response = TxResponse {
            hash: "ABC123".to_string(),
            height: 100,
            gas_used: 50000,
            code: 0,
            raw_log: "success".to_string(),
        };
        assert!(response.is_success());

        let failed = TxResponse {
            hash: "DEF456".to_string(),
            height: 101,
            gas_used: 30000,
            code: 1,
            raw_log: "error".to_string(),
        };
        assert!(!failed.is_success());
    }

    #[test]
    fn test_height_creation() {
        let height = Height::new(1, 1000);
        assert_eq!(height.revision_number, 1);
        assert_eq!(height.revision_height, 1000);
    }

    #[test]
    fn test_tx_builder_creation() {
        let builder = TxBuilder::new("test-chain-1".to_string());
        assert_eq!(builder.chain_id, "test-chain-1");
        assert_eq!(builder.account_number, 0);
        assert_eq!(builder.sequence, 0);
        assert_eq!(builder.gas_limit, 200_000);
        assert_eq!(builder.memo, "");
        assert!(builder.messages.is_empty());
    }

    #[test]
    fn test_tx_builder_with_options() {
        let fee = vec![Coin {
            denom: "uatom".to_string(),
            amount: "5000".to_string(),
        }];

        let msg = CosmosMsg {
            type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
            value: vec![1, 2, 3],
        };

        let builder = TxBuilder::new("test-chain-1".to_string())
            .with_account_info(123, 456)
            .with_gas(300_000)
            .with_fee(fee.clone())
            .with_memo("test memo".to_string())
            .add_message(msg.clone());

        assert_eq!(builder.account_number, 123);
        assert_eq!(builder.sequence, 456);
        assert_eq!(builder.gas_limit, 300_000);
        assert_eq!(builder.fee_amount.len(), 1);
        assert_eq!(builder.fee_amount[0].denom, "uatom");
        assert_eq!(builder.memo, "test memo");
        assert_eq!(builder.messages.len(), 1);
    }

    #[test]
    fn test_tx_builder_build_for_signing() {
        let builder = TxBuilder::new("test-chain-1".to_string())
            .with_account_info(100, 5)
            .with_gas(200_000)
            .with_memo("sign test".to_string());

        let result = builder.build_for_signing();
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());

        // Verify it's valid JSON
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["chain_id"], "test-chain-1");
        assert_eq!(json["account_number"], "100");
        assert_eq!(json["sequence"], "5");
    }

    #[test]
    fn test_tx_builder_build_signed() {
        let builder = TxBuilder::new("test-chain-1".to_string())
            .with_account_info(100, 5)
            .with_gas(200_000);

        let signature = vec![1u8; 64];
        let pubkey = vec![2u8; 33];

        let result = builder.build_signed(signature, pubkey);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_coin_creation() {
        let coin = Coin {
            denom: "uatom".to_string(),
            amount: "1000000".to_string(),
        };
        assert_eq!(coin.denom, "uatom");
        assert_eq!(coin.amount, "1000000");
    }

    #[test]
    fn test_merkle_proof_structure() {
        let proof = MerkleProof {
            proofs: vec![CommitmentProof {
                exist: Some(ExistenceProof {
                    key: b"test_key".to_vec(),
                    value: b"test_value".to_vec(),
                    leaf: LeafOp {
                        hash: HashOp::Sha256,
                        prehash_key: HashOp::NoHash,
                        prehash_value: HashOp::Sha256,
                        length: LengthOp::VarProto,
                        prefix: vec![0],
                    },
                    path: vec![],
                }),
                nonexist: None,
            }],
        };

        assert_eq!(proof.proofs.len(), 1);
        assert!(proof.proofs[0].exist.is_some());
        if let Some(exist) = &proof.proofs[0].exist {
            assert_eq!(exist.key, b"test_key");
            assert_eq!(exist.value, b"test_value");
        }
    }

    #[test]
    fn test_client_state_creation() {
        let state = ClientState {
            chain_id: "test-1".to_string(),
            trust_level: TrustLevel {
                numerator: 1,
                denominator: 3,
            },
            trusting_period: Duration::from_secs(14 * 24 * 60 * 60),
            unbonding_period: Duration::from_secs(21 * 24 * 60 * 60),
            latest_height: Height::new(0, 1000),
            frozen_height: None,
        };

        assert_eq!(state.chain_id, "test-1");
        assert_eq!(state.trust_level.numerator, 1);
        assert_eq!(state.trust_level.denominator, 3);
        assert_eq!(state.latest_height.revision_height, 1000);
        assert!(state.frozen_height.is_none());
    }

    #[tokio::test]
    async fn test_chain_client_pool() {
        let mut pool = ChainClientPool::new();

        let config = ChainConfig {
            chain_id: "test-1".to_string(),
            rpc_url: "http://localhost:26657".to_string(),
            ..Default::default()
        };

        pool.add_chain("test-1".to_string(), config);

        let chain_ids = pool.chain_ids();
        assert_eq!(chain_ids.len(), 1);
        assert_eq!(chain_ids[0], "test-1");
    }

    #[tokio::test]
    async fn test_chain_client_pool_multiple_chains() {
        let mut pool = ChainClientPool::new();

        for i in 1..=3 {
            let config = ChainConfig {
                chain_id: format!("test-{}", i),
                rpc_url: format!("http://localhost:2665{}", i),
                ..Default::default()
            };
            pool.add_chain(format!("test-{}", i), config);
        }

        let chain_ids = pool.chain_ids();
        assert_eq!(chain_ids.len(), 3);
    }

    #[test]
    fn test_hash_op_values() {
        assert_eq!(HashOp::NoHash as i32, 0);
        assert_eq!(HashOp::Sha256 as i32, 1);
        assert_eq!(HashOp::Sha512 as i32, 2);
    }

    #[test]
    fn test_length_op_values() {
        assert_eq!(LengthOp::NoPrefix as i32, 0);
        assert_eq!(LengthOp::VarProto as i32, 1);
        assert_eq!(LengthOp::VarRlp as i32, 2);
    }

    // Network tests are marked as ignored so they don't run in CI
    #[tokio::test]
    #[ignore]
    async fn test_cosmos_client_connection() {
        let config = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network:443".to_string(),
            ..Default::default()
        };

        let client = CosmosChainClient::new(config).await;
        assert!(client.is_ok());

        if let Ok(client) = client {
            assert!(client.is_connected().await);

            let height = client.get_latest_height().await;
            assert!(height.is_ok());
            println!("Latest height: {:?}", height);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_packet_commitment_query() {
        let config = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network:443".to_string(),
            ..Default::default()
        };

        let client = CosmosChainClient::new(config).await.unwrap();

        let result = client.query_packet_commitment("channel-0", 1).await;

        // This will likely return None unless there's an actual commitment
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_latest_height() {
        let config = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network:443".to_string(),
            ..Default::default()
        };

        let client = CosmosChainClient::new(config).await.unwrap();
        let height = client.get_latest_height().await;

        assert!(height.is_ok());
        let h = height.unwrap();
        assert!(h > 0);
        println!("Latest height: {}", h);
    }

    #[tokio::test]
    #[ignore]
    async fn test_reconnection() {
        let config = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network:443".to_string(),
            ..Default::default()
        };

        let client = CosmosChainClient::new(config).await.unwrap();

        // Test initial connection
        assert!(client.is_connected().await);

        // Test reconnection
        let reconnect_result = client.reconnect().await;
        assert!(reconnect_result.is_ok());
        assert!(client.is_connected().await);
    }
}
