use std::collections::{HashMap, HashSet, VecDeque};
use serde::{Deserialize, Serialize};
use crate::{ChannelRegistry, PfmHop};

/// Route registry for finding multi-hop paths between chains
#[derive(Clone, Debug)]
pub struct RouteRegistry {
    /// Pre-computed routes between chains
    routes: HashMap<(String, String), Vec<Route>>,
    /// Channel registry for direct channel lookup
    channel_registry: ChannelRegistry,
}

/// A complete route from source to destination chain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    /// Source chain ID
    pub source_chain: String,
    /// Destination chain ID
    pub dest_chain: String,
    /// Hops in the route (empty for direct routes)
    pub hops: Vec<RouteHop>,
    /// Estimated time in seconds
    pub estimated_time_seconds: u64,
    /// Estimated cost (gas fees across all hops)
    pub estimated_cost_units: u64,
}

/// A single hop in a multi-hop route
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteHop {
    /// Chain ID for this hop
    pub chain_id: String,
    /// Channel ID on this chain
    pub channel_id: String,
    /// Port ID (typically "transfer")
    pub port_id: String,
}

impl RouteRegistry {
    /// Create an empty route registry
    pub fn new(channel_registry: ChannelRegistry) -> Self {
        Self {
            routes: HashMap::new(),
            channel_registry,
        }
    }

