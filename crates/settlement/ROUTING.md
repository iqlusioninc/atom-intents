# Multi-hop PFM Routing for ATOM Intent-Based Liquidity System

## Overview

This implementation adds proper multi-hop Packet Forward Middleware (PFM) routing to the ATOM Intent-Based Liquidity System. Previously, multi-hop routing returned empty hop arrays. Now the system can intelligently find and use multi-hop routes between Cosmos chains.

## Key Components

### 1. RouteRegistry (`routing.rs`)

The core routing engine that finds optimal paths between chains.

**Key Features:**
- Pre-configured mainnet routes for common multi-hop paths
- BFS pathfinding algorithm for discovering routes dynamically
- Route cost and time estimation
- Support for both direct and multi-hop transfers

**API:**
```rust
// Create registry with mainnet routes
let registry = RouteRegistry::with_mainnet_routes();

// Find best route
let route = registry.find_route("neutron-1", "stride-1");

// Find all possible routes
let routes = registry.find_all_routes("cosmoshub-4", "osmosis-1");

// Add custom routes
registry.add_route(custom_route);
```

### 2. Route Types

**Route**: Complete path from source to destination
```rust
pub struct Route {
    pub source_chain: String,
    pub dest_chain: String,
    pub hops: Vec<RouteHop>,
    pub estimated_time_seconds: u64,
    pub estimated_cost_units: u64,
}
```

**RouteHop**: Single hop in a multi-hop route
```rust
pub struct RouteHop {
    pub chain_id: String,
    pub channel_id: String,
    pub port_id: String,
}
```

### 3. PFM Memo Generation

Builds nested JSON structures for PFM:

```rust
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

let memo = build_route_pfm_memo(&hops, "osmo1receiver");
```

Generates:
```json
{
  "forward": {
    "receiver": "stride-1",
    "port": "transfer",
    "channel": "channel-391",
    "retries": 2,
    "timeout": "10m",
    "next": {
      "forward": {
        "receiver": "osmo1receiver",
        "port": "transfer",
        "channel": "channel-5",
        "retries": 2,
        "timeout": "10m"
      }
    }
  }
}
```

### 4. IBC Flow Determination

Updated `determine_flow_with_routing()` function that uses RouteRegistry:

```rust
let flow = determine_flow_with_routing(
    "neutron-1",
    "stride-1",
    false, // needs_swap
    &route_registry,
);

match flow {
    IbcFlowType::SameChain => { /* same chain */ }
    IbcFlowType::DirectIbc { channel } => { /* direct IBC */ }
    IbcFlowType::MultiHopPfm { hops } => { /* multi-hop PFM */ }
    IbcFlowType::IbcHooksWasm { .. } => { /* IBC hooks */ }
}
```

## Pre-configured Mainnet Routes

The following multi-hop routes are pre-configured:

1. **cosmoshub-4 → stride-1 → osmosis-1**
   - Time: ~20s, Cost: 150k gas units

2. **neutron-1 → cosmoshub-4 → osmosis-1**
   - Time: ~20s, Cost: 150k gas units

3. **neutron-1 → cosmoshub-4 → stride-1**
   - Time: ~20s, Cost: 150k gas units

4. **osmosis-1 → cosmoshub-4 → stride-1**
   - Time: ~20s, Cost: 150k gas units

5. **osmosis-1 → cosmoshub-4 → neutron-1**
   - Time: ~20s, Cost: 150k gas units

6. **stride-1 → cosmoshub-4 → neutron-1**
   - Time: ~20s, Cost: 150k gas units

## Usage Examples

### Basic Route Finding

```rust
use atom_intents_settlement::{RouteRegistry, determine_flow_with_routing};

let registry = RouteRegistry::with_mainnet_routes();

// Find route
let route = registry.find_route("neutron-1", "osmosis-1").unwrap();

println!("Route has {} hops", route.hops.len());
println!("Estimated time: {}s", route.estimated_time_seconds);
```

### Building IBC Transfers with Routing

```rust
let route_registry = RouteRegistry::with_mainnet_routes();

// Determine the flow type
let flow = determine_flow_with_routing(
    "neutron-1",
    "osmosis-1",
    false,
    &route_registry,
);

// Calculate appropriate timeout
let timeout = calculate_timeout(&flow, 60);

match flow {
    IbcFlowType::MultiHopPfm { hops } => {
        let memo = build_pfm_memo(&hops);
        // Use memo in IBC transfer
    }
    _ => { /* handle other flow types */ }
}
```

