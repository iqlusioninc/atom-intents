# TIA/USDC Swap Specification

## Solver-Managed Settlement for Non-Smart-Contract Chains

---

## Executive Summary

This document specifies how users with TIA on Celestia (a chain without smart contracts) can swap to USDC. The key insight is that **solvers take responsibility for packet delivery** - they run their own relayers and are incentivized to ensure IBC packets don't timeout.

### The Problem

Celestia is a data availability layer without CosmWasm smart contract support. Traditional two-phase settlement requires locking user funds in an escrow contract, which cannot execute on Celestia.

### The Solution

**Solver-Managed Settlement with Hub Adjudication**:

1. User's TIA is sent via IBC to the **Hub escrow contract** (not directly to solver)
2. Solver delivers USDC to user on destination chain
3. Hub settlement contract verifies USDC delivery via IBC ack
4. Only then does Hub release TIA from escrow to solver
5. Solver's integrated relayer ensures packets don't timeout

**Key insight**: The Hub still adjudicates the swap - it verifies the solver delivered before releasing user funds. But **solvers take the relay risk** - they're responsible for ensuring their USDC delivery packet doesn't timeout.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                 SOLVER-MANAGED SETTLEMENT WITH HUB ADJUDICATION                  │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────┐          ┌─────────────────────────┐          ┌─────────────────┐
│                 │          │       COSMOS HUB        │          │                 │
│    CELESTIA     │          │    (Adjudication)       │          │      NOBLE      │
│                 │          │                         │          │                 │
│  ┌───────────┐  │   IBC    │  ┌─────────────────┐   │          │  ┌───────────┐  │
│  │           │  │ + Hooks  │  │                 │   │          │  │           │  │
│  │  User's   │──┼─────────►│  │  Escrow (TIA)   │   │          │  │   User    │  │
│  │   TIA     │  │          │  │                 │   │          │  │  receives │  │
│  │           │  │          │  └────────┬────────┘   │          │  │   USDC    │  │
│  └───────────┘  │          │           │            │          │  └───────────┘  │
│                 │          │           │ Verify     │          │        ▲        │
│  No smart       │          │           │ delivery   │          │        │        │
│  contracts      │          │           ▼            │          │        │        │
│  needed!        │          │  ┌─────────────────┐   │   IBC    │        │        │
│                 │          │  │   Settlement    │   │          │        │        │
│                 │          │  │   Contract      │───┼──────────┼────────┘        │
│                 │          │  │                 │   │  (USDC   │                 │
│                 │          │  └────────┬────────┘   │  to user)│                 │
│                 │          │           │            │          │                 │
│                 │          │           │ On success │          │                 │
│                 │          │           ▼            │          │                 │
│                 │          │  ┌─────────────────┐   │          │                 │
│                 │          │  │ Release TIA     │   │          │                 │
│                 │          │  │ to Solver       │   │          │                 │
│                 │          │  └─────────────────┘   │          │                 │
│                 │          │                         │          │                 │
└─────────────────┘          └─────────────────────────┘          └─────────────────┘

                              ┌────────────────────┐
                              │      SOLVER        │
                              │  ┌──────────────┐  │
                              │  │   Relayer    │  │
                              │  │  (Priority   │  │
                              │  │   delivery)  │  │
                              │  └──────────────┘  │
                              │                    │
                              │  Takes relay risk: │
                              │  If USDC packet    │
                              │  times out, solver │
                              │  loses (no TIA)    │
                              └────────────────────┘
