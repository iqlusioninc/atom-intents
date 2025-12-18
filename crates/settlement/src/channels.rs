use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// IBC Channel Registry - maps (source_chain, dest_chain) to channel info
#[derive(Clone, Debug)]
pub struct ChannelRegistry {
    channels: HashMap<(String, String), ChannelInfo>,
}

/// Information about an IBC channel
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Channel ID on the source chain
    pub channel_id: String,

    /// Port ID (typically "transfer" for ICS20)
    pub port_id: String,

    /// Channel ID on the counterparty chain
    pub counterparty_channel_id: String,

    /// Connection ID
    pub connection_id: String,

    /// Client ID
    pub client_id: String,

    /// Channel ordering
    pub ordering: ChannelOrdering,

    /// Channel version (typically "ics20-1" for ICS20)
    pub version: String,
}

/// Channel ordering type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelOrdering {
    Ordered,
    Unordered,
}

impl ChannelRegistry {
    /// Create an empty channel registry
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Create a registry pre-populated with known mainnet channels
    pub fn with_mainnet_channels() -> Self {
        let mut registry = Self::new();

        // cosmoshub-4 <-> osmosis-1
        registry.register_channel(
            "cosmoshub-4",
            "osmosis-1",
            ChannelInfo {
                channel_id: "channel-141".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-0".to_string(),
                connection_id: "connection-257".to_string(),
                client_id: "07-tendermint-259".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "osmosis-1",
            "cosmoshub-4",
            ChannelInfo {
                channel_id: "channel-0".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-141".to_string(),
                connection_id: "connection-0".to_string(),
                client_id: "07-tendermint-0".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        // cosmoshub-4 <-> neutron-1
        registry.register_channel(
            "cosmoshub-4",
            "neutron-1",
            ChannelInfo {
                channel_id: "channel-569".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-1".to_string(),
                connection_id: "connection-809".to_string(),
                client_id: "07-tendermint-1119".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "neutron-1",
            "cosmoshub-4",
            ChannelInfo {
                channel_id: "channel-1".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-569".to_string(),
                connection_id: "connection-0".to_string(),
                client_id: "07-tendermint-0".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        // osmosis-1 <-> neutron-1
        registry.register_channel(
            "osmosis-1",
            "neutron-1",
            ChannelInfo {
                channel_id: "channel-874".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-10".to_string(),
                connection_id: "connection-2338".to_string(),
                client_id: "07-tendermint-2823".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "neutron-1",
            "osmosis-1",
            ChannelInfo {
                channel_id: "channel-10".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-874".to_string(),
                connection_id: "connection-8".to_string(),
                client_id: "07-tendermint-8".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        // cosmoshub-4 <-> stride-1
        registry.register_channel(
            "cosmoshub-4",
            "stride-1",
            ChannelInfo {
                channel_id: "channel-391".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-0".to_string(),
                connection_id: "connection-635".to_string(),
                client_id: "07-tendermint-913".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "stride-1",
            "cosmoshub-4",
            ChannelInfo {
                channel_id: "channel-0".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-391".to_string(),
                connection_id: "connection-0".to_string(),
                client_id: "07-tendermint-0".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        // osmosis-1 <-> stride-1
        registry.register_channel(
            "osmosis-1",
            "stride-1",
            ChannelInfo {
                channel_id: "channel-326".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-5".to_string(),
                connection_id: "connection-1464".to_string(),
                client_id: "07-tendermint-1557".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "stride-1",
            "osmosis-1",
            ChannelInfo {
                channel_id: "channel-5".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-326".to_string(),
                connection_id: "connection-4".to_string(),
                client_id: "07-tendermint-4".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry
    }

    /// Create a registry pre-populated with known testnet channels
    pub fn with_testnet_channels() -> Self {
        let mut registry = Self::new();

        // theta-testnet-001 <-> osmo-test-5
        registry.register_channel(
            "theta-testnet-001",
            "osmo-test-5",
            ChannelInfo {
                channel_id: "channel-3306".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-4156".to_string(),
                connection_id: "connection-2727".to_string(),
                client_id: "07-tendermint-2728".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "osmo-test-5",
            "theta-testnet-001",
            ChannelInfo {
                channel_id: "channel-4156".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-3306".to_string(),
                connection_id: "connection-3651".to_string(),
                client_id: "07-tendermint-3651".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        // theta-testnet-001 <-> pion-1 (neutron testnet)
        registry.register_channel(
            "theta-testnet-001",
            "pion-1",
            ChannelInfo {
                channel_id: "channel-3839".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-2223".to_string(),
                connection_id: "connection-3152".to_string(),
                client_id: "07-tendermint-3199".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry.register_channel(
            "pion-1",
            "theta-testnet-001",
            ChannelInfo {
                channel_id: "channel-2223".to_string(),
                port_id: "transfer".to_string(),
                counterparty_channel_id: "channel-3839".to_string(),
                connection_id: "connection-2013".to_string(),
                client_id: "07-tendermint-136".to_string(),
                ordering: ChannelOrdering::Unordered,
                version: "ics20-1".to_string(),
            },
        );

        registry
    }

    /// Register a channel mapping
    pub fn register_channel(
        &mut self,
        source_chain: impl Into<String>,
        dest_chain: impl Into<String>,
        channel_info: ChannelInfo,
    ) {
        let key = (source_chain.into(), dest_chain.into());
        self.channels.insert(key, channel_info);
    }

    /// Get channel info for a source->dest chain pair
    pub fn get_channel(
        &self,
        source_chain: &str,
        dest_chain: &str,
    ) -> Option<&ChannelInfo> {
        let key = (source_chain.to_string(), dest_chain.to_string());
        self.channels.get(&key)
    }

    /// Get channel info or return an error
    pub fn get_channel_or_error(
        &self,
        source_chain: &str,
        dest_chain: &str,
    ) -> Result<&ChannelInfo, crate::ChannelError> {
        self.get_channel(source_chain, dest_chain)
            .ok_or_else(|| crate::ChannelError::ChannelNotFound(
                source_chain.to_string(),
                dest_chain.to_string(),
            ))
    }

    /// Get the reverse channel (dest->source)
    pub fn get_reverse_channel(
        &self,
        source_chain: &str,
        dest_chain: &str,
    ) -> Option<&ChannelInfo> {
        self.get_channel(dest_chain, source_chain)
    }

    /// Check if a channel exists between two chains
    pub fn has_channel(&self, source_chain: &str, dest_chain: &str) -> bool {
        self.get_channel(source_chain, dest_chain).is_some()
    }

    /// Get all registered channels
    pub fn all_channels(&self) -> &HashMap<(String, String), ChannelInfo> {
        &self.channels
    }

    /// Get the number of registered channels
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry() {
        let registry = ChannelRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_register_and_get_channel() {
        let mut registry = ChannelRegistry::new();

        let channel_info = ChannelInfo {
            channel_id: "channel-0".to_string(),
            port_id: "transfer".to_string(),
            counterparty_channel_id: "channel-141".to_string(),
            connection_id: "connection-0".to_string(),
            client_id: "07-tendermint-0".to_string(),
            ordering: ChannelOrdering::Unordered,
            version: "ics20-1".to_string(),
        };

        registry.register_channel("osmosis-1", "cosmoshub-4", channel_info.clone());

        let retrieved = registry.get_channel("osmosis-1", "cosmoshub-4");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().channel_id, "channel-0");
    }

    #[test]
    fn test_get_channel_not_found() {
        let registry = ChannelRegistry::new();
        assert!(registry.get_channel("chain-a", "chain-b").is_none());
    }

    #[test]
    fn test_get_channel_or_error() {
        let registry = ChannelRegistry::new();
        let result = registry.get_channel_or_error("chain-a", "chain-b");
        assert!(result.is_err());

        match result {
            Err(crate::ChannelError::ChannelNotFound(source, dest)) => {
                assert_eq!(source, "chain-a");
                assert_eq!(dest, "chain-b");
            }
            _ => panic!("Expected ChannelNotFound error"),
        }
    }

    #[test]
    fn test_reverse_channel() {
        let mut registry = ChannelRegistry::new();

        let forward = ChannelInfo {
            channel_id: "channel-0".to_string(),
            port_id: "transfer".to_string(),
            counterparty_channel_id: "channel-141".to_string(),
            connection_id: "connection-0".to_string(),
            client_id: "07-tendermint-0".to_string(),
            ordering: ChannelOrdering::Unordered,
            version: "ics20-1".to_string(),
        };

        let reverse = ChannelInfo {
            channel_id: "channel-141".to_string(),
            port_id: "transfer".to_string(),
            counterparty_channel_id: "channel-0".to_string(),
            connection_id: "connection-257".to_string(),
            client_id: "07-tendermint-259".to_string(),
            ordering: ChannelOrdering::Unordered,
            version: "ics20-1".to_string(),
        };

        registry.register_channel("osmosis-1", "cosmoshub-4", forward);
        registry.register_channel("cosmoshub-4", "osmosis-1", reverse);

        let reverse_channel = registry.get_reverse_channel("osmosis-1", "cosmoshub-4");
        assert!(reverse_channel.is_some());
        assert_eq!(reverse_channel.unwrap().channel_id, "channel-141");
    }

    #[test]
    fn test_has_channel() {
        let mut registry = ChannelRegistry::new();

        let channel_info = ChannelInfo {
            channel_id: "channel-0".to_string(),
            port_id: "transfer".to_string(),
            counterparty_channel_id: "channel-141".to_string(),
            connection_id: "connection-0".to_string(),
            client_id: "07-tendermint-0".to_string(),
            ordering: ChannelOrdering::Unordered,
            version: "ics20-1".to_string(),
        };

        registry.register_channel("osmosis-1", "cosmoshub-4", channel_info);

        assert!(registry.has_channel("osmosis-1", "cosmoshub-4"));
        assert!(!registry.has_channel("cosmoshub-4", "osmosis-1"));
    }

    #[test]
    fn test_mainnet_channels() {
        let registry = ChannelRegistry::with_mainnet_channels();

        // Should not be empty
        assert!(!registry.is_empty());

        // Test cosmoshub-4 <-> osmosis-1
        let hub_to_osmo = registry.get_channel("cosmoshub-4", "osmosis-1");
        assert!(hub_to_osmo.is_some());
        assert_eq!(hub_to_osmo.unwrap().channel_id, "channel-141");

        let osmo_to_hub = registry.get_channel("osmosis-1", "cosmoshub-4");
        assert!(osmo_to_hub.is_some());
        assert_eq!(osmo_to_hub.unwrap().channel_id, "channel-0");

        // Test cosmoshub-4 <-> neutron-1
        let hub_to_neutron = registry.get_channel("cosmoshub-4", "neutron-1");
        assert!(hub_to_neutron.is_some());
        assert_eq!(hub_to_neutron.unwrap().channel_id, "channel-569");

        let neutron_to_hub = registry.get_channel("neutron-1", "cosmoshub-4");
        assert!(neutron_to_hub.is_some());
        assert_eq!(neutron_to_hub.unwrap().channel_id, "channel-1");

        // Test osmosis-1 <-> neutron-1
        let osmo_to_neutron = registry.get_channel("osmosis-1", "neutron-1");
        assert!(osmo_to_neutron.is_some());
        assert_eq!(osmo_to_neutron.unwrap().channel_id, "channel-874");

        let neutron_to_osmo = registry.get_channel("neutron-1", "osmosis-1");
        assert!(neutron_to_osmo.is_some());
        assert_eq!(neutron_to_osmo.unwrap().channel_id, "channel-10");

        // Test cosmoshub-4 <-> stride-1
        let hub_to_stride = registry.get_channel("cosmoshub-4", "stride-1");
        assert!(hub_to_stride.is_some());
        assert_eq!(hub_to_stride.unwrap().channel_id, "channel-391");

        // Test osmosis-1 <-> stride-1
        let osmo_to_stride = registry.get_channel("osmosis-1", "stride-1");
        assert!(osmo_to_stride.is_some());
        assert_eq!(osmo_to_stride.unwrap().channel_id, "channel-326");
    }

    #[test]
    fn test_testnet_channels() {
        let registry = ChannelRegistry::with_testnet_channels();

        // Should not be empty
        assert!(!registry.is_empty());

        // Test theta-testnet-001 <-> osmo-test-5
        let theta_to_osmo = registry.get_channel("theta-testnet-001", "osmo-test-5");
        assert!(theta_to_osmo.is_some());
        assert_eq!(theta_to_osmo.unwrap().channel_id, "channel-3306");

        let osmo_to_theta = registry.get_channel("osmo-test-5", "theta-testnet-001");
        assert!(osmo_to_theta.is_some());
        assert_eq!(osmo_to_theta.unwrap().channel_id, "channel-4156");

        // Test theta-testnet-001 <-> pion-1
        let theta_to_pion = registry.get_channel("theta-testnet-001", "pion-1");
        assert!(theta_to_pion.is_some());
        assert_eq!(theta_to_pion.unwrap().channel_id, "channel-3839");
    }

    #[test]
    fn test_channel_ordering() {
        let registry = ChannelRegistry::with_mainnet_channels();

        let channel = registry.get_channel("cosmoshub-4", "osmosis-1").unwrap();
        assert_eq!(channel.ordering, ChannelOrdering::Unordered);
    }

    #[test]
    fn test_channel_version() {
        let registry = ChannelRegistry::with_mainnet_channels();

        let channel = registry.get_channel("cosmoshub-4", "osmosis-1").unwrap();
        assert_eq!(channel.version, "ics20-1");
    }

    #[test]
    fn test_all_channels() {
        let registry = ChannelRegistry::with_mainnet_channels();
        let all = registry.all_channels();

        assert!(all.contains_key(&("cosmoshub-4".to_string(), "osmosis-1".to_string())));
        assert!(all.contains_key(&("osmosis-1".to_string(), "cosmoshub-4".to_string())));
    }
}
