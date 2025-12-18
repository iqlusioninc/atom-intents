# IBC Channel Registry Usage Guide

## Overview

The IBC Channel Registry provides a dynamic channel mapping system for the ATOM Intent-Based Liquidity System. It replaces hardcoded channel IDs with a configurable registry that supports mainnet, testnet, and custom channel configurations.

## Quick Start

### Using the Registry with IbcTransferBuilder

```rust
use atom_intents_settlement::{ChannelRegistry, IbcTransferBuilder};
use cosmwasm_std::Uint128;

// Create a registry with mainnet channels
let registry = ChannelRegistry::with_mainnet_channels();

// Build an IBC transfer using the registry
let transfer = IbcTransferBuilder::from_registry(
    "cosmoshub-4",
    "osmosis-1",
    &registry,
)?
.denom("uatom")
.amount(Uint128::new(1_000_000))
.sender("cosmos1sender...")
.receiver("osmo1receiver...")
.timeout_secs(600)
.build(current_timestamp);
```

### Manual Channel Lookup

```rust
use atom_intents_settlement::ChannelRegistry;

let registry = ChannelRegistry::with_mainnet_channels();

// Get channel info
let channel = registry.get_channel("cosmoshub-4", "osmosis-1");
if let Some(info) = channel {
    println!("Channel ID: {}", info.channel_id);
    println!("Port ID: {}", info.port_id);
    println!("Connection ID: {}", info.connection_id);
}

// Get channel or error
let channel = registry.get_channel_or_error("cosmoshub-4", "osmosis-1")?;

// Check if channel exists
if registry.has_channel("cosmoshub-4", "osmosis-1") {
    // Channel exists
}

// Get reverse channel
let reverse = registry.get_reverse_channel("cosmoshub-4", "osmosis-1");
```

## Pre-configured Channels

### Mainnet Channels

The registry includes these pre-configured mainnet channel pairs:

- **cosmoshub-4 ↔ osmosis-1**
  - Hub → Osmo: channel-141
  - Osmo → Hub: channel-0

- **cosmoshub-4 ↔ neutron-1**
  - Hub → Neutron: channel-569
  - Neutron → Hub: channel-1

- **osmosis-1 ↔ neutron-1**
  - Osmo → Neutron: channel-874
  - Neutron → Osmo: channel-10

- **cosmoshub-4 ↔ stride-1**
  - Hub → Stride: channel-391
  - Stride → Hub: channel-0

- **osmosis-1 ↔ stride-1**
  - Osmo → Stride: channel-326
  - Stride → Osmo: channel-5

### Testnet Channels

```rust
let registry = ChannelRegistry::with_testnet_channels();
```

Includes:
- theta-testnet-001 ↔ osmo-test-5
- theta-testnet-001 ↔ pion-1 (Neutron testnet)

## Custom Channel Registration

### Creating a Custom Registry

```rust
use atom_intents_settlement::{ChannelRegistry, ChannelInfo, ChannelOrdering};

let mut registry = ChannelRegistry::new();

// Register a custom channel
registry.register_channel(
    "chain-a",
    "chain-b",
    ChannelInfo {
        channel_id: "channel-123".to_string(),
        port_id: "transfer".to_string(),
        counterparty_channel_id: "channel-456".to_string(),
        connection_id: "connection-789".to_string(),
        client_id: "07-tendermint-0".to_string(),
        ordering: ChannelOrdering::Unordered,
        version: "ics20-1".to_string(),
    },
);
```

### Extending Mainnet Registry

```rust
let mut registry = ChannelRegistry::with_mainnet_channels();

// Add additional custom channels
registry.register_channel("cosmoshub-4", "custom-chain", custom_channel_info);
```

## Error Handling

```rust
use atom_intents_settlement::{ChannelRegistry, ChannelError};

let registry = ChannelRegistry::new();

match IbcTransferBuilder::from_registry("chain-a", "chain-b", &registry) {
    Ok(builder) => {
        // Use the builder
    }
    Err(ChannelError::ChannelNotFound(source, dest)) => {
        eprintln!("No channel found for {} -> {}", source, dest);
    }
    Err(e) => {
        eprintln!("Channel error: {}", e);
    }
}
```

## Channel Information Structure

Each `ChannelInfo` contains:

```rust
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

    /// Channel ordering (Ordered or Unordered)
    pub ordering: ChannelOrdering,

    /// Channel version (typically "ics20-1" for ICS20)
    pub version: String,
}
```

## Best Practices

1. **Use Mainnet Registry for Production**: Always use `with_mainnet_channels()` for production deployments
2. **Validate Custom Channels**: When adding custom channels, ensure all connection details are correct
3. **Handle Missing Channels**: Always handle the `ChannelNotFound` error case
4. **Bidirectional Registration**: Register both directions of a channel pair for reverse lookups
5. **Keep Registry Updated**: Update the registry when new channels are established or old ones are deprecated

## Integration with Settlement Flow

```rust
use atom_intents_settlement::{ChannelRegistry, IbcTransferBuilder};

pub struct SettlementService {
    channel_registry: ChannelRegistry,
}

impl SettlementService {
    pub fn new() -> Self {
        Self {
            channel_registry: ChannelRegistry::with_mainnet_channels(),
        }
    }

    pub fn create_transfer(
        &self,
        source_chain: &str,
        dest_chain: &str,
        // ... other params
    ) -> Result<IbcTransferInfo, ChannelError> {
        let transfer = IbcTransferBuilder::from_registry(
            source_chain,
            dest_chain,
            &self.channel_registry,
        )?
        .denom(denom)
        .amount(amount)
        .sender(sender)
        .receiver(receiver)
        .build(current_time);

        Ok(transfer)
    }
}
```

## Testing

The channel registry includes comprehensive tests:

```bash
cargo test -p atom-intents-settlement
```

Key test coverage:
- Registry creation (empty, mainnet, testnet)
- Channel registration and lookup
- Error handling for missing channels
- Reverse channel lookup
- Integration with IbcTransferBuilder
- All pre-configured mainnet pairs