```

### How It Works

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            SETTLEMENT FLOW                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  1. User sends TIA → Hub escrow (via IBC from Celestia)                         │
│     └── Uses IBC Hooks to lock in escrow contract atomically                    │
│                                                                                  │
│  2. Solver sees escrow locked, sends USDC → User on Noble                       │
│     └── Solver takes risk: must deliver before timeout                          │
│     └── Solver's relayer prioritizes this packet                                │
│                                                                                  │
│  3. Hub settlement contract receives IBC ack proving USDC delivered             │
│     └── This is the adjudication step                                           │
│                                                                                  │
│  4. Hub releases TIA from escrow to solver                                      │
│     └── Solver is now made whole                                                │
│                                                                                  │
│  ═══════════════════════════════════════════════════════════════════════════    │
│                                                                                  │
│  SOLVER RISK: If USDC delivery times out:                                       │
│  • User's TIA remains locked (eventually refunded to user)                      │
│  • Solver loses the USDC they sent (it times out back to them)                  │
│  • This incentivizes solvers to run reliable relayers!                          │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## Design Principles

### 1. Hub as Neutral Adjudicator

The Cosmos Hub serves as the trust anchor:

| Aspect | Rationale |
|--------|-----------|
| **Escrow contract** | Holds user's TIA until delivery is verified |
| **Settlement contract** | Receives IBC acks, releases escrow on success |
| **Neutral ground** | Neither user nor solver controls the adjudication |
| **IBC Hooks** | Enables atomic lock-on-receive from Celestia |

### 2. Solvers Take Relay Risk

Solvers are responsible for ensuring IBC packets don't timeout:

| Aspect | Rationale |
|--------|-----------|
| **Integrated relayers** | Solvers already run relayers to protect their capital |
| **Aligned incentives** | Faster delivery = shorter exposure = more auction wins |
| **Timeout = loss** | If USDC delivery times out, solver doesn't get TIA |
| **Risk pricing** | Solvers can price relay risk into their quotes |

### 3. Single User Signature

Users sign **one transaction** on Celestia:
- IBC transfer to Hub escrow (with wasm hook memo)
- Escrow locks atomically on receive
- User is protected: TIA refunds if settlement fails

### 4. Trustless Settlement

The flow is trustless because:
- User's TIA is locked in Hub escrow (not sent to solver directly)
- Solver only gets TIA after Hub verifies USDC delivery
- If USDC delivery fails, user's TIA is refunded

---

## Complete TIA → USDC Flow

### Phase 0: Intent Submission & Solver Selection

User submits intent via Skip Select, solver wins auction:

```rust
Intent {
    id: "int_abc123",
    user: "celestia1user...",

    input: Asset {
        chain_id: "celestia",
        denom: "utia",
        amount: 1_000_000_000,  // 1000 TIA
    },

    output: OutputSpec {
        chain_id: "noble-1",
        denom: "uusdc",
        min_amount: 5_000_000_000,  // 5000 USDC minimum
        limit_price: Decimal::from_str("5.0")?,
        recipient: "noble1user...",
    },

    constraints: ExecutionConstraints {
        deadline: now + 60,
        max_hops: 3,
        ..Default::default()
    },
}
```

### Phase 1: User Sends TIA to Hub Escrow

User signs a single IBC transfer on Celestia with IBC Hooks memo:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 1: TIA → HUB ESCROW (Atomic via IBC Hooks)              │
└─────────────────────────────────────────────────────────────────────────────────┘

  CELESTIA                                    COSMOS HUB
      │                                            │
 T=0  │  User signs MsgTransfer                    │
      │  ┌──────────────────────────────────────┐  │
      │  │ receiver: cosmos1escrow...           │  │
      │  │ channel: channel-celestia-hub        │  │
      │  │ amount: 1000 TIA                     │  │
      │  │ memo: {                              │  │
      │  │   "wasm": {                          │  │
      │  │     "contract": "cosmos1escrow...",  │  │
      │  │     "msg": {                         │  │
      │  │       "lock_from_ibc": {             │  │
      │  │         "intent_id": "int_abc123",   │  │
      │  │         "solver_id": "solver1",      │  │
      │  │         "expires_at": T+15min        │  │
      │  │       }                              │  │
      │  │     }                                │  │
      │  │   }                                  │  │
      │  │ }                                    │  │
      │  └──────────────────────────────────────┘  │
      │                                            │
      │  ─────────────── IBC Packet ─────────────► │
      │                                            │
      │                                   ┌────────┴────────┐
      │                                   │   IBC Hooks     │
      │                                   │   Middleware    │
      │                                   │                 │
      │                                   │ 1. Receive TIA  │
      │                                   │ 2. Call escrow  │
      │                                   │    contract     │
      │                                   │ 3. Lock funds   │
      │                                   │    atomically   │
      │                                   └────────┬────────┘
      │                                            │
      │                                   Escrow Created:
      │                                   ┌─────────────────────┐
      │                                   │ id: "esc_xyz"       │
      │                                   │ intent: "int_abc123"│
      │                                   │ amount: 1000 TIA    │
      │                                   │ denom: ibc/TIA      │
      │                                   │ solver: "solver1"   │
      │                                   │ expires: T+15min    │
      │                                   └─────────────────────┘
      │                                            │
      │  ◄──────────── IBC Ack ────────────────── │
      │                                            │
 T=6s │  TIA LOCKED IN ESCROW ✓                    │
```