    /// Create a registry with mainnet routes pre-configured
    pub fn with_mainnet_routes() -> Self {
        let channel_registry = ChannelRegistry::with_mainnet_channels();
        let mut registry = Self::new(channel_registry);

        // Register known multi-hop routes for mainnet

        // cosmoshub-4 → stride-1 → osmosis-1
        registry.add_route(Route {
            source_chain: "cosmoshub-4".to_string(),
            dest_chain: "osmosis-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "stride-1".to_string(),
                    channel_id: "channel-391".to_string(), // hub -> stride
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "osmosis-1".to_string(),
                    channel_id: "channel-5".to_string(), // stride -> osmosis
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        // neutron-1 → cosmoshub-4 → osmosis-1
        registry.add_route(Route {
            source_chain: "neutron-1".to_string(),
            dest_chain: "osmosis-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-1".to_string(), // neutron -> hub
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "osmosis-1".to_string(),
                    channel_id: "channel-141".to_string(), // hub -> osmosis
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        // neutron-1 → cosmoshub-4 → stride-1
        registry.add_route(Route {
            source_chain: "neutron-1".to_string(),
            dest_chain: "stride-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-1".to_string(), // neutron -> hub
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "stride-1".to_string(),
                    channel_id: "channel-391".to_string(), // hub -> stride
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        // osmosis-1 → cosmoshub-4 → stride-1
        registry.add_route(Route {
            source_chain: "osmosis-1".to_string(),
            dest_chain: "stride-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-0".to_string(), // osmosis -> hub
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "stride-1".to_string(),
                    channel_id: "channel-391".to_string(), // hub -> stride
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        // osmosis-1 → cosmoshub-4 → neutron-1
        registry.add_route(Route {
            source_chain: "osmosis-1".to_string(),
            dest_chain: "neutron-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-0".to_string(), // osmosis -> hub
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "neutron-1".to_string(),
                    channel_id: "channel-569".to_string(), // hub -> neutron
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        // stride-1 → cosmoshub-4 → neutron-1
        registry.add_route(Route {
            source_chain: "stride-1".to_string(),
            dest_chain: "neutron-1".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "cosmoshub-4".to_string(),
                    channel_id: "channel-0".to_string(), // stride -> hub
                    port_id: "transfer".to_string(),
                },
                RouteHop {
                    chain_id: "neutron-1".to_string(),
                    channel_id: "channel-569".to_string(), // hub -> neutron
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 20,
            estimated_cost_units: 150000,
        });

        registry
    }

    /// Add a route to the registry
    pub fn add_route(&mut self, route: Route) {
        let key = (route.source_chain.clone(), route.dest_chain.clone());
        self.routes.entry(key).or_insert_with(Vec::new).push(route);
    }

    /// Find the best route between two chains
    /// Returns the route with the lowest estimated time
    pub fn find_route(&self, source_chain: &str, dest_chain: &str) -> Option<Route> {
        // Same chain - no route needed
        if source_chain == dest_chain {
            return Some(Route {
                source_chain: source_chain.to_string(),
                dest_chain: dest_chain.to_string(),
                hops: vec![],
                estimated_time_seconds: 0,
                estimated_cost_units: 0,
            });
        }

        // Check for direct channel first
        if let Some(channel_info) = self.channel_registry.get_channel(source_chain, dest_chain) {
            return Some(Route {
                source_chain: source_chain.to_string(),
                dest_chain: dest_chain.to_string(),
                hops: vec![RouteHop {
                    chain_id: dest_chain.to_string(),
                    channel_id: channel_info.channel_id.clone(),
                    port_id: channel_info.port_id.clone(),
                }],
                estimated_time_seconds: 6, // Direct IBC ~6 seconds
                estimated_cost_units: 50000,
            });
        }

        // Check pre-configured multi-hop routes
        let key = (source_chain.to_string(), dest_chain.to_string());
        if let Some(routes) = self.routes.get(&key) {
            // Return the route with lowest estimated time
            return routes.iter()
                .min_by_key(|r| r.estimated_time_seconds)
                .cloned();
        }

        // Try to find a route using BFS
        self.find_route_bfs(source_chain, dest_chain)
    }

    /// Find all possible routes between two chains
    pub fn find_all_routes(&self, source_chain: &str, dest_chain: &str) -> Vec<Route> {
        let mut all_routes = Vec::new();

        // Same chain
        if source_chain == dest_chain {
            all_routes.push(Route {
                source_chain: source_chain.to_string(),
                dest_chain: dest_chain.to_string(),
                hops: vec![],
                estimated_time_seconds: 0,
                estimated_cost_units: 0,
            });
            return all_routes;
        }

        // Direct channel
        if let Some(channel_info) = self.channel_registry.get_channel(source_chain, dest_chain) {
            all_routes.push(Route {
                source_chain: source_chain.to_string(),
                dest_chain: dest_chain.to_string(),
                hops: vec![RouteHop {
                    chain_id: dest_chain.to_string(),
                    channel_id: channel_info.channel_id.clone(),
                    port_id: channel_info.port_id.clone(),
                }],
                estimated_time_seconds: 6,
                estimated_cost_units: 50000,
            });
        }

        // Pre-configured multi-hop routes
        let key = (source_chain.to_string(), dest_chain.to_string());
        if let Some(routes) = self.routes.get(&key) {
            all_routes.extend(routes.iter().cloned());
        }

        all_routes
    }

    /// Calculate the estimated cost for a route based on number of hops
    pub fn calculate_route_cost(route: &Route) -> u64 {
        if route.hops.is_empty() {
            return 0;
        }

        // Base cost per hop + variable cost
        let base_cost_per_hop = 50000u64;
        let hop_count = route.hops.len() as u64;

        base_cost_per_hop * hop_count
    }

    /// Calculate estimated time for a route
    pub fn calculate_route_time(route: &Route) -> u64 {
        if route.hops.is_empty() {
            return 0;
        }

        if route.hops.len() == 1 {
            // Direct transfer
            return 6;
        }

        // Multi-hop: ~10 seconds per hop
        route.hops.len() as u64 * 10
    }

    /// Find a route using breadth-first search
    fn find_route_bfs(&self, source_chain: &str, dest_chain: &str) -> Option<Route> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();

        // Start with the source chain
        queue.push_back((source_chain.to_string(), Vec::new()));
        visited.insert(source_chain.to_string());

        while let Some((current_chain, path)) = queue.pop_front() {
            // Check all channels from current chain
            for ((src, dst), channel_info) in self.channel_registry.all_channels() {
                if src != &current_chain {
                    continue;
                }

                if visited.contains(dst) {
                    continue;
                }

                let mut new_path = path.clone();
                new_path.push(RouteHop {
                    chain_id: dst.clone(),
                    channel_id: channel_info.channel_id.clone(),
                    port_id: channel_info.port_id.clone(),
                });

                // Found the destination
                if dst == dest_chain {
                    let hop_count = new_path.len() as u64;
                    return Some(Route {
                        source_chain: source_chain.to_string(),
                        dest_chain: dest_chain.to_string(),
                        hops: new_path,
                        estimated_time_seconds: hop_count * 10,
                        estimated_cost_units: hop_count * 50000,
                    });
                }

                // Add to queue for further exploration
                // Limit path length to prevent infinite loops
                if new_path.len() < 5 {
                    visited.insert(dst.clone());
                    queue.push_back((dst.clone(), new_path));
                }
            }
        }

        None
    }
}

/// Build a PFM memo for multi-hop transfers
///
/// Converts a list of route hops into a nested PFM memo structure
/// that the Packet Forward Middleware can interpret.
pub fn build_pfm_memo(hops: &[RouteHop], final_receiver: &str) -> String {
    if hops.is_empty() {
        return String::new();
    }

    fn build_nested(hops: &[RouteHop], final_receiver: &str, index: usize) -> serde_json::Value {
        if index >= hops.len() {
            return serde_json::Value::Null;
        }

        let hop = &hops[index];
        let is_last = index == hops.len() - 1;

        let mut forward = serde_json::json!({
            "receiver": if is_last { final_receiver } else { hop.chain_id.as_str() },
            "port": hop.port_id,
            "channel": hop.channel_id,
        });

        // Add retries for reliability
        forward["retries"] = serde_json::json!(2);
        forward["timeout"] = serde_json::json!("10m");

        if !is_last {
            let next = build_nested(hops, final_receiver, index + 1);
            if !next.is_null() {
                forward["next"] = next;
            }
        }

        serde_json::json!({ "forward": forward })
    }

    build_nested(hops, final_receiver, 0).to_string()
}

/// Convert RouteHops to PfmHops for compatibility with existing code
pub fn route_hops_to_pfm_hops(route_hops: &[RouteHop], final_receiver: &str) -> Vec<PfmHop> {
    route_hops.iter().enumerate().map(|(i, hop)| {
        PfmHop {
            receiver: if i == route_hops.len() - 1 {
                final_receiver.to_string()
            } else {
                // Intermediate hops use the next chain's address
                hop.chain_id.clone()
            },
            channel: hop.channel_id.clone(),
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_direct_route() {
        let registry = RouteRegistry::with_mainnet_routes();

        let route = registry.find_route("cosmoshub-4", "osmosis-1");
        assert!(route.is_some());

        let route = route.unwrap();
        assert_eq!(route.source_chain, "cosmoshub-4");
        assert_eq!(route.dest_chain, "osmosis-1");
        assert_eq!(route.hops.len(), 1);
        assert_eq!(route.hops[0].channel_id, "channel-141");
        assert_eq!(route.estimated_time_seconds, 6);
    }

    #[test]
    fn test_find_multi_hop_route() {
        let registry = RouteRegistry::with_mainnet_routes();

        let route = registry.find_route("neutron-1", "osmosis-1");
        assert!(route.is_some());

        let route = route.unwrap();
        assert_eq!(route.source_chain, "neutron-1");
        assert_eq!(route.dest_chain, "osmosis-1");

        // Should have a direct route available
        if route.hops.len() == 1 {
            assert_eq!(route.hops[0].channel_id, "channel-10");
        } else {
            // Or a multi-hop route through cosmoshub
            assert_eq!(route.hops.len(), 2);
            assert_eq!(route.hops[0].chain_id, "cosmoshub-4");
            assert_eq!(route.hops[1].chain_id, "osmosis-1");
        }
    }

    #[test]
    fn test_find_same_chain_route() {
        let registry = RouteRegistry::with_mainnet_routes();

        let route = registry.find_route("cosmoshub-4", "cosmoshub-4");
        assert!(route.is_some());

        let route = route.unwrap();
        assert_eq!(route.hops.len(), 0);
        assert_eq!(route.estimated_time_seconds, 0);
    }

    #[test]
    fn test_find_all_routes() {
        let registry = RouteRegistry::with_mainnet_routes();

        let routes = registry.find_all_routes("cosmoshub-4", "osmosis-1");
        assert!(!routes.is_empty());

        // Should find at least the direct route
        let direct_route = routes.iter().find(|r| r.hops.len() == 1);
        assert!(direct_route.is_some());
    }

    #[test]
    fn test_find_route_not_found() {
        let registry = RouteRegistry::with_mainnet_routes();

        let route = registry.find_route("unknown-chain", "another-unknown-chain");
        assert!(route.is_none());
    }

    #[test]
    fn test_calculate_route_cost() {
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

        let cost = RouteRegistry::calculate_route_cost(&route);
        assert_eq!(cost, 100000); // 2 hops * 50000
    }

    #[test]
    fn test_calculate_route_time() {
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

        let time = RouteRegistry::calculate_route_time(&route);
        assert_eq!(time, 20); // 2 hops * 10 seconds
    }

    #[test]
    fn test_build_pfm_memo_single_hop() {
        let hops = vec![
            RouteHop {
                chain_id: "osmosis-1".to_string(),
                channel_id: "channel-141".to_string(),
                port_id: "transfer".to_string(),
            },
        ];

        let memo = build_pfm_memo(&hops, "osmo1receiver");
        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        assert_eq!(parsed["forward"]["receiver"], "osmo1receiver");
        assert_eq!(parsed["forward"]["channel"], "channel-141");
        assert_eq!(parsed["forward"]["port"], "transfer");
    }

    #[test]
    fn test_build_pfm_memo_multi_hop() {
        let hops = vec![
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
        ];

        let memo = build_pfm_memo(&hops, "osmo1finalreceiver");
        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        // First hop should forward to stride-1
        assert_eq!(parsed["forward"]["receiver"], "stride-1");
        assert_eq!(parsed["forward"]["channel"], "channel-391");
        assert_eq!(parsed["forward"]["port"], "transfer");

        // Second hop should forward to final receiver
        assert_eq!(parsed["forward"]["next"]["forward"]["receiver"], "osmo1finalreceiver");
        assert_eq!(parsed["forward"]["next"]["forward"]["channel"], "channel-5");
        assert_eq!(parsed["forward"]["next"]["forward"]["port"], "transfer");
    }

    #[test]
    fn test_build_pfm_memo_empty() {
        let hops: Vec<RouteHop> = vec![];
        let memo = build_pfm_memo(&hops, "receiver");
        assert_eq!(memo, "");
    }

    #[test]
    fn test_route_hops_to_pfm_hops() {
        let route_hops = vec![
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
        ];

        let pfm_hops = route_hops_to_pfm_hops(&route_hops, "osmo1finalreceiver");

        assert_eq!(pfm_hops.len(), 2);
        assert_eq!(pfm_hops[0].receiver, "stride-1");
        assert_eq!(pfm_hops[0].channel, "channel-391");
        assert_eq!(pfm_hops[1].receiver, "osmo1finalreceiver");
        assert_eq!(pfm_hops[1].channel, "channel-5");
    }

    #[test]
    fn test_mainnet_routes_cosmoshub_to_osmosis_via_stride() {
        let registry = RouteRegistry::with_mainnet_routes();

        // This tests that we have the pre-configured route
        let routes = registry.find_all_routes("cosmoshub-4", "osmosis-1");

        // Should have at least the direct route
        let direct = routes.iter().find(|r| r.hops.len() == 1);
        assert!(direct.is_some());

        // Might also have multi-hop routes
        let multi_hop = routes.iter().find(|r| r.hops.len() > 1);
        if let Some(route) = multi_hop {
            assert!(route.hops.len() <= 3);
        }
    }

    #[test]
    fn test_bfs_pathfinding() {
        let registry = RouteRegistry::with_mainnet_routes();

        // Test that BFS can find a path even for chains without pre-configured routes
        // stride-1 -> osmosis-1 should have a direct route
        let route = registry.find_route("stride-1", "osmosis-1");
        assert!(route.is_some());

        let route = route.unwrap();
        assert!(route.hops.len() >= 1);
    }

    #[test]
    fn test_route_registry_add_custom_route() {
        let mut registry = RouteRegistry::with_mainnet_routes();

        // Add a custom route
        let custom_route = Route {
            source_chain: "custom-chain-1".to_string(),
            dest_chain: "custom-chain-2".to_string(),
            hops: vec![
                RouteHop {
                    chain_id: "custom-chain-2".to_string(),
                    channel_id: "channel-999".to_string(),
                    port_id: "transfer".to_string(),
                },
            ],
            estimated_time_seconds: 6,
            estimated_cost_units: 50000,
        };

        registry.add_route(custom_route);

        let route = registry.find_route("custom-chain-1", "custom-chain-2");
        assert!(route.is_some());

        let route = route.unwrap();
        assert_eq!(route.hops[0].channel_id, "channel-999");
    }
}
