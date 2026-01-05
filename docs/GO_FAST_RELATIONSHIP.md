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

The Go Fast Simulator demonstrates the same **concepts** with a simplified architecture suitable for demos.

### Two Operating Modes

**1. Demo Mode (Simulated Wallet)**
- Quick exploration without real transactions
- Server simulates wallet and escrow
- Instant feedback for learning the flow

**2. Testnet Mode (Keplr Wallet)**
- Connect real Keplr wallet to Cosmos Hub testnet (`provider`)
- Sign actual transactions to deposit funds into escrow contract
- Real on-chain settlement via CosmWasm contracts
- Deployed on Cosmos Hub and Osmosis testnets

### User Flow with Keplr

```
1. Connect Keplr wallet (Cosmos Hub testnet)
2. Fill intent form (input token, output token, amounts)
3. Click "Sign & Deposit to Escrow"
4. Sign transaction in Keplr popup
5. Funds locked in escrow contract on-chain
6. Intent submitted to auction system (off-chain)
7. Solvers compete, winner executes settlement
8. User receives output tokens
```

### Order Submission: Off-chain First, On-chain Fallback

The recommended architecture prioritizes **off-chain order submission** for UX and efficiency, with **on-chain submission as a censorship-resistant fallback**:

| Path | How it works | When to use |
|------|--------------|-------------|
| **Primary (Off-chain)** | User signs escrow deposit → API call to coordinator | Normal operation - fast, low cost |
| **Fallback (On-chain)** | User submits order directly to settlement contract | Coordinator unavailable or censoring |

**Benefits of off-chain primary:**
- Faster UX (no waiting for order confirmation block)
- Lower gas costs (only escrow deposit, not full order data)
- Coordinator can optimize batching and routing

**Benefits of on-chain fallback:**
- Censorship resistance - users can always submit directly
- Trustless - contract enforces order validity
- Auditable - all orders visible on-chain if needed

### Architecture Comparison

| Component | Real Go Fast | Go Fast Simulator |
|-----------|-------------|-------------------|
| **Order Submission** | On-chain `submitOrder` | Off-chain API + on-chain fallback |
| **Escrow** | Built into Go Fast contract | Separate CosmWasm escrow contract |
| **Solver Discovery** | Monitor chain events | WebSocket subscription + on-chain events |
| **Order Matching** | Solvers compete on-chain | Server-side batch auction |
| **Fulfillment** | On-chain `fillOrder` call | Server executes settlement |
| **Settlement** | `initiateSettlement` + Hyperlane | CosmWasm settlement contract |
| **Cross-chain** | Hyperlane messaging | IBC (planned) |

### On-Chain Order Fallback (Implemented)

The settlement contract now supports direct on-chain order submission as a censorship-resistant fallback:

```rust
// User submits order with funds locked in contract
ExecuteMsg::SubmitOrder {
    min_output_amount: Uint128,
    output_denom: String,
    destination_chain: String,
    recipient: String,
    timeout_seconds: u64,
}

// Solver fills order (sends output funds)
ExecuteMsg::FillOrder { order_id: String }

// User cancels unfilled order
ExecuteMsg::CancelOrder { order_id: String }

// Anyone can refund expired orders
ExecuteMsg::RefundExpiredOrder { order_id: String }
```

**Order lifecycle:**
1. User calls `SubmitOrder` with input tokens attached
2. Contract generates order ID (SHA256 hash) and locks funds
3. Solvers query `OpenOrders` to find fillable orders
4. Solver calls `FillOrder` with output tokens to complete trade
5. If timeout expires, anyone can call `RefundExpiredOrder`

### Deployed Contracts

**Cosmos Hub Testnet (`provider`):**
- Settlement: `cosmos1xwft7w6kcspzufftw6ky4f5e8sykumpuenpm34tkxk4epmya0jdsahgsff`
- Escrow: `cosmos13jv2umdqvlkfncpd6vf7r2sc0ljdtenmzujlpqqpgagarassqsws86phq9`

**Osmosis Testnet (`osmo-test-5`):**
- Settlement: `osmo1qdfc5yhptjfv2puzlde49hjkdhqpllenka4t352henukw9vzkwvqafnzv3`
- Escrow: `osmo1gcgcng338wnwwmcwklqa2830qwhww96zg9rl3g4kf7mw89mljfcqe98wu7`

### Why This Architecture?

1. **Real wallet experience**: Users sign actual transactions with Keplr
2. **On-chain escrow**: Funds are locked in auditable smart contracts
3. **Testnet safety**: Uses testnet tokens, no real value at risk
4. **Demo flexibility**: Can also run in simulated mode for quick demos
5. **Educational**: Shows the full intent lifecycle with real transactions

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

1. ~~**On-chain order fallback**~~ ✅ - `SubmitOrder`, `FillOrder`, `CancelOrder`, `RefundExpiredOrder` implemented
2. **Solver event monitoring** - Solvers watch both API and on-chain events
3. **Hyperlane integration** - Add Hyperlane for cross-chain messaging verification
4. **IBC settlement** - Enable cross-chain asset movement via IBC
5. **Solver SDK** - Provide tools for running independent solver nodes
6. **Decentralized coordinator** - Run coordinator in TEE or as validator set

## References

- [Skip Go Fast Documentation](https://docs.skip.build/go/advanced-transfer/go-fast)
- [Go Fast Contracts](https://github.com/skip-mev/go-fast-contracts)
- [Go Fast Solver Reference](https://github.com/skip-mev/skip-go-fast-solver)
- [Skip Protocol](https://www.skip.build/)
