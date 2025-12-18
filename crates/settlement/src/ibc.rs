use atom_intents_types::IbcTransferInfo;
use cosmwasm_std::Uint128;
use serde::{Deserialize, Serialize};

/// IBC flow type for settlement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IbcFlowType {
    /// Same chain transfer (~3s)
    SameChain,

    /// Direct IBC transfer (~6s)
    DirectIbc { channel: String },

    /// Multi-hop via Packet Forward Middleware (~15-20s)
    MultiHopPfm { hops: Vec<PfmHop> },

    /// IBC Hooks with Wasm execution (~10-15s)
    IbcHooksWasm { contract: String, msg: String },
}

/// A hop in PFM routing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PfmHop {
    pub receiver: String,
    pub channel: String,
}

/// Build PFM memo for multi-hop transfers
pub fn build_pfm_memo(hops: &[PfmHop]) -> String {
    if hops.is_empty() {
        return String::new();
    }

    fn build_nested(hops: &[PfmHop]) -> serde_json::Value {
        if hops.is_empty() {
            return serde_json::Value::Null;
        }

        let hop = &hops[0];
        let mut obj = serde_json::json!({
            "forward": {
                "receiver": hop.receiver,
                "channel": hop.channel
            }
        });

        if hops.len() > 1 {
            if let Some(next) = build_nested(&hops[1..]).as_object() {
                obj["forward"]["next"] = serde_json::Value::Object(next.clone());
            }
        }

        obj
    }

    build_nested(hops).to_string()
}

/// Build IBC hooks memo for Wasm execution
pub fn build_wasm_memo(contract: &str, msg: &serde_json::Value, forward: Option<&PfmHop>) -> String {
    let mut memo = serde_json::json!({
        "wasm": {
            "contract": contract,
            "msg": msg
        }
    });

    if let Some(hop) = forward {
        memo["forward"] = serde_json::json!({
            "receiver": hop.receiver,
            "channel": hop.channel
        });
    }

    memo.to_string()
}

/// Calculate appropriate IBC timeout based on flow type
pub fn calculate_timeout(flow_type: &IbcFlowType, base_timeout_secs: u64) -> u64 {
    let multiplier = match flow_type {
        IbcFlowType::SameChain => 1,
        IbcFlowType::DirectIbc { .. } => 2,
        IbcFlowType::MultiHopPfm { hops } => 2 + hops.len() as u64,
        IbcFlowType::IbcHooksWasm { .. } => 3,
    };

    base_timeout_secs * multiplier
}

/// Determine the best IBC flow for a transfer using RouteRegistry
pub fn determine_flow_with_routing(
    source_chain: &str,
    dest_chain: &str,
    needs_swap: bool,
    route_registry: &crate::RouteRegistry,
) -> IbcFlowType {
    // Same chain
    if source_chain == dest_chain {
        return IbcFlowType::SameChain;
    }

    // Find the best route
    if let Some(route) = route_registry.find_route(source_chain, dest_chain) {
        if route.hops.is_empty() {
            // Same chain (shouldn't happen, but handle it)
            return IbcFlowType::SameChain;
        }

        if route.hops.len() == 1 {
            // Direct route
            if needs_swap {
                // Would need IBC hooks for swap on destination
                return IbcFlowType::IbcHooksWasm {
                    contract: "swap_router".to_string(),
                    msg: "{}".to_string(),
                };
            }
            return IbcFlowType::DirectIbc {
                channel: route.hops[0].channel_id.clone(),
            };
        }

        // Multi-hop route - convert to PfmHops
        let pfm_hops = route.hops.iter().enumerate().map(|(i, hop)| {
            PfmHop {
                receiver: if i == route.hops.len() - 1 {
                    // Final hop - use actual receiver (will be set by caller)
                    dest_chain.to_string()
                } else {
                    // Intermediate hop - forward to next chain
                    hop.chain_id.clone()
                },
                channel: hop.channel_id.clone(),
            }
        }).collect();

        return IbcFlowType::MultiHopPfm { hops: pfm_hops };
    }

    // No route found - return empty multi-hop as fallback
    IbcFlowType::MultiHopPfm { hops: vec![] }
}

