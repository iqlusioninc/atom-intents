use async_trait::async_trait;
use atom_intents_ratelimit::ExponentialBackoff;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::{PrioritizedPacket, PriorityLevel, PriorityQueue, RetryInfo};

/// Maximum retry attempts before dropping a packet
const MAX_RETRIES: u32 = 10;

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
            retry_info: RetryInfo::default(),
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
            retry_info: RetryInfo::default(),
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
            if let Some(mut packet) = self.priority_queue.write().await.pop() {
                // Skip packet if backoff period hasn't elapsed
                if packet.retry_info.next_retry_at > Instant::now() {
                    // Re-queue and skip for now
                    self.priority_queue.write().await.push(packet);

                    // Refresh priorities periodically
                    self.priority_queue.write().await.refresh_priorities();

                    tokio::time::sleep(tokio::time::Duration::from_millis(
                        self.config.poll_interval_ms,
                    ))
                    .await;
                    continue;
                }

                match self.relay_packet(&packet).await {
                    Ok(_) => {
                        tracing::info!(
                            packet_id = %packet.packet_id,
                            "Successfully relayed packet"
                        );
                    }
                    Err(e) => {
                        packet.retry_info.attempts += 1;
                        packet.retry_info.last_attempt = Instant::now();

                        if packet.retry_info.attempts >= MAX_RETRIES {
                            tracing::error!(
                                packet_id = %packet.packet_id,
                                attempts = packet.retry_info.attempts,
                                error = %e,
                                "Packet exceeded max retries, dropping"
                            );
                            // Don't re-queue, packet is dropped
                            continue;
                        }

                        // Calculate exponential backoff
                        let backoff = calculate_backoff(packet.retry_info.attempts);
                        packet.retry_info.next_retry_at = Instant::now() + backoff;

                        tracing::warn!(
                            packet_id = %packet.packet_id,
                            attempts = packet.retry_info.attempts,
                            backoff_ms = backoff.as_millis(),
                            error = %e,
                            "Failed to relay packet, re-queuing with backoff"
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

/// Calculate exponential backoff duration based on attempt number
/// - First retry: 1 second
/// - Second retry: 2 seconds
/// - Third retry: 4 seconds
/// - Max: 60 seconds
fn calculate_backoff(attempt: u32) -> Duration {
    let mut backoff = ExponentialBackoff::new(Duration::from_secs(1), Duration::from_secs(60));

    // Advance backoff to the current attempt
    for _ in 0..attempt {
        backoff.next_delay();
    }

    backoff.next_delay()
}

/// Mock chain client for testing
pub struct MockChainClient {
    #[allow(dead_code)]
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
        _channel: &str,
        _sequence: u64,
    ) -> Result<PacketProof, RelayerError> {
        Ok(PacketProof {
            proof: vec![0u8; 32],
            proof_height: 1000,
        })
    }

    async fn submit_recv_packet(
        &self,
        _channel: &str,
        _sequence: u64,
        _proof: PacketProof,
    ) -> Result<(), RelayerError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Mock chain client that can be configured to fail
    pub struct FailingChainClient {
        connected: Arc<RwLock<bool>>,
        fail_count: Arc<AtomicU32>,
        total_failures: u32,
    }

    impl FailingChainClient {
        pub fn new(_chain_id: impl Into<String>, fail_count: u32) -> Self {
            Self {
                connected: Arc::new(RwLock::new(true)),
                fail_count: Arc::new(AtomicU32::new(0)),
                total_failures: fail_count,
            }
        }
    }

    #[async_trait]
    impl ChainClient for FailingChainClient {
        async fn is_connected(&self) -> bool {
            *self.connected.read().await
        }

        async fn get_packet_commitment_proof(
            &self,
            _channel: &str,
            _sequence: u64,
        ) -> Result<PacketProof, RelayerError> {
            let current = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if current < self.total_failures {
                Err(RelayerError::ProofQueryFailed("Simulated failure".into()))
            } else {
                Ok(PacketProof {
                    proof: vec![0u8; 32],
                    proof_height: 1000,
                })
            }
        }

        async fn submit_recv_packet(
            &self,
            _channel: &str,
            _sequence: u64,
            _proof: PacketProof,
        ) -> Result<(), RelayerError> {
            Ok(())
        }
    }

    #[test]
    fn test_calculate_backoff_progression() {
        // First retry: 1 second
        let backoff1 = calculate_backoff(0);
        assert_eq!(backoff1, Duration::from_secs(1));

        // Second retry: 2 seconds
        let backoff2 = calculate_backoff(1);
        assert_eq!(backoff2, Duration::from_secs(2));

        // Third retry: 4 seconds
        let backoff3 = calculate_backoff(2);
        assert_eq!(backoff3, Duration::from_secs(4));

        // Fourth retry: 8 seconds
        let backoff4 = calculate_backoff(3);
        assert_eq!(backoff4, Duration::from_secs(8));
    }

    #[test]
    fn test_calculate_backoff_max_cap() {
        // Should cap at 60 seconds
        let backoff = calculate_backoff(10);
        assert_eq!(backoff, Duration::from_secs(60));

        let backoff = calculate_backoff(20);
        assert_eq!(backoff, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_packet_dropped_after_max_retries() {
        let config = RelayerConfig {
            solver_id: "solver-1".to_string(),
            chains: vec![
                ChainConfig {
                    chain_id: "hub".to_string(),
                    rpc_endpoint: "http://localhost:26657".to_string(),
                    grpc_endpoint: "http://localhost:9090".to_string(),
                },
                ChainConfig {
                    chain_id: "noble".to_string(),
                    rpc_endpoint: "http://localhost:26657".to_string(),
                    grpc_endpoint: "http://localhost:9090".to_string(),
                },
            ],
            poll_interval_ms: 10,
            batch_size: 10,
        };

        let mut chain_clients: HashMap<String, Arc<dyn ChainClient>> = HashMap::new();
        // This will always fail
        chain_clients.insert(
            "hub".to_string(),
            Arc::new(FailingChainClient::new("hub", 100)),
        );
        chain_clients.insert("noble".to_string(), Arc::new(MockChainClient::new("noble")));

        let relayer = SolverRelayer::new(config, chain_clients);

        // Add a packet
        relayer
            .add_own_packet(
                PacketDetails {
                    source_chain: "hub".to_string(),
                    dest_chain: "noble".to_string(),
                    channel: "channel-0".to_string(),
                    sequence: 1,
                    timeout_timestamp: current_timestamp() + 1000,
                },
                1_000_000,
            )
            .await;

        // Manually retry until max retries
        for _ in 0..MAX_RETRIES {
            let mut packet = relayer.priority_queue.write().await.pop().unwrap();
            assert!(relayer.relay_packet(&packet).await.is_err());
            packet.retry_info.attempts += 1;

            if packet.retry_info.attempts < MAX_RETRIES {
                // Use minimal backoff for testing
                packet.retry_info.next_retry_at = Instant::now();
                relayer.priority_queue.write().await.push(packet);
            }
            // Don't re-queue if max retries reached
        }

        // Packet should have been dropped
        assert_eq!(relayer.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_retry_with_backoff() {
        let config = RelayerConfig {
            solver_id: "solver-1".to_string(),
            chains: vec![
                ChainConfig {
                    chain_id: "hub".to_string(),
                    rpc_endpoint: "http://localhost:26657".to_string(),
                    grpc_endpoint: "http://localhost:9090".to_string(),
                },
                ChainConfig {
                    chain_id: "noble".to_string(),
                    rpc_endpoint: "http://localhost:26657".to_string(),
                    grpc_endpoint: "http://localhost:9090".to_string(),
                },
            ],
            poll_interval_ms: 10,
            batch_size: 10,
        };

        let mut chain_clients: HashMap<String, Arc<dyn ChainClient>> = HashMap::new();
        // Fail 2 times then succeed
        chain_clients.insert(
            "hub".to_string(),
            Arc::new(FailingChainClient::new("hub", 2)),
        );
        chain_clients.insert("noble".to_string(), Arc::new(MockChainClient::new("noble")));

        let relayer = SolverRelayer::new(config, chain_clients);

        // Add a packet
        relayer
            .add_own_packet(
                PacketDetails {
                    source_chain: "hub".to_string(),
                    dest_chain: "noble".to_string(),
                    channel: "channel-0".to_string(),
                    sequence: 1,
                    timeout_timestamp: current_timestamp() + 1000,
                },
                1_000_000,
            )
            .await;

        assert_eq!(relayer.pending_count().await, 1);

        // First attempt - should fail
        let mut packet = relayer.priority_queue.write().await.pop().unwrap();
        assert!(relayer.relay_packet(&packet).await.is_err());
        packet.retry_info.attempts += 1;
        packet.retry_info.next_retry_at = Instant::now() + calculate_backoff(packet.retry_info.attempts);
        relayer.priority_queue.write().await.push(packet);

        // Should still have the packet
        assert_eq!(relayer.pending_count().await, 1);

        // Second attempt - should fail
        let mut packet = relayer.priority_queue.write().await.pop().unwrap();
        assert!(relayer.relay_packet(&packet).await.is_err());
        packet.retry_info.attempts += 1;
        packet.retry_info.next_retry_at = Instant::now() + calculate_backoff(packet.retry_info.attempts);
        relayer.priority_queue.write().await.push(packet);

        // Third attempt - should succeed
        let packet = relayer.priority_queue.write().await.pop().unwrap();
        assert!(relayer.relay_packet(&packet).await.is_ok());

        // Packet should be removed from queue
        assert_eq!(relayer.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_backoff_prevents_immediate_retry() {
        let mut packet = PrioritizedPacket {
            packet_id: "test-packet".to_string(),
            source_chain: "hub".to_string(),
            dest_chain: "noble".to_string(),
            channel: "channel-0".to_string(),
            sequence: 1,
            priority_level: PriorityLevel::Own,
            solver_exposure: 1_000_000,
            timeout_timestamp: current_timestamp() + 1000,
            added_at: current_timestamp(),
            retry_info: RetryInfo::default(),
        };

        // Simulate a failure and set backoff
        packet.retry_info.attempts = 1;
        let backoff = calculate_backoff(packet.retry_info.attempts);
        packet.retry_info.next_retry_at = Instant::now() + backoff;

        // Packet should not be ready for retry immediately
        assert!(packet.retry_info.next_retry_at > Instant::now());

        // After waiting for backoff, packet should be ready
        tokio::time::sleep(backoff).await;
        assert!(packet.retry_info.next_retry_at <= Instant::now());
    }
}