### Phase 2: Solver Delivers USDC to User

Solver sees escrow lock and sends USDC (solver takes relay risk here):

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 2: SOLVER DELIVERS USDC (Solver takes risk)             │
└─────────────────────────────────────────────────────────────────────────────────┘

  SOLVER (on Hub or Osmosis)                      NOBLE
      │                                               │
 T=7s │  Solver sees escrow locked                    │
      │  Initiates USDC delivery                      │
      │                                               │
      │  MsgTransfer (via Settlement Contract)        │
      │  ┌──────────────────────────────────────┐     │
      │  │ sender: cosmos1settlement...         │     │
      │  │ receiver: noble1user...              │     │
      │  │ channel: channel-hub-noble           │     │
      │  │ amount: 5100 USDC                    │     │
      │  │ memo: { "intent_id": "int_abc123" }  │     │
      │  │ ibc_callback: cosmos1settlement...   │ ◄── IBC callback for ack!
      │  └──────────────────────────────────────┘     │
      │                                               │
      │  ─────────────── IBC Packet ────────────────► │
      │                                               │
      │                                  ┌────────────┴────────────┐
      │                                  │                         │
      │                                  │  User receives          │
      │                                  │  5100 USDC on Noble     │
      │                                  │                         │
      │                                  │  noble1user...          │
      │                                  │                         │
      │                                  └────────────┬────────────┘
      │                                               │
      │  ◄─────────────── IBC Ack ───────────────────│
      │                                               │
T=12s │  USER HAS USDC ✓                              │
      │                                               │
      │  ┌────────────────────────────────────────┐   │
      │  │ Settlement contract receives IBC ack   │   │
      │  │ This PROVES delivery happened          │   │
      │  └────────────────────────────────────────┘   │
```

### Phase 3: Hub Releases Escrow to Solver

Settlement contract verifies delivery and releases TIA:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 3: HUB ADJUDICATION & ESCROW RELEASE                    │
└─────────────────────────────────────────────────────────────────────────────────┘

  COSMOS HUB                                      SOLVER
      │                                               │
T=12s │  Settlement contract receives IBC ack         │
      │  proving USDC was delivered to user           │
      │                                               │
      │  ┌────────────────────────────────────────┐   │
      │  │  Settlement Contract Logic:            │   │
      │  │                                        │   │
      │  │  1. Verify ack matches intent_id       │   │
      │  │  2. Verify delivery amount correct     │   │
      │  │  3. Call escrow.release(solver)        │   │
      │  └────────────────────────────────────────┘   │
      │                                               │
      │  MsgExecuteContract (internal)                │
      │  ┌────────────────────────────────────────┐   │
      │  │ escrow.release {                       │   │
      │  │   escrow_id: "esc_xyz",                │   │
      │  │   recipient: "cosmos1solver..."        │   │
      │  │ }                                      │   │
      │  └────────────────────────────────────────┘   │
      │                                               │
      │  Escrow releases 1000 ibc/TIA ───────────────►│
      │                                               │
T=13s │  SETTLEMENT COMPLETE ✓                        │
      │                                               │
      │  Solver now has ibc/TIA on Hub                │
      │  Can swap on Osmosis or IBC back to Celestia  │
```