/// Determine the best IBC flow for a transfer (legacy API using channel map)
pub fn determine_flow(
    source_chain: &str,
    dest_chain: &str,
    needs_swap: bool,
    channel_map: &std::collections::HashMap<(String, String), String>,
) -> IbcFlowType {
    // Same chain
    if source_chain == dest_chain {
        return IbcFlowType::SameChain;
    }

    // Direct channel exists
    let key = (source_chain.to_string(), dest_chain.to_string());
    if let Some(channel) = channel_map.get(&key) {
        if needs_swap {
            // Would need IBC hooks for swap
            return IbcFlowType::IbcHooksWasm {
                contract: "swap_router".to_string(),
                msg: "{}".to_string(),
            };
        }
        return IbcFlowType::DirectIbc {
            channel: channel.clone(),
        };
    }

    // Need multi-hop
    // In production, this would query a routing table
    IbcFlowType::MultiHopPfm { hops: vec![] }
}

/// IBC transfer builder
pub struct IbcTransferBuilder {
    source_chain: String,
    dest_chain: String,
    channel: String,
    denom: String,
    amount: Uint128,
    sender: String,
    receiver: String,
    timeout_secs: u64,
    memo: Option<String>,
}

impl IbcTransferBuilder {
    /// Create a new IbcTransferBuilder with an explicit channel
    pub fn new(
        source_chain: impl Into<String>,
        dest_chain: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self {
            source_chain: source_chain.into(),
            dest_chain: dest_chain.into(),
            channel: channel.into(),
            denom: String::new(),
            amount: Uint128::zero(),
            sender: String::new(),
            receiver: String::new(),
            timeout_secs: 600, // 10 minutes default
            memo: None,
        }
    }

    /// Create a new IbcTransferBuilder using the ChannelRegistry
    ///
    /// # Errors
    ///
    /// Returns `ChannelError::ChannelNotFound` if no channel exists between the chains
    pub fn from_registry(
        source_chain: impl Into<String>,
        dest_chain: impl Into<String>,
        registry: &crate::ChannelRegistry,
    ) -> Result<Self, crate::ChannelError> {
        let source = source_chain.into();
        let dest = dest_chain.into();

        let channel_info = registry.get_channel_or_error(&source, &dest)?;

        Ok(Self {
            source_chain: source,
            dest_chain: dest,
            channel: channel_info.channel_id.clone(),
            denom: String::new(),
            amount: Uint128::zero(),
            sender: String::new(),
            receiver: String::new(),
            timeout_secs: 600, // 10 minutes default
            memo: None,
        })
    }

    pub fn denom(mut self, denom: impl Into<String>) -> Self {
        self.denom = denom.into();
        self
    }

    pub fn amount(mut self, amount: Uint128) -> Self {
        self.amount = amount;
        self
    }

