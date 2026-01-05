//! Ethereum escrow types for IBC Eureka-based intents
//!
//! Enables Ethereum users to participate in atom-intents auctions by
//! escrowing funds via IBC Eureka packets.

use cosmwasm_schema::cw_serde;

use crate::Intent;

/// Status of Eureka escrow for an Ethereum-originated intent
#[cw_serde]
#[derive(Default)]
pub enum EscrowStatus {
    /// Waiting for Eureka packet to arrive
    #[default]
    Pending,

    /// Eureka packet received, awaiting ZK proof finality
    Received { packet_id: String, received_at: u64 },

    /// ZK proof verified, funds fully escrowed
    Finalized {
        packet_id: String,
        finalized_at: u64,
    },

    /// Packet failed or timed out
    Failed { reason: String },
}

impl EscrowStatus {
    /// Check if escrow is fully finalized
    pub fn is_finalized(&self) -> bool {
        matches!(self, Self::Finalized { .. })
    }

    /// Check if escrow has failed
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Check if packet has been received (but not yet finalized)
    pub fn is_received(&self) -> bool {
        matches!(self, Self::Received { .. })
    }

    /// Get packet ID if available
    pub fn packet_id(&self) -> Option<&str> {
        match self {
            Self::Received { packet_id, .. } | Self::Finalized { packet_id, .. } => Some(packet_id),
            _ => None,
        }
    }
}

/// Intent with Ethereum-side escrow via IBC Eureka
///
/// Represents an intent where the user's input funds are coming from
/// Ethereum via an IBC Eureka packet. Solvers can front the output
/// before the escrow is finalized, taking on settlement risk.
#[cw_serde]
pub struct EthereumEscrowedIntent {
    /// The underlying intent
    pub base: Intent,

    /// Ethereum address that will send the Eureka packet
    pub eth_sender: String,

    /// Expected Eureka packet hash (optional, for pre-commitment)
    pub expected_packet_hash: Option<String>,

    /// Current escrow status
    pub escrow_status: EscrowStatus,

    /// Eureka timeout in seconds from creation
    pub eureka_timeout_secs: u64,
}

impl EthereumEscrowedIntent {
    /// Create new Ethereum escrowed intent
    pub fn new(base: Intent, eth_sender: String, eureka_timeout_secs: u64) -> Self {
        Self {
            base,
            eth_sender,
            expected_packet_hash: None,
            escrow_status: EscrowStatus::Pending,
            eureka_timeout_secs,
        }
    }

    /// Check if intent is still waiting for packet
    pub fn is_pending(&self) -> bool {
        matches!(self.escrow_status, EscrowStatus::Pending)
    }

    /// Check if intent is ready for solver fronting
    /// (packet received but not yet finalized)
    pub fn is_ready_for_fronting(&self) -> bool {
        matches!(self.escrow_status, EscrowStatus::Received { .. })
    }

    /// Check if escrow is fully finalized
    pub fn is_finalized(&self) -> bool {
        self.escrow_status.is_finalized()
    }

    /// Check if escrow has failed
    pub fn is_failed(&self) -> bool {
        self.escrow_status.is_failed()
    }

    /// Update status to received
    pub fn mark_received(&mut self, packet_id: String, received_at: u64) {
        self.escrow_status = EscrowStatus::Received {
            packet_id,
            received_at,
        };
    }

    /// Update status to finalized
    pub fn mark_finalized(&mut self, packet_id: String, finalized_at: u64) {
        self.escrow_status = EscrowStatus::Finalized {
            packet_id,
            finalized_at,
        };
    }

    /// Update status to failed
    pub fn mark_failed(&mut self, reason: String) {
        self.escrow_status = EscrowStatus::Failed { reason };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Asset, ExecutionConstraints, FillConfig, OutputSpec};
    use cosmwasm_std::{Binary, Uint128};

    fn mock_intent() -> Intent {
        Intent {
            id: "test-eth-escrow-1".to_string(),
            version: "1".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset {
                chain_id: "ethereum-1".to_string(),
                denom: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC
                amount: Uint128::new(1_000_000_000),                             // 1000 USDC
            },
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uatom".to_string(),
                min_amount: Uint128::new(100_000_000), // 100 ATOM
                limit_price: "0.1".to_string(),
                recipient: "cosmos1recipient".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::default(),
            public_key: Binary::default(),
            created_at: 1704067200,
            expires_at: 9999999999,
        }
    }

    #[test]
    fn test_escrow_status_pending() {
        let status = EscrowStatus::Pending;
        assert!(!status.is_finalized());
        assert!(!status.is_failed());
        assert!(!status.is_received());
        assert!(status.packet_id().is_none());
    }

    #[test]
    fn test_escrow_status_received() {
        let status = EscrowStatus::Received {
            packet_id: "pkt-123".to_string(),
            received_at: 1704067200,
        };
        assert!(!status.is_finalized());
        assert!(!status.is_failed());
        assert!(status.is_received());
        assert_eq!(status.packet_id(), Some("pkt-123"));
    }

    #[test]
    fn test_escrow_status_finalized() {
        let status = EscrowStatus::Finalized {
            packet_id: "pkt-123".to_string(),
            finalized_at: 1704067230,
        };
        assert!(status.is_finalized());
        assert!(!status.is_failed());
        assert!(!status.is_received());
        assert_eq!(status.packet_id(), Some("pkt-123"));
    }

    #[test]
    fn test_escrow_status_failed() {
        let status = EscrowStatus::Failed {
            reason: "timeout".to_string(),
        };
        assert!(!status.is_finalized());
        assert!(status.is_failed());
        assert!(status.packet_id().is_none());
    }

    #[test]
    fn test_ethereum_escrowed_intent_new() {
        let intent = mock_intent();
        let escrowed = EthereumEscrowedIntent::new(
            intent.clone(),
            "0x1234567890abcdef".to_string(),
            300, // 5 min timeout
        );

        assert_eq!(escrowed.eth_sender, "0x1234567890abcdef");
        assert_eq!(escrowed.eureka_timeout_secs, 300);
        assert!(escrowed.is_pending());
        assert!(!escrowed.is_ready_for_fronting());
        assert!(!escrowed.is_finalized());
    }

    #[test]
    fn test_ethereum_escrowed_intent_lifecycle() {
        let mut escrowed = EthereumEscrowedIntent::new(mock_intent(), "0x1234".to_string(), 300);

        // Initially pending
        assert!(escrowed.is_pending());

        // Mark as received
        escrowed.mark_received("pkt-456".to_string(), 1704067200);
        assert!(!escrowed.is_pending());
        assert!(escrowed.is_ready_for_fronting());
        assert!(!escrowed.is_finalized());

        // Mark as finalized
        escrowed.mark_finalized("pkt-456".to_string(), 1704067230);
        assert!(!escrowed.is_ready_for_fronting());
        assert!(escrowed.is_finalized());
    }

    #[test]
    fn test_ethereum_escrowed_intent_failure() {
        let mut escrowed = EthereumEscrowedIntent::new(mock_intent(), "0x1234".to_string(), 300);

        escrowed.mark_failed("packet timed out".to_string());
        assert!(escrowed.is_failed());
        assert!(!escrowed.is_pending());
        assert!(!escrowed.is_ready_for_fronting());
    }
}