### Complete Timeline

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         COMPLETE TIA → USDC TIMELINE                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  T+0s      User signs IBC transfer on Celestia (with wasm hook memo)            │
│            └── Single signature, single transaction                             │
│                                                                                  │
│  T+6s      TIA arrives on Hub, escrow contract locks it atomically              │
│            └── Atomic via IBC Hooks - if lock fails, IBC reverts                │
│                                                                                  │
│  T+7s      Solver sees escrow, initiates USDC delivery via Hub                  │
│            └── SOLVER TAKES RISK: must deliver before timeout                   │
│                                                                                  │
│  T+12s     USDC arrives on Noble, user receives funds                           │
│            └── IBC ack sent back to Hub settlement contract                     │
│                                                                                  │
│  T+13s     Settlement contract verifies delivery, releases TIA to solver        │
│            └── ADJUDICATION COMPLETE                                             │
│                                                                                  │
│  ═══════════════════════════════════════════════════════════════════════════    │
│  TOTAL TIME: ~13 seconds                                                         │
│  USER ACTIONS: 1 signature                                                       │
│  SMART CONTRACTS ON CELESTIA: None required                                      │
│  TRUST: Hub adjudicates - user protected, solver takes relay risk               │
│  ═══════════════════════════════════════════════════════════════════════════    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Failure Scenarios

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         FAILURE HANDLING                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  SCENARIO 1: User's IBC to Hub fails                                            │
│  ─────────────────────────────────────                                          │
│  • IBC times out or errors                                                       │
│  • TIA returns to user on Celestia automatically                                │
│  • No escrow created, no settlement                                             │
│  • User can retry                                                                │
│                                                                                  │
│  SCENARIO 2: Solver's USDC delivery times out                                   │
│  ───────────────────────────────────────────────                                │
│  • User's TIA remains in Hub escrow                                             │
│  • Solver's USDC times out back to solver (no loss of USDC)                     │
│  • BUT: Solver doesn't get the TIA (opportunity cost)                           │
│  • Escrow expires → TIA refunded to user via IBC                                │
│  • SOLVER TAKES THIS RISK → incentive to run reliable relayers                  │
│                                                                                  │
│  SCENARIO 3: Solver never attempts delivery                                     │
│  ─────────────────────────────────────────────                                  │
│  • Escrow expires after 15 minutes                                              │
│  • User can trigger refund                                                       │
│  • TIA sent back to Celestia via IBC                                            │
│  • Solver potentially slashed (if bonded)                                       │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## Escrow Contract Updates

### New Fields for Cross-Chain Escrow

```rust
#[cw_serde]
pub struct Escrow {
    pub id: String,

    /// Owner address - may be on a DIFFERENT chain
    pub owner: String,

    /// Chain where owner address exists
    pub owner_chain_id: String,

    /// IBC channel used for inbound transfer (for refunds)
    pub source_channel: String,

    /// Original denom on source chain (for refund routing)
    pub source_denom: String,

    pub amount: Uint128,
    pub denom: String,  // This is the ibc/... denom on Hub
    pub intent_id: String,
    pub expires_at: u64,
    pub status: EscrowStatus,
}
```

### Lock via IBC Hooks

```rust
/// Called by IBC Hooks middleware when receiving funds
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard lock (for Hub-native users)
    Lock {
        escrow_id: String,
        intent_id: String,
        expires_at: u64,
    },

    /// Lock via IBC receive (for cross-chain users)
    LockFromIbc {
        intent_id: String,
        expires_at: u64,
        /// User's address on the source chain
        user_source_address: String,
        /// Source chain ID (e.g., "celestia")
        source_chain_id: String,
        /// Channel the funds came through (for refunds)
        source_channel: String,
    },

    // ... existing messages
}
```

### Refund via IBC

```rust
fn execute_refund(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
) -> Result<Response, ContractError> {
    let escrow = ESCROWS.load(deps.storage, &escrow_id)?;

    // Verify escrow is expired
    if env.block.time.seconds() < escrow.expires_at {
        return Err(ContractError::EscrowNotExpired { id: escrow_id });
    }

    // Check status
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::InvalidStatus {});
    }

    // Determine refund method based on owner chain
    let refund_msg = if escrow.owner_chain_id == "cosmoshub-4" {
        // Local refund - simple bank send
        BankMsg::Send {
            to_address: escrow.owner.clone(),
            amount: vec![Coin {
                denom: escrow.denom.clone(),
                amount: escrow.amount,
            }],
        }.into()
    } else {
        // Cross-chain refund - IBC transfer back to source
        MsgTransfer {
            source_port: "transfer".to_string(),
            source_channel: escrow.source_channel.clone(),
            token: Some(Coin {
                denom: escrow.denom.clone(),
                amount: escrow.amount,
            }.into()),
            sender: env.contract.address.to_string(),
            receiver: escrow.owner.clone(),  // Address on source chain
            timeout_height: None,
            timeout_timestamp: env.block.time.plus_seconds(600).nanos(),
            memo: "".to_string(),
        }.into()
    };

    // Update status
    ESCROWS.update(deps.storage, &escrow_id, |e| -> StdResult<_> {
        let mut escrow = e.unwrap();
        escrow.status = EscrowStatus::Refunding;
        Ok(escrow)
    })?;

    Ok(Response::new()
        .add_message(refund_msg)
        .add_attribute("action", "refund")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("refund_to", escrow.owner)
        .add_attribute("refund_chain", escrow.owner_chain_id))
}
```

