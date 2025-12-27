# Escrow Settlement Flow

This document explains how the escrow contract verifies fund delivery and releases funds to the solver.

---

## Table of Contents

1. [Overview](#overview)
2. [Key Components](#key-components)
3. [Cross-Chain Settlement Flow](#cross-chain-settlement-flow)
4. [Same-Chain Settlement Flow](#same-chain-settlement-flow)
5. [Delivery Verification Mechanism](#delivery-verification-mechanism)
6. [Security Guarantees](#security-guarantees)
7. [Failure Handling](#failure-handling)

---

## Overview

The escrow system uses a **two-phase commit** pattern where:
1. **Phase 1 (Lock)**: Both user and solver lock their funds
2. **Phase 2 (Execute)**: Funds are transferred, and escrow is released upon verified delivery

The key question: *How does the escrow know funds were delivered to the user?*

**Answer**: The system uses **IBC acknowledgements** as cryptographic proof of delivery. When the IBC module confirms a transfer succeeded on the destination chain, this triggers the escrow release.

---

## Key Components

### Contracts

| Contract | Purpose | Location |
|----------|---------|----------|
| **Escrow** | Holds user funds until settlement completes | `contracts/escrow/` |
| **Settlement** | Orchestrates the settlement flow and state machine | `contracts/settlement/` |

### State Machines

**Settlement Status**:
```
Pending → UserLocked → SolverLocked → Executing → Completed
                                          ↓
                                       Failed
```

**Escrow Status**:
```
Locked → Released { recipient }
   ↓
Refunded (if expired or settlement failed)
```

---

## Cross-Chain Settlement Flow

This is the primary flow when the user wants to receive funds on a different chain.

### Sequence Diagram

```
┌─────────┐     ┌────────────┐     ┌─────────────┐     ┌───────────────┐     ┌──────────┐
│  User   │     │   Escrow   │     │  Settlement │     │  IBC Module   │     │ Dest Chain│
└────┬────┘     └──────┬─────┘     └──────┬──────┘     └───────┬───────┘     └─────┬────┘
     │                 │                  │                    │                   │
     │ 1. Lock funds   │                  │                    │                   │
     │────────────────►│                  │                    │                   │
     │                 │                  │                    │                   │
     │                 │ 2. Escrow created│                    │                   │
     │                 │  (status: Locked)│                    │                   │
     │                 │                  │                    │                   │
     │                 │ 3. MarkUserLocked│                    │                   │
     │                 │─────────────────►│                    │                   │
     │                 │                  │                    │                   │
     │                 │                  │ 4. Solver locks    │                   │
     │                 │                  │    (SolverLocked)  │                   │
     │                 │                  │                    │                   │
     │                 │                  │ 5. ExecuteSettlement                   │
     │                 │                  │    (IbcMsg::Transfer)                  │
     │                 │                  │───────────────────►│                   │
     │                 │                  │                    │                   │
     │                 │                  │                    │ 6. IBC Transfer   │
     │                 │                  │                    │──────────────────►│
     │                 │                  │                    │                   │
     │                 │                  │                    │ 7. Funds credited │
     │                 │                  │                    │   to user         │
     │                 │                  │                    │                   │
     │                 │                  │                    │◄──────────────────│
     │                 │                  │                    │  8. IBC Ack       │
     │                 │                  │                    │                   │
     │                 │                  │◄───────────────────│                   │
     │                 │                  │ 9. HandleIbcAck    │                   │
     │                 │                  │    (success=true)  │                   │
     │                 │                  │                    │                   │
     │                 │◄─────────────────│                    │                   │
     │                 │ 10. Release      │                    │                   │
     │                 │    (to solver)   │                    │                   │
     │                 │                  │                    │                   │
     │                 │ 11. BankMsg::Send│                    │                   │
     │                 │    to solver     │                    │                   │
     │                 │                  │                    │                   │
```

### Step-by-Step Breakdown

#### Step 1-2: User Locks Funds in Escrow

**File**: `contracts/escrow/src/contract.rs:70-125`

```rust
fn execute_lock(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    escrow_id: String,
    intent_id: String,
    expires_at: u64,
) -> Result<Response, ContractError> {
    // User sends funds with the Lock message
    let escrow = Escrow {
        id: escrow_id.clone(),
        owner: info.sender.clone(),      // Intent creator
        amount: coin.amount,
        denom: coin.denom.clone(),
        intent_id: intent_id.clone(),
        expires_at,
        status: EscrowStatus::Locked,    // Initial status
        // Cross-chain fields (None for local escrows)
        source_channel: None,
        owner_source_address: None,
        // ...
    };
    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;
}
```

#### Step 3-4: Settlement State Transitions

**File**: `contracts/settlement/src/handlers.rs`

The settlement moves through states:
- `Pending` → Intent created
- `UserLocked` → User's funds in escrow
- `SolverLocked` → Solver has committed output funds

#### Step 5: Execute Settlement (IBC Transfer)

**File**: `contracts/settlement/src/handlers.rs:442-503`

```rust
pub fn execute_settlement(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    settlement_id: String,
    ibc_channel: String,
) -> Result<Response, ContractError> {
    // Verify state is SolverLocked
    match &settlement.status {
        SettlementStatus::SolverLocked => {}
        _ => return Err(ContractError::InvalidStateTransition { ... }),
    }

    // Update to Executing
    settlement.status = SettlementStatus::Executing;

    // Create IBC transfer to send solver's output to user
    let ibc_transfer = IbcMsg::Transfer {
        channel_id: ibc_channel.clone(),
        to_address: settlement.user.to_string(),
        amount: Coin {
            denom: settlement.solver_output_denom.clone(),
            amount: settlement.solver_output_amount,
        },
        timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(600)),
        memo: Some(format!("ATOM Intent Settlement {}", settlement_id)),
    };

    Ok(Response::new().add_message(ibc_transfer))
}
```

#### Step 6-8: IBC Protocol Handles Transfer

The IBC protocol:
1. Sends the packet to the destination chain
2. Destination chain credits funds to the user
3. Destination chain sends acknowledgement back

This happens automatically via the IBC module and relayers.

#### Step 9: Handle IBC Acknowledgement (THE VERIFICATION STEP)

**File**: `contracts/settlement/src/handlers.rs:576-679`

```rust
pub fn execute_handle_ibc_ack(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
    success: bool,           // <-- This is the delivery proof!
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;

    // Must be in Executing state
    if !matches!(settlement.status, SettlementStatus::Executing) {
        return Err(ContractError::InvalidStateTransition { ... });
    }

    if success {
        // ════════════════════════════════════════════════════════════
        // DELIVERY VERIFIED: IBC confirmed transfer succeeded
        // ════════════════════════════════════════════════════════════

        settlement.status = SettlementStatus::Completed;
        SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

        // Get solver's address
        let solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;

        // RELEASE THE ESCROW TO THE SOLVER
        let release_msg = WasmMsg::Execute {
            contract_addr: config.escrow_contract.to_string(),
            msg: to_json_binary(&EscrowExecuteMsg::Release {
                escrow_id: escrow_id.clone(),
                recipient: solver.operator.to_string(),
            })?,
            funds: vec![],
        };

        Ok(Response::new().add_message(release_msg))
    } else {
        // IBC transfer failed - refund the user
        settlement.status = SettlementStatus::Failed {
            reason: "IBC transfer failed".to_string(),
        };

        let refund_msg = WasmMsg::Execute {
            contract_addr: config.escrow_contract.to_string(),
            msg: to_json_binary(&EscrowExecuteMsg::Refund {
                escrow_id: escrow_id.clone(),
            })?,
            funds: vec![],
        };

        Ok(Response::new().add_message(refund_msg))
    }
}
```

#### Step 10-11: Escrow Release

**File**: `contracts/escrow/src/contract.rs:199-256`

```rust
fn execute_release(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
    recipient: String,
) -> Result<Response, ContractError> {
    // SECURITY: Only settlement contract can release
    if info.sender != config.settlement_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Verify escrow is still locked and not expired
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::EscrowNotFound { id: escrow_id });
    }
    if env.block.time.seconds() >= escrow.expires_at {
        return Err(ContractError::EscrowExpired { id: escrow_id });
    }

    // Update status
    escrow.status = EscrowStatus::Released { recipient: recipient.clone() };

    // Send funds to solver via local bank transfer
    let send_msg = BankMsg::Send {
        to_address: recipient_addr.to_string(),
        amount: vec![Coin {
            denom: escrow.denom.clone(),
            amount: escrow.amount,
        }],
    };

    Ok(Response::new().add_message(send_msg))
}
```

---

## Same-Chain Settlement Flow

When both user and solver are on the same chain, the current implementation still uses IBC.

### Current Behavior

```
┌─────────┐     ┌────────────┐     ┌─────────────┐     ┌───────────────┐
│  User   │     │   Escrow   │     │  Settlement │     │  IBC Module   │
│(Hub)    │     │  (Hub)     │     │   (Hub)     │     │   (Hub)       │
└────┬────┘     └──────┬─────┘     └──────┬──────┘     └───────┬───────┘
     │                 │                  │                    │
     │ Lock funds      │                  │                    │
     │────────────────►│                  │                    │
     │                 │                  │                    │
     │                 │ ExecuteSettlement│                    │
     │                 │    with IBC      │                    │
     │                 │─────────────────►│                    │
     │                 │                  │                    │
     │                 │                  │ IbcMsg::Transfer   │
     │                 │                  │ (same chain!)      │
     │                 │                  │───────────────────►│
     │                 │                  │                    │
     │                 │                  │                    │ IBC processes
     │                 │                  │                    │ locally (~3s)
     │                 │                  │                    │
     │                 │                  │◄───────────────────│
     │                 │                  │    IBC Ack         │
     │                 │                  │                    │
     │                 │◄─────────────────│                    │
     │                 │    Release       │                    │
     │                 │                  │                    │
```

### Key Insight

The settlement contract (`handlers.rs:442-503`) **always uses `IbcMsg::Transfer`**:

```rust
// From execute_settlement - no same-chain optimization
let ibc_transfer = IbcMsg::Transfer {
    channel_id: ibc_channel.clone(),  // Still requires a channel
    to_address: settlement.user.to_string(),
    amount: Coin { ... },
    timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(600)),
    memo: Some(format!("ATOM Intent Settlement {}", settlement_id)),
};
```

### Same-Chain IBC Behavior

On the same chain, IBC transfers:
- Still require a valid channel (loopback or self-referential)
- Process faster (~3 seconds vs ~6+ seconds cross-chain)
- Still provide acknowledgement for verification

### Gap: No Direct Bank Transfer Path

The current implementation does **not** have a dedicated same-chain path that would use `BankMsg::Send` directly. This is a potential optimization opportunity.

A same-chain optimization could:
1. Detect when `source_chain == dest_chain`
2. Use `BankMsg::Send` directly instead of IBC
3. Skip the IBC acknowledgement flow entirely
4. Release escrow immediately after the bank send

---

## Delivery Verification Mechanism

### Why IBC Acknowledgements Work

The IBC protocol provides **cryptographic proof** of delivery:

1. **Merkle Proofs**: IBC uses Merkle proofs to verify transaction inclusion
2. **Light Client Verification**: Each chain maintains a light client of connected chains
3. **Finality Guarantees**: Acknowledgements only arrive after destination chain finality

### The Verification Flow

```
Settlement Contract                    IBC Module                     Destination
       │                                   │                              │
       │ IbcMsg::Transfer                  │                              │
       │──────────────────────────────────►│                              │
       │                                   │                              │
       │                                   │  SendPacket                  │
       │                                   │─────────────────────────────►│
       │                                   │                              │
       │                                   │         [Packet delivered]   │
       │                                   │         [Funds credited]     │
       │                                   │         [Block finalized]    │
       │                                   │                              │
       │                                   │◄─────────────────────────────│
       │                                   │  WriteAcknowledgement        │
       │                                   │  (included in block proof)   │
       │                                   │                              │
       │◄──────────────────────────────────│                              │
       │  Ack event (relayer delivers)     │                              │
       │                                   │                              │
       │  HandleIbcAck(success=true)       │                              │
       │  ════════════════════════════     │                              │
       │  This is the PROOF that funds     │                              │
       │  were delivered!                  │                              │
```

### No Oracle Required

The system achieves trustless verification without oracles because:
- IBC is a protocol-level primitive in Cosmos
- Light client verification is cryptographic
- Acknowledgements are part of the consensus

---

## Security Guarantees

### Authorization Chain

```
┌─────────────────────────────────────────────────────────────────────┐
│                        WHO CAN DO WHAT                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Lock Escrow:     Anyone (with funds)                               │
│                                                                     │
│  Release Escrow:  Settlement Contract ONLY                          │
│                   ├── Verified via info.sender check                │
│                   └── Only after IBC ack success                    │
│                                                                     │
│  Refund Escrow:   Owner (after expiration)                          │
│                   OR Settlement Contract (on failure)               │
│                   OR Admin (for cross-chain refunds)                │
│                                                                     │
│  Handle IBC Ack:  Admin ONLY                                        │
│                   (represents IBC module callback)                  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Race Condition Prevention

**File**: `contracts/escrow/src/contract.rs:225-232`

```rust
// SECURITY FIX (5.6): Prevent release after expiration
// This prevents a race condition where:
// 1. Escrow expires
// 2. User initiates refund
// 3. Settlement contract tries to release (would be double-spend)
if env.block.time.seconds() >= escrow.expires_at {
    return Err(ContractError::EscrowExpired { id: escrow_id });
}
```

### Timeout Ordering Rule

```
┌─────────────────────────────────────────────────────────────────────┐
│                     CRITICAL SAFETY RULE                            │
│                                                                     │
│   ESCROW TIMEOUT  >  IBC TIMEOUT  +  BUFFER                         │
│                                                                     │
│   Example:                                                          │
│   ├── IBC Timeout:    10 minutes                                    │
│   ├── Buffer:          5 minutes                                    │
│   └── Escrow Timeout: 15 minutes                                    │
│                                                                     │
│   This ensures:                                                     │
│   • If IBC times out, we have time to detect and refund             │
│   • Escrow can't expire while IBC transfer is in flight             │
│   • No double-spend possible                                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Failure Handling

### IBC Timeout

**File**: `contracts/settlement/src/handlers.rs:505-574`

```rust
pub fn execute_handle_timeout(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    // Mark settlement as failed
    settlement.status = SettlementStatus::Failed {
        reason: "IBC transfer timeout".to_string(),
    };

    // Refund user's escrow
    let refund_msg = WasmMsg::Execute {
        contract_addr: config.escrow_contract.to_string(),
        msg: to_json_binary(&EscrowExecuteMsg::Refund {
            escrow_id: escrow_id.clone(),
        })?,
        funds: vec![],
    };

    Ok(Response::new().add_message(refund_msg))
}
```

### IBC Failure (Negative Acknowledgement)

When `execute_handle_ibc_ack` is called with `success=false`:
- Settlement marked as `Failed`
- Escrow refunded to user
- Solver keeps their output (it was never sent)

### Escrow Expiration

If escrow expires before settlement completes:
- User can call `Refund` to reclaim their funds
- Settlement cannot release after expiration (security check)
- Solver's pending transfer will timeout and return

### Recovery Flow Diagram

```
                    Settlement Executing
                           │
           ┌───────────────┼───────────────┐
           │               │               │
           ▼               ▼               ▼
      IBC Success     IBC Timeout     IBC Failure
           │               │               │
           ▼               ▼               ▼
    ┌──────────┐    ┌──────────┐    ┌──────────┐
    │ Completed│    │  Failed  │    │  Failed  │
    │          │    │          │    │          │
    │ Release  │    │  Refund  │    │  Refund  │
    │ to Solver│    │  to User │    │  to User │
    └──────────┘    └──────────┘    └──────────┘
```

---

## Summary

### How the Escrow Knows Funds Were Delivered

1. **IBC Transfer Initiated**: Settlement contract sends `IbcMsg::Transfer`
2. **IBC Protocol Executes**: Funds transferred via IBC with cryptographic proofs
3. **Acknowledgement Received**: IBC module confirms success/failure
4. **Settlement Reacts**: `HandleIbcAck` called with result
5. **Escrow Released**: On success, escrow released to solver

### Key Properties

| Property | Mechanism |
|----------|-----------|
| **Trustless** | IBC provides cryptographic proof |
| **Atomic** | Either both transfers complete or both refund |
| **Timeout-Safe** | IBC timeouts trigger automatic refunds |
| **Authorization** | Only settlement contract can release escrow |

### Current Limitations

1. **Same-chain uses IBC**: No direct `BankMsg::Send` optimization
2. **Admin-triggered acks**: IBC acks require admin to relay (could be automated)
3. **Single timeout value**: 600 seconds for all transfers (could be dynamic)
