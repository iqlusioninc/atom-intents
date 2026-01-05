//! Eureka packet monitoring for IBC Eureka transfers
//!
//! Provides a trait for monitoring Eureka packets and a mock implementation
//! for testing purposes.

use async_trait::async_trait;
use atom_intents_types::{EurekaPacketInfo, EurekaPacketStatus, MonitorError};
use cosmwasm_std::Uint128;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Trait for monitoring Eureka IBC packets
#[async_trait]
pub trait EurekaPacketMonitor: Send + Sync {
    /// Watch for Eureka packets matching the given criteria
    async fn watch_for_packet(
        &self,
        intent_id: &str,
        eth_sender: &str,
        expected_amount: Uint128,
        timeout: Duration,
    ) -> Result<EurekaPacketStatus, MonitorError>;

    /// Get the current status of a known packet
    async fn get_packet_status(&self, packet_id: &str) -> Result<EurekaPacketStatus, MonitorError>;

    /// Verify ZK proof finality for a packet
    async fn verify_finality(&self, packet_id: &str) -> Result<bool, MonitorError>;

    /// Get detailed packet information
    async fn get_packet_info(&self, packet_id: &str) -> Result<EurekaPacketInfo, MonitorError>;
}

/// Mock implementation for testing
pub struct MockEurekaMonitor {
    packets: Arc<RwLock<HashMap<String, EurekaPacketStatus>>>,
    packet_info: Arc<RwLock<HashMap<String, EurekaPacketInfo>>>,
}

impl MockEurekaMonitor {
    pub fn new() -> Self {
        Self {
            packets: Arc::new(RwLock::new(HashMap::new())),
            packet_info: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Simulate a packet being received
    pub fn simulate_packet_received(
        &self,
        packet_id: &str,
        sender: &str,
        amount: Uint128,
        received_at: u64,
    ) {
        let mut packets = self.packets.write().unwrap();
        packets.insert(
            packet_id.to_string(),
            EurekaPacketStatus::Pending {
                packet_id: packet_id.to_string(),
                amount,
                sender: sender.to_string(),
                received_at,
            },
        );
    }

    /// Simulate a packet being finalized
    pub fn simulate_packet_finalized(&self, packet_id: &str, amount: Uint128, proof_block: u64) {
        let mut packets = self.packets.write().unwrap();
        packets.insert(
            packet_id.to_string(),
            EurekaPacketStatus::Finalized {
                packet_id: packet_id.to_string(),
                amount,
                proof_block,
            },
        );
    }

    /// Simulate a packet failure
    pub fn simulate_packet_failed(&self, packet_id: &str, reason: &str) {
        let mut packets = self.packets.write().unwrap();
        packets.insert(
            packet_id.to_string(),
            EurekaPacketStatus::Failed {
                reason: reason.to_string(),
            },
        );
    }

    /// Add mock packet info
    pub fn add_packet_info(&self, info: EurekaPacketInfo) {
        let mut packet_info = self.packet_info.write().unwrap();
        packet_info.insert(info.packet_id.clone(), info);
    }
}

impl Default for MockEurekaMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EurekaPacketMonitor for MockEurekaMonitor {
    async fn watch_for_packet(
        &self,
        _intent_id: &str,
        eth_sender: &str,
        expected_amount: Uint128,
        _timeout: Duration,
    ) -> Result<EurekaPacketStatus, MonitorError> {
        // In mock, immediately return pending status
        Ok(EurekaPacketStatus::Pending {
            packet_id: format!("mock-pkt-{}", eth_sender),
            amount: expected_amount,
            sender: eth_sender.to_string(),
            received_at: 1704067200,
        })
    }

    async fn get_packet_status(&self, packet_id: &str) -> Result<EurekaPacketStatus, MonitorError> {
        let packets = self.packets.read().unwrap();
        packets
            .get(packet_id)
            .cloned()
            .ok_or_else(|| MonitorError::PacketNotFound(packet_id.to_string()))
    }

    async fn verify_finality(&self, packet_id: &str) -> Result<bool, MonitorError> {
        let packets = self.packets.read().unwrap();
        match packets.get(packet_id) {
            Some(EurekaPacketStatus::Finalized { .. }) => Ok(true),
            Some(_) => Ok(false),
            None => Err(MonitorError::PacketNotFound(packet_id.to_string())),
        }
    }

    async fn get_packet_info(&self, packet_id: &str) -> Result<EurekaPacketInfo, MonitorError> {
        let packet_info = self.packet_info.read().unwrap();
        packet_info
            .get(packet_id)
            .cloned()
            .ok_or_else(|| MonitorError::PacketNotFound(packet_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_monitor_watch() {
        let monitor = MockEurekaMonitor::new();
        let result = monitor
            .watch_for_packet(
                "int-1",
                "0x1234",
                Uint128::new(1_000_000),
                Duration::from_secs(60),
            )
            .await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.is_pending());
    }

    #[tokio::test]
    async fn test_mock_monitor_packet_lifecycle() {
        let monitor = MockEurekaMonitor::new();

        // Initially not found
        let result = monitor.get_packet_status("pkt-123").await;
        assert!(result.is_err());

        // Simulate received
        monitor.simulate_packet_received("pkt-123", "0x1234", Uint128::new(1_000_000), 1704067200);

        let status = monitor.get_packet_status("pkt-123").await.unwrap();
        assert!(status.is_pending());
        assert!(!monitor.verify_finality("pkt-123").await.unwrap());

        // Simulate finalized
        monitor.simulate_packet_finalized("pkt-123", Uint128::new(1_000_000), 12345678);

        let status = monitor.get_packet_status("pkt-123").await.unwrap();
        assert!(status.is_finalized());
        assert!(monitor.verify_finality("pkt-123").await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_monitor_failure() {
        let monitor = MockEurekaMonitor::new();

        monitor.simulate_packet_received("pkt-456", "0x5678", Uint128::new(500_000), 1704067200);
        monitor.simulate_packet_failed("pkt-456", "timeout");

        let status = monitor.get_packet_status("pkt-456").await.unwrap();
        assert!(status.is_failed());
    }

    #[tokio::test]
    async fn test_mock_monitor_packet_info() {
        let monitor = MockEurekaMonitor::new();

        let info = EurekaPacketInfo {
            packet_id: "pkt-789".to_string(),
            source_chain: "ethereum-1".to_string(),
            dest_chain: "cosmoshub-4".to_string(),
            sender: "0xabcd".to_string(),
            receiver: "cosmos1xyz".to_string(),
            denom: "usdc".to_string(),
            amount: Uint128::new(1_000_000),
            memo: "{}".to_string(),
            timeout_timestamp: 9999999999,
            received_at: Some(1704067200),
            finalized_at: None,
        };

        monitor.add_packet_info(info.clone());

        let retrieved = monitor.get_packet_info("pkt-789").await.unwrap();
        assert_eq!(retrieved.sender, "0xabcd");
        assert_eq!(retrieved.amount, Uint128::new(1_000_000));
    }
}