---

## IBC Memo Format

### Standard Transfer + Escrow Lock

```json
{
  "wasm": {
    "contract": "cosmos1escrowcontract...",
    "msg": {
      "lock_from_ibc": {
        "intent_id": "int_abc123",
        "expires_at": 1703520000,
        "user_source_address": "celestia1user...",
        "source_chain_id": "celestia",
        "source_channel": "channel-0"
      }
    }
  }
}
```

### With Swap + Forward (TIA → USDC via Osmosis)

For cases where the swap happens on an intermediate chain:

```json
{
  "wasm": {
    "contract": "cosmos1escrowcontract...",
    "msg": {
      "lock_from_ibc": {
        "intent_id": "int_abc123",
        "expires_at": 1703520000,
        "user_source_address": "celestia1user...",
        "source_chain_id": "celestia",
        "source_channel": "channel-0"
      }
    }
  }
}
```

The swap is handled **after** escrow lock, during settlement phase, not during the initial transfer.

---

## Settlement Flow Decision Tree

```
Intent received from user on chain X
    │
    ├── Is X = cosmoshub-4?
    │       │
    │       ├── YES: Lock directly in escrow contract
    │       │
    │       └── NO: Does X have smart contracts?
    │               │
    │               ├── YES: Lock in escrow on X (future)
    │               │
    │               └── NO: Route to Hub escrow via IBC Hooks
    │                         │
    │                         └── Construct IBC transfer with wasm memo
    │                             pointing to Hub escrow contract
    │
    ▼
Escrow locked on Hub
    │
    ├── Solver commits output in Hub vault
    │
    ├── Settlement contract releases output via IBC
    │   │
    │   └── Routes to user's desired destination chain
    │
    └── On success: Release escrow to solver
        On timeout: Refund escrow to user (via IBC if needed)
```

---

## Chain Capability Registry

To support this flow, we need to track chain capabilities:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainCapabilities {
    pub chain_id: String,

    /// Does this chain support CosmWasm smart contracts?
    pub has_cosmwasm: bool,

    /// Address of escrow contract (if deployed on this chain)
    pub escrow_contract: Option<String>,

    /// IBC channels to the default escrow chain (Hub)
    pub channel_to_hub: Option<String>,

    /// Does this chain support IBC Hooks?
    pub has_ibc_hooks: bool,
}

pub struct ChainRegistry {
    chains: HashMap<String, ChainCapabilities>,
}

impl ChainRegistry {
    pub fn new() -> Self {
        let mut chains = HashMap::new();

        // Cosmos Hub - default escrow chain
        chains.insert("cosmoshub-4".to_string(), ChainCapabilities {
            chain_id: "cosmoshub-4".to_string(),
            has_cosmwasm: true,
            escrow_contract: Some("cosmos1escrow...".to_string()),
            channel_to_hub: None,  // Is the Hub
            has_ibc_hooks: true,
        });

        // Celestia - no smart contracts
        chains.insert("celestia".to_string(), ChainCapabilities {
            chain_id: "celestia".to_string(),
            has_cosmwasm: false,
            escrow_contract: None,
            channel_to_hub: Some("channel-0".to_string()),
            has_ibc_hooks: false,
        });

        // Osmosis - has contracts
        chains.insert("osmosis-1".to_string(), ChainCapabilities {
            chain_id: "osmosis-1".to_string(),
            has_cosmwasm: true,
            escrow_contract: None,  // Using Hub for now
            channel_to_hub: Some("channel-0".to_string()),
            has_ibc_hooks: true,
        });

        // Noble - no CosmWasm but supports IBC
        chains.insert("noble-1".to_string(), ChainCapabilities {
            chain_id: "noble-1".to_string(),
            has_cosmwasm: false,
            escrow_contract: None,
            channel_to_hub: Some("channel-4".to_string()),
            has_ibc_hooks: false,
        });

        Self { chains }
    }

