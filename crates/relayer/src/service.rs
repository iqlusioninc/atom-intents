use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{PrioritizedPacket, PriorityLevel, PriorityQueue};

/// Configuration for the solver relayer
#[derive(Clone, Debug)]
pub struct RelayerConfig {
    /// Solver ID this relayer is integrated with
    pub solver_id: String,

    /// Chains to relay between
    pub chains: Vec<ChainConfig>,

    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,

    /// Maximum packets to process per iteration
    pub batch_size: usize,
}

#[derive(Clone, Debug)]
pub struct ChainConfig {
    pub chain_id: String,
    pub rpc_endpoint: String,
    pub grpc_endpoint: String,
}

/// Solver-integrated relayer service
pub struct SolverRelayer {
    config: RelayerConfig,
    priority_queue: Arc<RwLock<PriorityQueue>>,
    chain_clients: HashMap<String, Arc<dyn ChainClient>>,
    pending_settlements: Arc<RwLock<HashMap<String, Vec<String>>>>, // settlement_id -> packet_ids
}

impl SolverRelayer {
    pub fn new(config: RelayerConfig, chain_clients: HashMap<String, Arc<dyn ChainClient>>) -> Self {
        Self {
            config,
            priority_queue: Arc::new(RwLock::new(PriorityQueue::new())),
            chain_clients,
            pending_settlements: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a packet for priority relay (our solver's packets)
    pub async fn add_own_packet(&self, packet: PacketDetails, solver_exposure: u128) {
        let prioritized = PrioritizedPacket {
            packet_id: format!(
                "{}-{}-{}",
                packet.source_chain, packet.channel, packet.sequence
            ),
            source_chain: packet.source_chain,
            dest_chain: packet.dest_chain,
            channel: packet.channel,
            sequence: packet.sequence,
            priority_level: PriorityLevel::Own,
            solver_exposure,
            timeout_timestamp: packet.timeout_timestamp,
            added_at: current_timestamp(),
        };

        self.priority_queue.write().await.push(prioritized);
    }

    /// Add a paid relay request
    pub async fn add_paid_request(&self, packet: PacketDetails, _payment: u128) {
        let prioritized = PrioritizedPacket {
            packet_id: format!(
                "{}-{}-{}",
                packet.source_chain, packet.channel, packet.sequence
            ),
            source_chain: packet.source_chain,
            dest_chain: packet.dest_chain,
            channel: packet.channel,
            sequence: packet.sequence,
            priority_level: PriorityLevel::Paid,
            solver_exposure: 0,
            timeout_timestamp: packet.timeout_timestamp,
            added_at: current_timestamp(),
        };

        self.priority_queue.write().await.push(prioritized);
    }

    /// Track settlement for priority relaying
    pub async fn track_settlement(&self, settlement_id: &str, packet_ids: Vec<String>) {
        self.pending_settlements
            .write()
            .await
            .insert(settlement_id.to_string(), packet_ids);
    }

    /// Run the relayer loop
    pub async fn run(&self) -> Result<(), RelayerError> {
        loop {
            // Process highest priority packet
            if let Some(packet) = self.priority_queue.write().await.pop() {
                match self.relay_packet(&packet).await {
                    Ok(_) => {
                        tracing::info!(
                            packet_id = %packet.packet_id,
                            "Successfully relayed packet"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            packet_id = %packet.packet_id,
                            error = %e,
                            "Failed to relay packet, re-queuing"
                        );
                        // Re-queue for retry
                        self.priority_queue.write().await.push(packet);
                    }
                }
            }

            // Refresh priorities periodically
            self.priority_queue.write().await.refresh_priorities();

            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.poll_interval_ms,
            ))
            .await;
        }
    }

    async fn relay_packet(&self, packet: &PrioritizedPacket) -> Result<(), RelayerError> {
        // Get source chain client
        let source_client = self
            .chain_clients
            .get(&packet.source_chain)
            .ok_or_else(|| RelayerError::ChainNotConfigured(packet.source_chain.clone()))?;

        // Get dest chain client
        let dest_client = self
            .chain_clients
            .get(&packet.dest_chain)
            .ok_or_else(|| RelayerError::ChainNotConfigured(packet.dest_chain.clone()))?;

        // Get packet commitment proof from source
        let proof = source_client
            .get_packet_commitment_proof(&packet.channel, packet.sequence)
            .await?;

        // Submit MsgRecvPacket to destination
        dest_client
            .submit_recv_packet(&packet.channel, packet.sequence, proof)
            .await?;

        Ok(())
    }

    /// Get pending packet count
    pub async fn pending_count(&self) -> usize {
        self.priority_queue.read().await.len()
    }

    /// Check if relayer is healthy
    pub async fn is_healthy(&self) -> bool {
        // Check all chain clients are connected
        for client in self.chain_clients.values() {
            if !client.is_connected().await {
                return false;
            }
        }
        true
    }
}

/// Details of an IBC packet
#[derive(Clone, Debug)]
pub struct PacketDetails {
    pub source_chain: String,
    pub dest_chain: String,
    pub channel: String,
    pub sequence: u64,
    pub timeout_timestamp: u64,
}

/// Proof data for IBC packet
#[derive(Clone, Debug)]
pub struct PacketProof {
    pub proof: Vec<u8>,
    pub proof_height: u64,
}

/// Chain client trait for interacting with chains
#[async_trait]
pub trait ChainClient: Send + Sync {
    async fn is_connected(&self) -> bool;
    async fn get_packet_commitment_proof(
        &self,
        channel: &str,
        sequence: u64,
    ) -> Result<PacketProof, RelayerError>;
    async fn submit_recv_packet(
        &self,
        channel: &str,
        sequence: u64,
        proof: PacketProof,
    ) -> Result<(), RelayerError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerError {
    #[error("chain not configured: {0}")]
    ChainNotConfigured(String),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("proof query failed: {0}")]
    ProofQueryFailed(String),

    #[error("transaction failed: {0}")]
    TransactionFailed(String),
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Mock chain client for testing
pub struct MockChainClient {
    chain_id: String,
    connected: Arc<RwLock<bool>>,
}

impl MockChainClient {
    pub fn new(chain_id: impl Into<String>) -> Self {
        Self {
            chain_id: chain_id.into(),
            connected: Arc::new(RwLock::new(true)),
        }
    }

    pub async fn set_connected(&self, connected: bool) {
        *self.connected.write().await = connected;
    }
}

#[async_trait]
impl ChainClient for MockChainClient {
    async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    async fn get_packet_commitment_proof(
        &self,
        channel: &str,
        sequence: u64,
    ) -> Result<PacketProof, RelayerError> {
        Ok(PacketProof {
            proof: vec![0u8; 32],
            proof_height: 1000,
        })
    }

    async fn submit_recv_packet(
        &self,
        channel: &str,
        sequence: u64,
        _proof: PacketProof,
    ) -> Result<(), RelayerError> {
        Ok(())
    }
}
