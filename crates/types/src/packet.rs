//! Eureka packet types for monitoring IBC Eureka transfers

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use thiserror::Error;

/// Status of an Eureka packet
#[cw_serde]
pub enum EurekaPacketStatus {
    /// Packet not yet seen
    NotFound,

    /// Packet received, ZK proof pending
    Pending {
        packet_id: String,
        amount: Uint128,
        sender: String,
        received_at: u64,
    },

    /// ZK proof verified, fully finalized
    Finalized {
        packet_id: String,
        amount: Uint128,
        proof_block: u64,
    },

    /// Packet failed or timed out
    Failed {
        reason: String,
    },
}

impl EurekaPacketStatus {
    pub fn is_finalized(&self) -> bool {
        matches!(self, Self::Finalized { .. })
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn packet_id(&self) -> Option<&str> {
        match self {
            Self::Pending { packet_id, .. } | Self::Finalized { packet_id, .. } => Some(packet_id),
            _ => None,
        }
    }

    pub fn amount(&self) -> Option<Uint128> {
        match self {
            Self::Pending { amount, .. } | Self::Finalized { amount, .. } => Some(*amount),
            _ => None,
        }
    }
}

/// Detailed information about an Eureka packet
#[cw_serde]
pub struct EurekaPacketInfo {
    pub packet_id: String,
    pub source_chain: String,
    pub dest_chain: String,
    pub sender: String,
    pub receiver: String,
    pub denom: String,
    pub amount: Uint128,
    pub memo: String,
    pub timeout_timestamp: u64,
    pub received_at: Option<u64>,
    pub finalized_at: Option<u64>,
}

/// Errors that can occur during packet monitoring
#[derive(Error, Debug, Clone)]
pub enum MonitorError {
    #[error("Failed to connect to chain: {0}")]
    ConnectionFailed(String),

    #[error("Packet not found: {0}")]
    PacketNotFound(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Invalid packet data: {0}")]
    InvalidPacket(String),

    #[error("Timeout waiting for packet: {0}")]
    Timeout(String),

    #[error("ZK proof verification failed: {0}")]
    ProofVerificationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_status_not_found() {
        let status = EurekaPacketStatus::NotFound;
        assert!(!status.is_finalized());
        assert!(!status.is_pending());
        assert!(status.packet_id().is_none());
    }

    #[test]
    fn test_packet_status_pending() {
        let status = EurekaPacketStatus::Pending {
            packet_id: "pkt-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0x1234".to_string(),
            received_at: 1704067200,
        };
        assert!(status.is_pending());
        assert!(!status.is_finalized());
        assert_eq!(status.packet_id(), Some("pkt-123"));
        assert_eq!(status.amount(), Some(Uint128::new(1_000_000)));
    }

    #[test]
    fn test_packet_status_finalized() {
        let status = EurekaPacketStatus::Finalized {
            packet_id: "pkt-123".to_string(),
            amount: Uint128::new(1_000_000),
            proof_block: 12345678,
        };
        assert!(status.is_finalized());
        assert!(!status.is_pending());
    }

    #[test]
    fn test_packet_info() {
        let info = EurekaPacketInfo {
            packet_id: "pkt-456".to_string(),
            source_chain: "ethereum-1".to_string(),
            dest_chain: "cosmoshub-4".to_string(),
            sender: "0xabcd".to_string(),
            receiver: "cosmos1xyz".to_string(),
            denom: "usdc".to_string(),
            amount: Uint128::new(500_000_000),
            memo: r#"{"atom_intents":{"intent_id":"int-1"}}"#.to_string(),
            timeout_timestamp: 9999999999,
            received_at: Some(1704067200),
            finalized_at: None,
        };
        assert_eq!(info.source_chain, "ethereum-1");
        assert_eq!(info.amount, Uint128::new(500_000_000));
    }

    #[test]
    fn test_monitor_error_display() {
        let err = MonitorError::PacketNotFound("pkt-999".to_string());
        assert!(err.to_string().contains("pkt-999"));
    }
}