    /// Determine which chain should hold escrow for a given source chain
    pub fn get_escrow_chain(&self, source_chain: &str) -> &str {
        let source = self.chains.get(source_chain);

        match source {
            Some(caps) if caps.escrow_contract.is_some() => source_chain,
            _ => "cosmoshub-4",  // Default to Hub
        }
    }
}
```

---

## Timeout and Failure Handling

### Scenario 1: IBC Transfer to Hub Times Out

```
CELESTIA                           COSMOS HUB
    │                                   │
    │  MsgTransfer + wasm memo          │
    │  ───────────────────────────────► │
    │                                   │
    │      ╳ TIMEOUT (chain down,       │
    │        relayer issues, etc.)      │
    │                                   │
    │  ◄─────── MsgTimeout ──────────── │
    │                                   │
    │  Funds return to user on Celestia │
    │  (IBC standard timeout refund)    │
    │                                   │
    │  USER CAN RETRY ✓                 │
```

### Scenario 2: Escrow Lock Fails (contract error)

```
CELESTIA                           COSMOS HUB
    │                                   │
    │  MsgTransfer + wasm memo          │
    │  ───────────────────────────────► │
    │                                   │
    │                              IBC Hooks calls
    │                              escrow.lock()
    │                                   │
    │                              ╳ CONTRACT ERROR
    │                              (invalid params,
    │                               escrow exists, etc.)
    │                                   │
    │                              IBC receive REVERTS
    │                                   │
    │  ◄─────── Error Ack ──────────── │
    │                                   │
    │  Funds return to user on Celestia │
    │  (IBC atomic revert)              │
    │                                   │
    │  USER CAN RETRY ✓                 │
```

### Scenario 3: Solver Fails to Deliver

```
COSMOS HUB                              NOBLE
    │                                      │
    │  User escrow: LOCKED                 │
    │  Solver vault: LOCKED                │
    │                                      │
    │  MsgTransfer (solver output)         │
    │  ─────────────────────────────────►  │
    │                                      │
    │      ╳ TIMEOUT                       │
    │                                      │
    │  ◄─────── MsgTimeout ──────────────  │
    │                                      │
    │  Settlement contract detects timeout │
    │  1. Unlock solver vault              │
    │  2. Refund user escrow               │
    │     - If user on Hub: bank send      │
    │     - If user on Celestia: IBC back  │
    │                                      │
    │  BOTH PARTIES REFUNDED ✓             │
```

### Scenario 4: Refund IBC Fails

```
COSMOS HUB                              CELESTIA
    │                                      │
    │  Escrow expired, initiating refund   │
    │                                      │
    │  MsgTransfer (refund to user)        │
    │  ─────────────────────────────────►  │
    │                                      │
    │      ╳ TIMEOUT                       │
    │                                      │
    │  ◄─────── MsgTimeout ──────────────  │
    │                                      │
    │  Funds return to escrow contract     │
    │  Escrow status: RefundFailed         │
    │                                      │
    │  User or admin can retry refund      │
    │  FUNDS SAFE IN ESCROW ✓              │