### Custom Routes

```rust
let mut registry = RouteRegistry::with_mainnet_routes();

// Add a custom route
registry.add_route(Route {
    source_chain: "custom-chain-1".to_string(),
    dest_chain: "custom-chain-2".to_string(),
    hops: vec![
        RouteHop {
            chain_id: "intermediate-chain".to_string(),
            channel_id: "channel-123".to_string(),
            port_id: "transfer".to_string(),
        },
        RouteHop {
            chain_id: "custom-chain-2".to_string(),
            channel_id: "channel-456".to_string(),
            port_id: "transfer".to_string(),
        },
    ],
    estimated_time_seconds: 25,
    estimated_cost_units: 200000,
});
```

## Running the Demo

A comprehensive demo is available:

```bash
cargo run -p atom-intents-settlement --example routing_demo
```

The demo shows:
- Direct route finding
- Multi-hop route finding
- PFM memo generation
- IBC flow determination
- Route comparison and metrics

## Testing

All routing functionality is fully tested:

```bash
cargo test -p atom-intents-settlement
```

**Test Coverage:**
- ✅ Direct route finding
- ✅ Multi-hop route finding
- ✅ Same-chain detection
- ✅ PFM memo generation (single and multi-hop)
- ✅ Route cost calculation
- ✅ Route time estimation
- ✅ BFS pathfinding
- ✅ Custom route registration
- ✅ Integration with IBC flow determination
- ✅ All mainnet chain pair routing

Total: 65 tests passing

## Architecture Decisions

### Why BFS for Pathfinding?

Breadth-first search ensures we find the shortest path (minimum hops) when no pre-configured route exists. This is important for:
- Minimizing transfer time
- Reducing gas costs
- Improving reliability (fewer hops = fewer failure points)

### Why Pre-configure Common Routes?

While BFS can find routes dynamically, pre-configured routes allow:
- Optimization of common paths
- Manual override of BFS defaults
- Explicit cost/time estimates based on real-world data
- Better control over preferred routing

### Timeout Calculation

Timeouts are calculated based on hop count:
- Same chain: 1x base timeout
- Direct IBC: 2x base timeout
- Multi-hop: (2 + hop_count) × base timeout
- IBC Hooks: 3x base timeout

This ensures adequate time for complex multi-hop transfers while not waiting unnecessarily for simple transfers.

## Future Enhancements

Potential improvements:

1. **Dynamic Route Discovery**: Query chain registries for channels
2. **Route Caching**: Cache discovered routes to reduce computation
3. **Route Preferences**: Allow users to prefer faster vs. cheaper routes
4. **Gas Price Integration**: Factor in actual gas prices for cost estimation
5. **Failure Recovery**: Automatic route re-calculation on transfer failure
6. **Route Analytics**: Track success rates and adjust preferences
7. **Noble Integration**: Add routes through Noble for USDC transfers
8. **Testnet Routes**: Expand pre-configured testnet routes

## API Reference

### RouteRegistry Methods

- `new(channel_registry)` - Create empty registry
- `with_mainnet_routes()` - Create with pre-configured mainnet routes
- `find_route(source, dest)` - Find best route
- `find_all_routes(source, dest)` - Find all possible routes
- `add_route(route)` - Add custom route
- `calculate_route_cost(route)` - Calculate route cost
- `calculate_route_time(route)` - Calculate route time

### IBC Functions

- `determine_flow_with_routing(source, dest, needs_swap, registry)` - Determine IBC flow type
- `calculate_timeout(flow_type, base_timeout)` - Calculate appropriate timeout
- `build_route_pfm_memo(hops, final_receiver)` - Build PFM memo from route hops

## Migration Guide

### From Legacy `determine_flow`

**Before:**
```rust
let flow = determine_flow(source, dest, needs_swap, &channel_map);
// Returns MultiHopPfm { hops: vec![] } for multi-hop
```

**After:**
```rust
let registry = RouteRegistry::with_mainnet_routes();
let flow = determine_flow_with_routing(source, dest, needs_swap, &registry);
// Returns MultiHopPfm with actual hops populated
```

The legacy `determine_flow` function is still available for backwards compatibility but should be migrated to `determine_flow_with_routing` for proper multi-hop support.
