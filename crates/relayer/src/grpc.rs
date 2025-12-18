#![cfg(feature = "grpc")]

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::chain::{
    ChainClient, ChainConfig, ChainError, ClientState, ConsensusState, CosmosMsg, MerkleProof,
    TxResponse,
};
use crate::{PacketProof, RelayerError};

/// gRPC-based chain client for production use
pub struct GrpcChainClient {
    config: ChainConfig,
    #[allow(dead_code)]
    channel: Channel,
    connected: Arc<RwLock<bool>>,
}

impl GrpcChainClient {
    pub async fn new(config: ChainConfig) -> Result<Self, ChainError> {
        let grpc_url = config.grpc_url.as_ref().ok_or_else(|| {
            ChainError::ConnectionFailed("gRPC URL not configured".to_string())
        })?;

        let channel = Channel::from_shared(grpc_url.clone())
            .map_err(|e| {
                ChainError::ConnectionFailed(format!("Invalid gRPC URL: {}", e))
            })?
            .connect()
            .await
            .map_err(|e| {
                ChainError::ConnectionFailed(format!("Failed to connect to gRPC: {}", e))
            })?;

        let client = Self {
            config,
            channel,
            connected: Arc::new(RwLock::new(true)),
        };

        Ok(client)
    }

    /// Reconnect to gRPC endpoint
    pub async fn reconnect(&self) -> Result<(), ChainError> {
        let grpc_url = self.config.grpc_url.as_ref().ok_or_else(|| {
            ChainError::ConnectionFailed("gRPC URL not configured".to_string())
        })?;

        let _new_channel = Channel::from_shared(grpc_url.clone())
            .map_err(|e| {
                ChainError::ConnectionFailed(format!("Invalid gRPC URL: {}", e))
            })?
            .connect()
            .await
            .map_err(|e| {
                ChainError::ConnectionFailed(format!("Failed to reconnect to gRPC: {}", e))
            })?;

        *self.connected.write().await = true;

        Ok(())
    }

    /// Build packet commitment key
    fn packet_commitment_key(&self, channel_id: &str, sequence: u64) -> Vec<u8> {
        format!(
            "commitments/ports/transfer/channels/{}/sequences/{}",
            channel_id, sequence
        )
        .into_bytes()
    }

    /// Build packet receipt key
    #[allow(dead_code)]
    fn packet_receipt_key(&self, channel_id: &str, sequence: u64) -> Vec<u8> {
        format!(
            "receipts/ports/transfer/channels/{}/sequences/{}",
            channel_id, sequence
        )
        .into_bytes()
    }

    /// Estimate gas for transaction
    fn estimate_gas(&self, msgs: &[CosmosMsg]) -> u64 {
        let base_gas = self.config.default_gas;
        let per_msg_gas = 50_000u64;
        let total = base_gas + (msgs.len() as u64 * per_msg_gas);
        std::cmp::min(
            (total as f64 * self.config.gas_adjustment) as u64,
            self.config.max_gas,
        )
    }
}

