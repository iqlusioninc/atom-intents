# Go Fast Simulator: Relationship to Skip Protocol

## Overview

The Go Fast Simulator is a demonstration implementation inspired by [Skip Protocol's Go Fast](https://docs.skip.build/go/advanced-transfer/go-fast) intent-based cross-chain transfer system. This document explains the relationship between the real Go Fast protocol and our simulator, including architectural differences and design decisions.

## What is Skip Go Fast?

Skip Go Fast is a decentralized bridging protocol that accelerates cross-chain transactions through an intent-based solver system:

1. **Users submit intents** - Call `submitOrder` on a source chain smart contract
2. **Solvers fulfill orders** - Monitor events, evaluate risk/reward, call `fillOrder` on destination chain
3. **Settlement occurs** - Solvers call `initiateSettlement` to recover funds via cross-chain messaging (Hyperlane)

Key characteristics:
- On-chain order submission and fulfillment
- Permissionless solver network
- Cross-chain verification via Hyperlane
- Currently supports EVM chains to IBC-connected chains

## Our Simulator

The Go Fast Simulator demonstrates the same **concepts** with a simplified architecture suitable for demos:

### Architecture Comparison

| Component | Real Go Fast | Go Fast Simulator |
|-----------|-------------|-------------------|
| **Order Submission** | On-chain `submitOrder` to smart contract | HTTP POST `/api/v1/intents` |
| **Solver Discovery** | Monitor chain events | WebSocket subscription |
| **Order Matching** | Solvers compete on-chain | Server-side batch auction |
| **Fulfillment** | On-chain `fillOrder` call | Server executes settlement |
| **Settlement** | `initiateSettlement` + Hyperlane | CosmWasm escrow contract |
| **Cross-chain** | Hyperlane messaging | IBC (planned) |

### Why the Differences?

1. **Demo UX**: Users can create intents without MetaMask/Keplr transactions for initial exploration
2. **Simplicity**: No need to deploy Go Fast contracts across multiple chains
3. **Iteration Speed**: API changes don't require contract migrations
4. **Educational**: Clear separation of concerns for understanding the flow

## API Mapping

### Intent Submission

**Real Go Fast:**
```solidity
// On source chain contract
function submitOrder(
    bytes32 sender,
    bytes32 recipient,
    uint256 amountIn,
    uint256 amountOut,
    uint32 destinationDomain,
    uint64 timeoutTimestamp,
    bytes calldata data
) external payable returns (bytes32 orderId);
```

**Our Simulator:**
```typescript
POST /api/v1/intents
{
  "sender": "cosmos1...",
  "intent_type": "swap",
  "input_token": "ATOM",
  "input_amount": "100.0",
  "output_token": "OSMO",
  "min_output_amount": "95.0",
  "destination_chain": "osmosis-1",
  "timeout_seconds": 300
}
```

### Solver Quotes

**Real Go Fast:** Solvers monitor events and submit competing `fillOrder` transactions on-chain.

**Our Simulator:** Solvers receive intents via WebSocket and submit quotes via API:
```typescript
WebSocket /ws -> Receive: { type: "new_intent", intent: {...} }
POST /api/v1/quotes -> { intent_id, output_amount, execution_time_ms }
```

### Settlement

**Real Go Fast:**
```solidity
// On destination chain
function initiateSettlement(
    bytes32[] calldata orderIds,
    address repaymentAddress
) external;
```

**Our Simulator:** Server-side settlement via CosmWasm escrow contract:
```rust
// ExecuteMsg::SettleEscrow
{
  "settle_escrow": {
    "escrow_id": "...",
    "recipient": "cosmos1...",
    "amount": "1000000"
  }
}
```

## Shared Concepts

Despite implementation differences, both systems share these core concepts:

1. **Intent-based execution** - Users express desired outcomes, not execution paths
2. **Competitive solver network** - Multiple solvers compete to fill orders
3. **MEV protection** - Batch auctions prevent front-running
4. **Cross-chain asset movement** - Assets move between chains atomically
5. **Timeout guarantees** - Unfilled intents return funds to users

## Roadmap to Alignment

Future work could align the simulator more closely with Go Fast:

1. **Deploy Go Fast contracts** - Use actual Go Fast CosmWasm contracts
2. **Hyperlane integration** - Add Hyperlane for cross-chain messaging
3. **On-chain submission** - Require wallet transactions for intent creation
4. **Solver SDK** - Provide tools for running independent solver nodes

## References

- [Skip Go Fast Documentation](https://docs.skip.build/go/advanced-transfer/go-fast)
- [Go Fast Contracts](https://github.com/skip-mev/go-fast-contracts)
- [Go Fast Solver Reference](https://github.com/skip-mev/skip-go-fast-solver)
- [Skip Protocol](https://www.skip.build/)
