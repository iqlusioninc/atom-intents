/// Example demonstrating multi-hop PFM routing in the ATOM Intent-Based Liquidity System
///
/// This example shows how to:
/// 1. Create a RouteRegistry with mainnet routes
/// 2. Find routes between chains (direct and multi-hop)
/// 3. Build PFM memos for multi-hop transfers
/// 4. Use routing with IBC flow determination
use atom_intents_settlement::{
    build_route_pfm_memo, determine_flow_with_routing, IbcFlowType, Route, RouteHop, RouteRegistry,
};

fn main() {
    println!("=== ATOM Intent-Based Liquidity System - Routing Demo ===\n");

    // 1. Create a route registry with pre-configured mainnet routes
    let route_registry = RouteRegistry::with_mainnet_routes();
    println!("1. Created RouteRegistry with mainnet routes\n");

    // 2. Find direct routes
    println!("2. Finding direct routes:");
    demonstrate_direct_route(&route_registry);
    println!();

    // 3. Find multi-hop routes
    println!("3. Finding multi-hop routes:");
    demonstrate_multi_hop_route(&route_registry);
    println!();

    // 4. Build PFM memos
    println!("4. Building PFM memos:");
    demonstrate_pfm_memo_generation();
    println!();

    // 5. Use routing with IBC flow determination
    println!("5. IBC Flow Determination with Routing:");
    demonstrate_flow_determination(&route_registry);
    println!();

    // 6. Compare all routes between chains
    println!("6. Finding all possible routes:");
    demonstrate_all_routes(&route_registry);
    println!();

    // 7. Route cost and time estimation
    println!("7. Route cost and time estimation:");
    demonstrate_route_metrics(&route_registry);
    println!();

    println!("=== Demo Complete ===");
}

fn demonstrate_direct_route(registry: &RouteRegistry) {
    let route = registry.find_route("cosmoshub-4", "osmosis-1");

    match route {
        Some(route) => {
            println!("  Route from cosmoshub-4 to osmosis-1:");
            println!("    - Hops: {}", route.hops.len());
            println!("    - Estimated time: {}s", route.estimated_time_seconds);
            println!(
                "    - Estimated cost: {} gas units",
                route.estimated_cost_units
            );

            if route.hops.len() == 1 {
                println!("    - Channel: {}", route.hops[0].channel_id);
                println!("    - Type: Direct IBC transfer");
            }
        }
        None => println!("  No route found!"),
    }
}

fn demonstrate_multi_hop_route(registry: &RouteRegistry) {
    // Try to find a multi-hop route
    // neutron-1 -> cosmoshub-4 -> stride-1
    let route = registry.find_route("neutron-1", "stride-1");

    match route {
        Some(route) => {
            println!("  Route from neutron-1 to stride-1:");
            println!("    - Hops: {}", route.hops.len());
            println!("    - Estimated time: {}s", route.estimated_time_seconds);
            println!(
                "    - Estimated cost: {} gas units",
                route.estimated_cost_units
            );

            if route.hops.len() > 1 {
                println!("    - Type: Multi-hop PFM transfer");
                println!("    - Path:");
                for (i, hop) in route.hops.iter().enumerate() {
                    println!(
                        "      {}. {} (channel: {})",
                        i + 1,
                        hop.chain_id,
                        hop.channel_id
                    );
                }
            } else {
                println!("    - Type: Direct IBC transfer");
                println!("    - Channel: {}", route.hops[0].channel_id);
            }
        }
        None => println!("  No route found!"),
    }
}

fn demonstrate_pfm_memo_generation() {
    // Create a multi-hop route manually for demonstration
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

    let final_receiver = "osmo1receiveraddress1234567890";
    let memo = build_route_pfm_memo(&hops, final_receiver);

    println!("  Multi-hop route: cosmoshub-4 → stride-1 → osmosis-1");
    println!("  Final receiver: {}", final_receiver);
    println!("\n  Generated PFM memo:");

    // Pretty print the JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&memo) {
        println!(
            "{}",
            serde_json::to_string_pretty(&parsed).unwrap_or(memo.clone())
        );
    } else {
        println!("{}", memo);
    }
}

fn demonstrate_flow_determination(registry: &RouteRegistry) {
    // Test different flow determinations
    let test_cases = vec![
        ("cosmoshub-4", "cosmoshub-4", false, "Same chain transfer"),
        ("cosmoshub-4", "osmosis-1", false, "Direct IBC transfer"),
        (
            "cosmoshub-4",
            "osmosis-1",
            true,
            "Direct with swap (IBC Hooks)",
        ),
        ("neutron-1", "stride-1", false, "Multi-hop or direct"),
    ];

    for (source, dest, needs_swap, description) in test_cases {
        println!("\n  {} -> {} ({})", source, dest, description);

        let flow = determine_flow_with_routing(source, dest, needs_swap, registry);

        match flow {
            IbcFlowType::SameChain => {
                println!("    Flow: Same chain (no IBC needed)");
            }
            IbcFlowType::DirectIbc { channel } => {
                println!("    Flow: Direct IBC");
                println!("    Channel: {}", channel);
            }
            IbcFlowType::MultiHopPfm { hops } => {
                println!("    Flow: Multi-hop PFM");
                println!("    Number of hops: {}", hops.len());
                for (i, hop) in hops.iter().enumerate() {
                    println!(
                        "      Hop {}: {} via channel {}",
                        i + 1,
                        hop.receiver,
                        hop.channel
                    );
                }
            }
            IbcFlowType::IbcHooksWasm { contract, .. } => {
                println!("    Flow: IBC Hooks with Wasm execution");
                println!("    Contract: {}", contract);
            }
        }
    }
}