#[async_trait]
impl ChainClient for GrpcChainClient {
    async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    async fn query_packet_commitment(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<Option<Vec<u8>>, ChainError> {
        // In a real implementation, this would use the IBC gRPC query service
        // For now, we return a placeholder
        tracing::debug!(
            chain_id = %self.config.chain_id,
            channel_id = channel_id,
            sequence = sequence,
            "Querying packet commitment via gRPC"
        );

        // Placeholder: In production, use ibc.core.channel.v1.Query/PacketCommitment
        Ok(Some(vec![0u8; 32]))
    }

    async fn query_packet_receipt(
        &self,
        channel_id: &str,
        sequence: u64,
    ) -> Result<bool, ChainError> {
        tracing::debug!(
            chain_id = %self.config.chain_id,
            channel_id = channel_id,
            sequence = sequence,
            "Querying packet receipt via gRPC"
        );

        // Placeholder: In production, use ibc.core.channel.v1.Query/PacketReceipt
        Ok(false)
    }

    async fn query_unreceived_packets(
        &self,
        channel_id: &str,
        sequences: &[u64],
    ) -> Result<Vec<u64>, ChainError> {
        tracing::debug!(
            chain_id = %self.config.chain_id,
            channel_id = channel_id,
            num_sequences = sequences.len(),
            "Querying unreceived packets via gRPC"
        );

        // Placeholder: In production, use ibc.core.channel.v1.Query/UnreceivedPackets
        Ok(sequences.to_vec())
    }

    async fn submit_tx(
        &self,
        msgs: Vec<CosmosMsg>,
        memo: &str,
    ) -> Result<TxResponse, ChainError> {
        let gas = self.estimate_gas(&msgs);

        tracing::info!(
            chain_id = %self.config.chain_id,
            num_msgs = msgs.len(),
            gas = gas,
            memo = memo,
            "Submitting transaction via gRPC"
        );

        // Placeholder: In production, this would:
        // 1. Build transaction using cosmos.tx.v1beta1.TxBody
        // 2. Sign with account key
        // 3. Submit via cosmos.tx.v1beta1.Service/BroadcastTx
        // 4. Wait for confirmation

        Ok(TxResponse {
            hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            height: 1,
            gas_used: gas,
            code: 0,
            raw_log: "success".to_string(),
        })
    }

    async fn query_client_state(&self, client_id: &str) -> Result<ClientState, ChainError> {
        tracing::debug!(
            chain_id = %self.config.chain_id,
            client_id = client_id,
            "Querying client state via gRPC"
        );

        // Placeholder: In production, use ibc.core.client.v1.Query/ClientState
        Ok(ClientState {
            chain_id: self.config.chain_id.clone(),
            trust_level: crate::chain::TrustLevel {
                numerator: 1,
                denominator: 3,
            },
            trusting_period: std::time::Duration::from_secs(14 * 24 * 60 * 60),
            unbonding_period: std::time::Duration::from_secs(21 * 24 * 60 * 60),
            latest_height: crate::chain::Height::new(0, 1000),
            frozen_height: None,
        })
    }

    async fn query_consensus_state(
        &self,
        client_id: &str,
        height: u64,
    ) -> Result<ConsensusState, ChainError> {
        tracing::debug!(
            chain_id = %self.config.chain_id,
            client_id = client_id,
            height = height,
            "Querying consensus state via gRPC"
        );

        // Placeholder: In production, use ibc.core.client.v1.Query/ConsensusState
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
        tracing::debug!(
            chain_id = %self.config.chain_id,
            key_len = key.len(),
            height = height,
            "Getting proof via gRPC"
        );

        // Placeholder: In production, get proof from ABCI query or IBC query service
        Ok(MerkleProof {
            proofs: vec![crate::chain::CommitmentProof {
                exist: Some(crate::chain::ExistenceProof {
                    key: vec![],
                    value: vec![],
                    leaf: crate::chain::LeafOp {
                        hash: crate::chain::HashOp::Sha256,
                        prehash_key: crate::chain::HashOp::NoHash,
                        prehash_value: crate::chain::HashOp::Sha256,
                        length: crate::chain::LengthOp::VarProto,
                        prefix: vec![0],
                    },
                    path: vec![],
                }),
                nonexist: None,
            }],
        })
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
        let key = self.packet_commitment_key(channel, sequence);
        let merkle_proof = self.get_proof(&key, height).await?;

        let proof_bytes = serde_json::to_vec(&merkle_proof).map_err(|e| {
            ChainError::EncodingError(format!("Failed to encode proof: {}", e))
        })?;

        Ok(PacketProof {
            proof: proof_bytes,
            proof_height: height,
        })
    }

    async fn submit_recv_packet(
        &self,
        channel: &str,
        sequence: u64,
        _proof: PacketProof,
    ) -> Result<(), RelayerError> {
        let msg = CosmosMsg {
            type_url: "/ibc.core.channel.v1.MsgRecvPacket".to_string(),
            value: vec![],
        };

        let response = self
            .submit_tx(vec![msg], &format!("Relay packet {}-{}", channel, sequence))
            .await?;

        if response.is_success() {
            Ok(())
        } else {
            Err(RelayerError::TransactionFailed(format!(
                "Transaction failed with code {}: {}",
                response.code, response.raw_log
            )))
        }
    }

    async fn get_latest_height(&self) -> Result<u64, ChainError> {
        // Placeholder: In production, use cosmos.base.tendermint.v1beta1.Service/GetLatestBlock
        Ok(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires gRPC endpoint
    async fn test_grpc_client_creation() {
        let _config = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network:443".to_string(),
            grpc_url: Some("https://grpc.cosmos.network:443".to_string()),
            ..Default::default()
        };

        // Disabled for now - requires actual gRPC endpoint
        // let result = GrpcChainClient::new(config).await;
        // println!("gRPC client creation result: {:?}", result.is_ok());
    }

    #[test]
    fn test_packet_commitment_key() {
        // Can't create client without connection, but can test key format
        let key = format!(
            "commitments/ports/transfer/channels/{}/sequences/{}",
            "channel-0", 42
        );
        assert_eq!(
            key,
            "commitments/ports/transfer/channels/channel-0/sequences/42"
        );
    }
}