    pub fn sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = sender.into();
        self
    }

    pub fn receiver(mut self, receiver: impl Into<String>) -> Self {
        self.receiver = receiver.into();
        self
    }

    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }

    pub fn build(self, current_time: u64) -> IbcTransferInfo {
        IbcTransferInfo {
            source_chain: self.source_chain,
            dest_chain: self.dest_chain,
            channel: self.channel,
            amount: self.amount,
            denom: self.denom,
            sender: self.sender,
            receiver: self.receiver,
            timeout_timestamp: current_time + self.timeout_secs,
            memo: self.memo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelRegistry;

    #[test]
    fn test_ibc_transfer_builder_with_registry() {
        let registry = ChannelRegistry::with_mainnet_channels();

        let builder = IbcTransferBuilder::from_registry(
            "cosmoshub-4",
            "osmosis-1",
            &registry,
        );

        assert!(builder.is_ok());
        let builder = builder.unwrap();
        assert_eq!(builder.channel, "channel-141");
    }

    #[test]
    fn test_ibc_transfer_builder_with_registry_not_found() {
        let registry = ChannelRegistry::new();

        let builder = IbcTransferBuilder::from_registry(
            "unknown-chain-1",
            "unknown-chain-2",
            &registry,
        );

        assert!(builder.is_err());
        match builder {
            Err(crate::ChannelError::ChannelNotFound(source, dest)) => {
                assert_eq!(source, "unknown-chain-1");
                assert_eq!(dest, "unknown-chain-2");
            }
            _ => panic!("Expected ChannelNotFound error"),
        }
    }

    #[test]
    fn test_ibc_transfer_builder_complete_flow() {
        let registry = ChannelRegistry::with_mainnet_channels();

        let transfer = IbcTransferBuilder::from_registry(
            "cosmoshub-4",
            "osmosis-1",
            &registry,
        )
        .unwrap()
        .denom("uatom")
        .amount(Uint128::new(1_000_000))
        .sender("cosmos1sender")
        .receiver("osmo1receiver")
        .timeout_secs(300)
        .memo("test transfer")
        .build(1000);

        assert_eq!(transfer.source_chain, "cosmoshub-4");
        assert_eq!(transfer.dest_chain, "osmosis-1");
        assert_eq!(transfer.channel, "channel-141");
        assert_eq!(transfer.denom, "uatom");
        assert_eq!(transfer.amount, Uint128::new(1_000_000));
        assert_eq!(transfer.sender, "cosmos1sender");
        assert_eq!(transfer.receiver, "osmo1receiver");
        assert_eq!(transfer.timeout_timestamp, 1300);
        assert_eq!(transfer.memo, Some("test transfer".to_string()));
    }

    #[test]
    fn test_ibc_transfer_builder_all_mainnet_pairs() {
        let registry = ChannelRegistry::with_mainnet_channels();

        // Test all registered pairs
        let pairs = vec![
            ("cosmoshub-4", "osmosis-1", "channel-141"),
            ("osmosis-1", "cosmoshub-4", "channel-0"),
            ("cosmoshub-4", "neutron-1", "channel-569"),
            ("neutron-1", "cosmoshub-4", "channel-1"),
            ("osmosis-1", "neutron-1", "channel-874"),
            ("neutron-1", "osmosis-1", "channel-10"),
            ("cosmoshub-4", "stride-1", "channel-391"),
            ("stride-1", "cosmoshub-4", "channel-0"),
            ("osmosis-1", "stride-1", "channel-326"),
            ("stride-1", "osmosis-1", "channel-5"),
        ];

        for (source, dest, expected_channel) in pairs {
            let builder = IbcTransferBuilder::from_registry(source, dest, &registry);
            assert!(builder.is_ok(), "Failed to create builder for {} -> {}", source, dest);
            assert_eq!(
                builder.unwrap().channel,
                expected_channel,
                "Wrong channel for {} -> {}",
                source,
                dest
            );
        }
    }

    #[test]
    fn test_pfm_memo_single_hop() {
        let hop = PfmHop {
            receiver: "cosmos1receiver".to_string(),
            channel: "channel-0".to_string(),
        };

        let memo = build_pfm_memo(&[hop]);
        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        assert_eq!(parsed["forward"]["receiver"], "cosmos1receiver");
        assert_eq!(parsed["forward"]["channel"], "channel-0");
    }

    #[test]
    fn test_pfm_memo_multi_hop() {
        let hops = vec![
            PfmHop {
                receiver: "osmo1intermediate".to_string(),
                channel: "channel-141".to_string(),
            },
            PfmHop {
                receiver: "cosmos1final".to_string(),
                channel: "channel-0".to_string(),
            },
        ];

        let memo = build_pfm_memo(&hops);
        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        assert_eq!(parsed["forward"]["receiver"], "osmo1intermediate");
        assert_eq!(parsed["forward"]["channel"], "channel-141");
        assert_eq!(parsed["forward"]["next"]["forward"]["receiver"], "cosmos1final");
        assert_eq!(parsed["forward"]["next"]["forward"]["channel"], "channel-0");
    }

    #[test]
    fn test_wasm_memo() {
        let msg = serde_json::json!({
            "swap": {
                "min_output": "1000000"
            }
        });

        let memo = build_wasm_memo("cosmos1contract", &msg, None);
        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        assert_eq!(parsed["wasm"]["contract"], "cosmos1contract");
        assert_eq!(parsed["wasm"]["msg"]["swap"]["min_output"], "1000000");
    }

    #[test]
    fn test_calculate_timeout() {
        assert_eq!(calculate_timeout(&IbcFlowType::SameChain, 60), 60);
        assert_eq!(
            calculate_timeout(&IbcFlowType::DirectIbc { channel: "channel-0".to_string() }, 60),
            120
        );
        assert_eq!(
            calculate_timeout(&IbcFlowType::MultiHopPfm { hops: vec![PfmHop {
                receiver: "test".to_string(),
                channel: "channel-0".to_string(),
            }] }, 60),
            180
        );
        assert_eq!(
            calculate_timeout(&IbcFlowType::IbcHooksWasm {
                contract: "contract".to_string(),
                msg: "{}".to_string(),
            }, 60),
            180
        );
    }

    #[test]
    fn test_determine_flow_with_routing_same_chain() {
        let route_registry = crate::RouteRegistry::with_mainnet_routes();

        let flow = determine_flow_with_routing(
            "cosmoshub-4",
            "cosmoshub-4",
            false,
            &route_registry,
        );

        assert!(matches!(flow, IbcFlowType::SameChain));
    }

    #[test]
    fn test_determine_flow_with_routing_direct() {
        let route_registry = crate::RouteRegistry::with_mainnet_routes();

        let flow = determine_flow_with_routing(
            "cosmoshub-4",
            "osmosis-1",
            false,
            &route_registry,
        );

        match flow {
            IbcFlowType::DirectIbc { channel } => {
                assert_eq!(channel, "channel-141");
            }
            _ => panic!("Expected DirectIbc flow"),
        }
    }

    #[test]
    fn test_determine_flow_with_routing_multi_hop() {
        let route_registry = crate::RouteRegistry::with_mainnet_routes();

        // neutron-1 -> cosmoshub-4 -> stride-1 should use multi-hop
        // (or direct if there's a direct channel)
        let flow = determine_flow_with_routing(
            "neutron-1",
            "stride-1",
            false,
            &route_registry,
        );

        match flow {
            IbcFlowType::DirectIbc { .. } => {
                // Direct route is fine
            }
            IbcFlowType::MultiHopPfm { hops } => {
                // Multi-hop should have at least one hop
                assert!(!hops.is_empty());
            }
            _ => panic!("Expected DirectIbc or MultiHopPfm flow"),
        }
    }

    #[test]
    fn test_determine_flow_with_routing_needs_swap() {
        let route_registry = crate::RouteRegistry::with_mainnet_routes();

        let flow = determine_flow_with_routing(
            "cosmoshub-4",
            "osmosis-1",
            true, // needs swap
            &route_registry,
        );

        assert!(matches!(flow, IbcFlowType::IbcHooksWasm { .. }));
    }

    #[test]
    fn test_multi_hop_timeout_calculation() {
        let multi_hop_flow = IbcFlowType::MultiHopPfm {
            hops: vec![
                PfmHop {
                    receiver: "stride1intermediate".to_string(),
                    channel: "channel-391".to_string(),
                },
                PfmHop {
                    receiver: "osmo1final".to_string(),
                    channel: "channel-5".to_string(),
                },
            ],
        };

        let timeout = calculate_timeout(&multi_hop_flow, 60);
        // 2 hops + base multiplier of 2 = 4 * 60 = 240
        assert_eq!(timeout, 240);
    }

    #[test]
    fn test_multi_hop_pfm_conversion() {
        use crate::routing::{Route, RouteHop};

        let route = Route {
            source_chain: "cosmoshub-4".to_string(),
            dest_chain: "osmosis-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "stride-1".to_string(),
                    channel_id: "channel-391".to_string(),
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "osmosis-1".to_string(),
                    channel_id: "channel-5".to_string(),
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 100000,
        };

        let pfm_hops: Vec<PfmHop> = route.hops.iter().enumerate().map(|(i, hop)| {
            PfmHop {
                receiver: if i == route.hops.len() - 1 {
                    "osmosis-1".to_string()
                } else {
                    hop.chain_id.clone()
                },
                channel: hop.channel_id.clone(),
            }
        }).collect();

        assert_eq!(pfm_hops.len(), 2);
        assert_eq!(pfm_hops[0].receiver, "stride-1");
        assert_eq!(pfm_hops[0].channel, "channel-391");
        assert_eq!(pfm_hops[1].receiver, "osmosis-1");
        assert_eq!(pfm_hops[1].channel, "channel-5");
    }

    #[test]
    fn test_integration_route_to_pfm_memo() {
        use crate::routing::RouteRegistry;

        let route_registry = RouteRegistry::with_mainnet_routes();

        // Find a multi-hop route
        if let Some(route) = route_registry.find_route("neutron-1", "stride-1") {
            if route.hops.len() > 1 {
                // Convert to PfmHops
                let pfm_hops: Vec<PfmHop> = route.hops.iter().enumerate().map(|(i, hop)| {
                    PfmHop {
                        receiver: if i == route.hops.len() - 1 {
                            "stride1finalreceiver".to_string()
                        } else {
                            hop.chain_id.clone()
                        },
                        channel: hop.channel_id.clone(),
                    }
                }).collect();

                // Build PFM memo
                let memo = build_pfm_memo(&pfm_hops);

                // Verify memo is valid JSON
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&memo);
                assert!(parsed.is_ok());

                let parsed = parsed.unwrap();
                assert!(parsed["forward"].is_object());
            }
        }
    }

    #[test]
    fn test_route_registry_with_all_mainnet_pairs() {
        use crate::routing::RouteRegistry;

        let route_registry = RouteRegistry::with_mainnet_routes();

        // Test all known mainnet chain pairs can find routes
        let chains = vec![
            "cosmoshub-4",
            "osmosis-1",
            "neutron-1",
            "stride-1",
        ];

        for source in &chains {
            for dest in &chains {
                if source != dest {
                    let route = route_registry.find_route(source, dest);
                    assert!(
                        route.is_some(),
                        "Should find route from {} to {}",
                        source,
                        dest
                    );
                }
            }
        }
    }
}