fn demonstrate_all_routes(registry: &RouteRegistry) {
    let routes = registry.find_all_routes("cosmoshub-4", "osmosis-1");

    println!("  All routes from cosmoshub-4 to osmosis-1:");
    println!("  Found {} route(s)", routes.len());

    for (i, route) in routes.iter().enumerate() {
        println!("\n  Route {}:", i + 1);
        println!("    - Hops: {}", route.hops.len());
        println!("    - Time: {}s", route.estimated_time_seconds);
        println!("    - Cost: {} gas units", route.estimated_cost_units);

        if route.hops.len() > 1 {
            println!("    - Path:");
            for hop in &route.hops {
                println!("      -> {} ({})", hop.chain_id, hop.channel_id);
            }
        }
    }
}

fn demonstrate_route_metrics(registry: &RouteRegistry) {
    let routes = vec![
        ("cosmoshub-4", "osmosis-1", "Direct route"),
        ("neutron-1", "stride-1", "Multi-hop route"),
        ("osmosis-1", "stride-1", "Alternative route"),
    ];

    for (source, dest, description) in routes {
        if let Some(route) = registry.find_route(source, dest) {
            println!("\n  {} -> {} ({})", source, dest, description);
            println!("    Hops: {}", route.hops.len());

            let calculated_cost = RouteRegistry::calculate_route_cost(&route);
            let calculated_time = RouteRegistry::calculate_route_time(&route);

            println!(
                "    Estimated time: {}s (calculated: {}s)",
                route.estimated_time_seconds, calculated_time
            );
            println!(
                "    Estimated cost: {} gas units (calculated: {})",
                route.estimated_cost_units, calculated_cost
            );

            // Cost per second metric
            if route.estimated_time_seconds > 0 {
                let cost_per_second = route.estimated_cost_units / route.estimated_time_seconds;
                println!("    Efficiency: {} gas units/second", cost_per_second);
            }
        }
    }
}

// Additional utility functions for the demo

#[allow(dead_code)]
fn print_route_summary(route: &Route) {
    println!("Route Summary:");
    println!("  Source: {}", route.source_chain);
    println!("  Destination: {}", route.dest_chain);
    println!("  Hops: {}", route.hops.len());
    println!("  Estimated time: {}s", route.estimated_time_seconds);
    println!("  Estimated cost: {} gas units", route.estimated_cost_units);

    if !route.hops.is_empty() {
        println!("  Path:");
        println!("    {}", route.source_chain);
        for hop in &route.hops {
            println!("    -> {} (via {})", hop.chain_id, hop.channel_id);
        }
    }
}

#[allow(dead_code)]
fn compare_routes(route1: &Route, route2: &Route) {
    println!("Comparing routes:");
    println!(
        "\n  Route 1: {} -> {}",
        route1.source_chain, route1.dest_chain
    );
    println!(
        "    Time: {}s, Cost: {}, Hops: {}",
        route1.estimated_time_seconds,
        route1.estimated_cost_units,
        route1.hops.len()
    );

    println!(
        "\n  Route 2: {} -> {}",
        route2.source_chain, route2.dest_chain
    );
    println!(
        "    Time: {}s, Cost: {}, Hops: {}",
        route2.estimated_time_seconds,
        route2.estimated_cost_units,
        route2.hops.len()
    );

    println!("\n  Comparison:");
    if route1.estimated_time_seconds < route2.estimated_time_seconds {
        println!(
            "    Route 1 is faster by {}s",
            route2.estimated_time_seconds - route1.estimated_time_seconds
        );
    } else if route2.estimated_time_seconds < route1.estimated_time_seconds {
        println!(
            "    Route 2 is faster by {}s",
            route1.estimated_time_seconds - route2.estimated_time_seconds
        );
    } else {
        println!("    Routes have equal time");
    }

    if route1.estimated_cost_units < route2.estimated_cost_units {
        println!(
            "    Route 1 is cheaper by {} gas units",
            route2.estimated_cost_units - route1.estimated_cost_units
        );
    } else if route2.estimated_cost_units < route1.estimated_cost_units {
        println!(
            "    Route 2 is cheaper by {} gas units",
            route1.estimated_cost_units - route2.estimated_cost_units
        );
    } else {
        println!("    Routes have equal cost");
    }
}