```

---

## Security Considerations

### 1. IBC Hooks Authentication

The escrow contract must verify that `LockFromIbc` is only called by the IBC Hooks middleware:

```rust
fn execute_lock_from_ibc(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: LockFromIbcMsg,
) -> Result<Response, ContractError> {
    // Verify caller is the IBC transfer module
    // IBC Hooks calls the contract with the transfer module as sender
    let config = CONFIG.load(deps.storage)?;

    // The actual sender in IBC Hooks is the derived address
    // We verify the funds came through IBC by checking the denom
    if !info.funds.iter().any(|c| c.denom.starts_with("ibc/")) {
        return Err(ContractError::NotIbcFunds {});
    }

    // Additional verification: check that exactly one IBC coin was sent
    let ibc_funds: Vec<_> = info.funds.iter()
        .filter(|c| c.denom.starts_with("ibc/"))
        .collect();

    if ibc_funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            expected: "exactly one IBC denom".to_string(),
            got: format!("{} IBC denoms", ibc_funds.len()),
        });
    }

    // Proceed with escrow creation
    // ...
}
```

### 2. Replay Protection

Each intent has a unique ID, and each escrow is tied to that intent:

```rust
// Prevent duplicate escrows for same intent
if ESCROWS_BY_INTENT.has(deps.storage, &msg.intent_id) {
    return Err(ContractError::IntentAlreadyEscrowed {
        intent_id: msg.intent_id
    });
}
```

### 3. Timeout Ordering

Escrow timeout must be longer than IBC timeout + safety buffer:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           TIMEOUT ORDERING                                       │
│                                                                                  │
│  T+0       User initiates transfer from Celestia                                │
│  T+6s      IBC receives on Hub, escrow locks                                    │
│  T+10min   IBC timeout for output delivery (if not delivered)                   │
│  T+15min   Escrow timeout (5 min buffer after IBC timeout)                      │
│                                                                                  │
│  This ensures:                                                                   │
│  - Solver has time to detect IBC timeout                                        │
│  - Settlement contract can unwind before escrow expires                         │
│  - No race conditions between refund and release                                │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## Gas Costs

| Operation | Estimated Gas | Estimated Cost |
|-----------|---------------|----------------|
| IBC Transfer (Celestia → Hub) | 100,000 | ~$0.01 TIA |
| Escrow Lock (via Hooks) | 150,000 | ~$0.02 ATOM |
| Solver Vault Commit | 120,000 | ~$0.02 ATOM |
| IBC Transfer (Hub → Noble) | 100,000 | ~$0.02 ATOM |
| Escrow Release | 80,000 | ~$0.01 ATOM |
| **Total** | ~550,000 | **~$0.08** |

---

## Implementation Checklist

### Smart Contracts

- [ ] Update `contracts/escrow` to support `LockFromIbc` message
- [ ] Add `owner_chain_id` and `source_channel` fields to Escrow struct
- [ ] Implement IBC refund logic for cross-chain escrows
- [ ] Add escrow-by-intent index for duplicate prevention

### Settlement Crate

- [ ] Add `ChainCapabilities` registry
- [ ] Update `SettlementManager` to route to Hub escrow for non-wasm chains
- [ ] Implement IBC memo construction for wasm hooks
- [ ] Add cross-chain refund handling

### Solver Integration

- [ ] Update solvers to watch for escrow events on Hub
- [ ] Handle ibc/* denoms in inventory management
- [ ] Add TIA/USDC pair support

### Configuration

- [ ] Add Celestia chain config with IBC channel mappings
- [ ] Configure TIA denom traces
- [ ] Set up channel registry for Celestia ↔ Hub

---

## Summary

This specification enables users on chains without smart contracts (like Celestia) to participate in the intent-based swap system through:

### Core Design

1. **Hub as Neutral Adjudicator** - Cosmos Hub holds escrow and verifies delivery
2. **IBC Hooks for atomic lock-on-receive** - User's TIA locked atomically on Hub
3. **Solver takes relay risk** - Solvers responsible for timely delivery
4. **Cross-chain refunds via IBC** - Timeout protection works across chains

### Trust Model

| Party | Trust Required | Protection |
|-------|----------------|------------|
| **User** | None | TIA locked in Hub escrow, refunded if delivery fails |
| **Solver** | Takes relay risk | Must ensure USDC delivery; loses opportunity if timeout |
| **Hub** | Smart contract correctness | Audited contracts, on-chain verification |

### Key Properties

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         SYSTEM PROPERTIES                                        │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ✓ TRUSTLESS FOR USER                                                           │
│    User's funds protected by Hub escrow contract                                │
│    No trust required in solver - Hub adjudicates                                │
│                                                                                  │
│  ✓ INCENTIVE-ALIGNED FOR SOLVER                                                 │
│    Solver takes relay risk → runs reliable relayers                             │
│    Faster delivery → shorter exposure → more auction wins                       │
│                                                                                  │
│  ✓ NO SMART CONTRACTS ON CELESTIA                                               │
│    Works with pure IBC transfer capability                                       │
│    Hub escrow handles the complexity                                            │
│                                                                                  │
│  ✓ SINGLE USER SIGNATURE                                                        │
│    User signs one IBC transfer on Celestia                                      │
│    Everything else happens automatically                                         │
│                                                                                  │
│  ✓ ~13 SECOND END-TO-END                                                        │
│    Fast execution via solver relayers                                           │
│    Comparable to centralized exchange experience                                │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### User Experience

The user experience is simple: **sign one transaction on Celestia, receive USDC on Noble**.

The complexity of Hub escrow, solver coordination, and IBC relaying is handled entirely by the system.
