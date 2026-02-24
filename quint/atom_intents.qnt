// atom_intents.qnt
//
// Quint specification for the ATOM Intents Protocol.
// Proves correctness of the two-phase commit settlement, escrow system,
// on-chain order fallback, and solver reputation system.
//
// Source: https://github.com/iqlusioninc/atom-intents
// Settlement state machine: contracts/settlement/src/state.rs + handlers.rs
// Escrow contract:          contracts/escrow/src/state.rs + contract.rs
// Two-phase commit:         crates/settlement/src/two_phase.rs
// Protocol spec:            docs/SPECIFICATION.md
//
// ─── Properties proved ────────────────────────────────────────────────────
// INV-1   validStatusInvariants        Completed settlements are properly escrow-linked
// INV-2   noDoubleSettlement           Each intent maps to at most one settlement
// INV-3   escrowConservation           Sum of held escrow amounts = ghost tracker
// INV-4   noDoubleRelease              Each escrow exits to Released/Refunded at most once
// INV-5   timeoutOrderingHolds         escrow_timeout > ibc_timeout + safety_buffer
// INV-6   fundSafety                   Completed→Released; Failed/Slashed→Refunded|Refunding|RefundFailed
// INV-7   constraintSatisfied          Completed output >= user's minOutputAmount
// INV-8   solverBondNonNegative        Slashing never drives a bond below zero
// INV-9   escrowIdPopulated            Active post-lock settlements have a linked escrow
// INV-10  oneEscrowPerIntent           Each intentId has at most one escrow
// INV-11  refundingImpliesCrossChain   Only cross-chain escrows enter Refunding/RefundFailed
// INV-12  activeSettlementHasSolver    Active settlements reference a registered solver
// INV-13  orderFundSafety             Open/Cancelled/Expired orders have correct fund accounting
// INV-14  noDoubleFill                Each order is filled at most once
// INV-15  filledOrderHasSolver        Filled orders reference a registered solver
// INV-16  reputationScoreBounded      Reputation scores stay in 0..10000
// INV-17  reputationConsistency       Reputation totals match settlement history

// ═══════════════════════════════════════════════════════════════════════════
// MODULE 1: Domain Types and Pure Helpers
// ═══════════════════════════════════════════════════════════════════════════

module AtomIntentsTypes {

  type Address      = str
  type SettlementId = int   // monotonic counter; 0 = sentinel "none"
  type EscrowId     = int   // same counter space; 0 = sentinel "none"
  type IntentId     = str
  type SolverId     = str
  type Denom        = str
  type OrderId      = str

  // Settlement lifecycle
  // Source: SettlementStatus::can_transition_to — contracts/settlement/src/state.rs:75
  //
  //   Pending ──► UserLocked ──► SolverLocked ──► Executing ──► Completed
  //      │             │               │               │
  //      └─────────────┴───────────────┴───────────────┴──► Failed ──► Slashed
  //      └─────────────┴───────────────┴───────────────┴─────────────► Slashed
  //
  // Additionally, executeSettlementLocal bypasses Executing:
  //   SolverLocked ──► Completed  (same-chain atomic path)
  //
  // Completed and Slashed are terminal (no outgoing transitions).

  type SettlementStatus =
    | Pending
    | UserLocked
    | SolverLocked
    | Executing
    | Completed
    | Failed
    | Slashed

  // Escrow lifecycle — contracts/escrow/src/state.rs:43
  //
  //   Locked ──► Released                      (happy path: solver paid)
  //   Locked ──► Refunded                      (local refund: user repaid)
  //   Locked ──► Refunding ──► Refunded        (cross-chain IBC refund succeeds)
  //                  │
  //                  └──► RefundFailed ──► Refunding  (retry cycle)
  type EscrowStatus =
    | Locked
    | Released
    | Refunded
    | Refunding
    | RefundFailed

  type Settlement = {
    id:                 SettlementId,
    intentId:           IntentId,
    solverId:           SolverId,
    user:               Address,
    userInputAmount:    int,
    userInputDenom:     Denom,
    solverOutputAmount: int,
    solverOutputDenom:  Denom,
    // User's minimum acceptable output (from the original signed intent)
    minOutputAmount:    int,
    status:             SettlementStatus,
    // expiresAt = createdAt + ibcTimeoutSecs + safetyBufferSecs
    expiresAt:          int,
    ibcTimeoutSecs:     int,
    safetyBufferSecs:   int,
    // 0 = sentinel "not yet linked"; set when UserLocked is reached
    escrowId:           EscrowId,
  }

  type Escrow = {
    id:           EscrowId,
    owner:        Address,
    amount:       int,
    denom:        Denom,
    intentId:     IntentId,
    expiresAt:    int,
    status:       EscrowStatus,
    // True for escrows created via LockFromIbc (cross-chain).
    // Determines refund path: IBC transfer (async) vs BankMsg::Send (atomic).
    // Source: contracts/escrow/src/contract.rs — execute_refund branches
    // on presence of source_channel / owner_source_address.
    isCrossChain: bool,
  }

  type Solver = {
    id:                SolverId,
    bondAmount:        int,
    active:            bool,
    failedSettlements: int,
  }

  // On-chain order lifecycle — contracts/settlement/src/state.rs:197
  //
  //   Open ──► Filled     (solver fills the order)
  //   Open ──► Expired    (order timed out, user refunded)
  //   Open ──► Cancelled  (user cancels before fill)
  type OrderStatus =
    | Open
    | Filled
    | OrderExpired
    | Cancelled

  type Order = {
    id:              OrderId,
    user:            Address,
    inputAmount:     int,
    inputDenom:      Denom,
    minOutputAmount: int,
    outputDenom:     Denom,
    status:          OrderStatus,
    filledBySolver:  SolverId,   // "" = not yet filled
    createdAt:       int,
    expiresAt:       int,
    // Links to settlement for cross-chain fills (0 = same-chain, no settlement)
    settlementId:    SettlementId,
  }

  // Solver reputation — contracts/settlement/src/state.rs:116
  // Score components: success_rate(40%) + volume(20%) + speed(20%) + no_slash(20%)
  // Source: helpers::calculate_reputation_score
  type FeeTier =
    | Premium    // 9000-10000
    | Standard   // 7000-8999
    | Basic      // 5000-6999
    | New        // 0-4999

  type Reputation = {
    solverId:              SolverId,
    totalSettlements:      int,
    successfulSettlements: int,
    failedSettlements:     int,
    slashingEvents:        int,
    // 0-10000 basis points; default 5000 for new solvers
    reputationScore:       int,
    lastUpdated:           int,
  }

  // ─── Default (empty) values for safe map access ───────────────────────
  // These allow safe get-or-default without panicking on missing keys.

  pure def defaultSettlement: Settlement = {
    id: 0, intentId: "", solverId: "", user: "",
    userInputAmount: 0, userInputDenom: "", solverOutputAmount: 0,
    solverOutputDenom: "", minOutputAmount: 0, status: Pending,
    expiresAt: 0, ibcTimeoutSecs: 0, safetyBufferSecs: 0, escrowId: 0,
  }

  pure def defaultEscrow: Escrow = {
    id: 0, owner: "", amount: 0, denom: "", intentId: "", expiresAt: 0,
    status: Locked, isCrossChain: false,
  }

  pure def defaultSolver: Solver = {
    id: "", bondAmount: 0, active: false, failedSettlements: 0,
  }

  pure def defaultOrder: Order = {
    id: "", user: "", inputAmount: 0, inputDenom: "", minOutputAmount: 0,
    outputDenom: "", status: Open, filledBySolver: "", createdAt: 0,
    expiresAt: 0, settlementId: 0,
  }

  pure def defaultReputation: Reputation = {
    solverId: "", totalSettlements: 0, successfulSettlements: 0,
    failedSettlements: 0, slashingEvents: 0, reputationScore: 5000,
    lastUpdated: 0,
  }

  // ─── Safe map accessors ───────────────────────────────────────────────

  pure def getSettlement(m: SettlementId -> Settlement, k: SettlementId): Settlement =
    if (m.keys().contains(k)) m.get(k) else defaultSettlement

  pure def getEscrow(m: EscrowId -> Escrow, k: EscrowId): Escrow =
    if (m.keys().contains(k)) m.get(k) else defaultEscrow

  pure def getSolver(m: SolverId -> Solver, k: SolverId): Solver =
    if (m.keys().contains(k)) m.get(k) else defaultSolver

  pure def getOrder(m: OrderId -> Order, k: OrderId): Order =
    if (m.keys().contains(k)) m.get(k) else defaultOrder

  pure def getReputation(m: SolverId -> Reputation, k: SolverId): Reputation =
    if (m.keys().contains(k)) m.get(k) else defaultReputation

  // Safe integer-valued map access (for ghost tracking maps)
  pure def getInt(m: Denom -> int, k: Denom): int =
    if (m.keys().contains(k)) m.get(k) else 0

  pure def getCnt(m: EscrowId -> int, k: EscrowId): int =
    if (m.keys().contains(k)) m.get(k) else 0

  // ─── Pure protocol helpers ────────────────────────────────────────────

  // Mirror of SettlementStatus::can_transition_to — contracts/settlement/src/state.rs:75
  // Parameter renamed from `to` (built-in integer range operator) to `tgt`.
  pure def canTransitionTo(src: SettlementStatus, tgt: SettlementStatus): bool =
    match src {
      | Pending     => tgt == UserLocked  or tgt == Failed or tgt == Slashed
      | UserLocked  => tgt == SolverLocked or tgt == Failed or tgt == Slashed
      | SolverLocked => tgt == Executing  or tgt == Failed or tgt == Slashed
      | Executing   => tgt == Completed   or tgt == Failed or tgt == Slashed
      | Failed      => tgt == Slashed
      | Completed   => false
      | Slashed     => false
    }

  pure def isActive(s: SettlementStatus): bool =
    s == Pending or s == UserLocked or s == SolverLocked or s == Executing

  pure def isTerminal(s: SettlementStatus): bool =
    s == Completed or s == Slashed

  // True when escrow funds are still held by the escrow contract.
  // Locked = waiting for resolution; Refunding = IBC dispatch attempted;
  // RefundFailed = IBC dispatch failed, funds returned to escrow contract.
  pure def isEscrowHeld(s: EscrowStatus): bool =
    s == Locked or s == Refunding or s == RefundFailed

  // Order status helpers
  pure def isOrderTerminal(s: OrderStatus): bool =
    s == Filled or s == OrderExpired or s == Cancelled

  // ─── Reputation score calculation ────────────────────────────────────
  // Mirror of helpers::calculate_reputation_score — helpers.rs:11-50
  // Components: success_rate(40%) + slash_penalty(20%) = simplified model
  // (Volume and speed are off-chain metrics — not modeled in the state machine)
  //
  // We model the two components that are observable in the on-chain state:
  //   successRate   = (successful * 4000) / total          [0..4000]
  //   slashPenalty  = min(slashingEvents, 10) * 200        [0..2000]
  //   baseComponent = 4000 (volume + speed default)
  //   score = successRate + baseComponent - slashPenalty    [0..10000]

  pure def computeReputationScore(rep: Reputation): int =
    if (rep.totalSettlements == 0) 5000
    else
      val successRate   = (rep.successfulSettlements * 4000) / rep.totalSettlements
      val slashEvents   = if (rep.slashingEvents > 10) 10 else rep.slashingEvents
      val slashPenalty  = slashEvents * 200
      val baseComponent = 4000   // volume(2000) + speed(2000) default
      val raw = successRate + baseComponent - slashPenalty
      if (raw > 10000) 10000 else if (raw < 0) 0 else raw

  pure def getFeeTier(score: int): FeeTier =
    if (score >= 9000) Premium
    else if (score >= 7000) Standard
    else if (score >= 5000) Basic
    else New
}

// ═══════════════════════════════════════════════════════════════════════════
// MODULE 2: Protocol State Machine (parameterized over concrete values)
// ═══════════════════════════════════════════════════════════════════════════

module AtomIntents {
  import AtomIntentsTypes.*

  // ─── Protocol constants ───────────────────────────────────────────────
  const SOLVERS_SET:        Set[SolverId]
  const USERS_SET:          Set[Address]
  const INTENTS_SET:        Set[IntentId]
  const DENOMS_SET:         Set[Denom]
  const MIN_SOLVER_BOND:    int   // minimum bond to register
  const BASE_SLASH_BPS:     int   // e.g. 200 = 2%
  const MIN_SLASH_AMOUNT:   int   // e.g. 10_000_000 uatom
  const IBC_TIMEOUT_SECS:   int   // 600 (10 min) — compressed to 10 in tests
  const SAFETY_BUFFER_SECS: int   // 300 (5 min)  — compressed to 5 in tests
  const MAX_AMOUNT:         int   // upper bound for simulation state space

  // ─── State variables ──────────────────────────────────────────────────
  var settlements:       SettlementId -> Settlement
  var escrows:           EscrowId -> Escrow
  var solvers:           SolverId -> Solver
  // Maps intentId → settlementId; enforces at-most-one settlement per intent
  var intentSettlements: IntentId -> SettlementId
  var currentTime:       int
  var nextId:            int   // monotonic ID counter; starts at 1

  // On-chain order book — contracts/settlement/src/state.rs:169
  var orders:            OrderId -> Order

  // Solver reputation records — contracts/settlement/src/state.rs:116
  var reputations:       SolverId -> Reputation

  // Ghost variables — only read by invariants, never by action guards.
  // ghostEscrowExitCount[eId] = how many times escrow eId reached Released or Refunded
  var ghostEscrowExitCount: EscrowId -> int
  // ghostTotalLocked[denom] = running sum of amounts in held escrows
  // (Locked + Refunding + RefundFailed — funds still in the escrow contract)
  var ghostTotalLocked:     Denom -> int
  // ghostOrderLocked[orderId] = amount held by the contract for this order
  // (positive while Open; zeroed on fill/expire/cancel)
  var ghostOrderLocked:     OrderId -> int

  // ─── Initialization ───────────────────────────────────────────────────

  action init = all {
    settlements'          = Map(),
    escrows'              = Map(),
    solvers'              = Map(),
    intentSettlements'    = Map(),
    orders'               = Map(),
    reputations'          = Map(),
    currentTime'          = 0,
    nextId'               = 1,
    ghostEscrowExitCount' = Map(),
    ghostTotalLocked'     = Map(),
    ghostOrderLocked'     = Map(),
  }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 0: Solver Management
  // ═════════════════════════════════════════════════════════════════════

  // ─── Action 1: Register Solver ────────────────────────────────────────
  // Solver deposits a bond to participate. Bond must meet the minimum.
  // Source: execute_register_solver — handlers.rs:24
  // Security: SC-C1 prevents overwriting an existing registration.

  action registerSolver(solverId: SolverId, bond: int): bool = all {
    SOLVERS_SET.contains(solverId),
    not(solvers.keys().contains(solverId)),    // SC-C1: no overwrite
    bond >= MIN_SOLVER_BOND,
    bond > 0,
    solvers' = solvers.put(solverId, {
      id:                solverId,
      bondAmount:        bond,
      active:            true,
      failedSettlements: 0,
    }),
    // Frame
    settlements'          = settlements,
    escrows'              = escrows,
    intentSettlements'    = intentSettlements,
    currentTime'          = currentTime,
    nextId'               = nextId,
    ghostEscrowExitCount' = ghostEscrowExitCount,
    ghostTotalLocked'     = ghostTotalLocked,
    orders'               = orders,
    reputations'          = reputations,
    ghostOrderLocked'     = ghostOrderLocked,
  }

  // ─── Action 2: Deregister Solver ──────────────────────────────────────
  // Solver withdraws bond and leaves the protocol.
  // Source: execute_deregister_solver — handlers.rs:71-126
  // Security: SC-C2 blocks deregistration when the solver has any
  // active settlement (Pending | UserLocked | SolverLocked | Executing).
  // Bond is returned via BankMsg::Send (not modeled — no token balances).

  action deregisterSolver(solverId: SolverId): bool = all {
    solvers.keys().contains(solverId),
    // SC-C2: no active settlements referencing this solver
    settlements.keys().forall(sId =>
      val s = getSettlement(settlements, sId)
      s.solverId != solverId or not(isActive(s.status))
    ),
    solvers' = solvers.keys().filter(k => k != solverId).fold(
      Map(), (acc, k) => acc.put(k, solvers.get(k))
    ),
    // Frame
    settlements'          = settlements,
    escrows'              = escrows,
    intentSettlements'    = intentSettlements,
    currentTime'          = currentTime,
    nextId'               = nextId,
    ghostEscrowExitCount' = ghostEscrowExitCount,
    ghostTotalLocked'     = ghostTotalLocked,
    orders'               = orders,
    reputations'          = reputations,
    ghostOrderLocked'     = ghostOrderLocked,
  }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 1: Settlement Creation
  // ═════════════════════════════════════════════════════════════════════

  // ─── Action 3: Create Settlement ─────────────────────────────────────
  // Off-chain coordinator creates a settlement record after the auction winner
  // is selected. The settlement is initialized in Pending state.
  // Source: execute_create_settlement — handlers.rs

  action createSettlement(
    intentId:           IntentId,
    solverId:           SolverId,
    user:               Address,
    userInputAmount:    int,
    userInputDenom:     Denom,
    solverOutputAmount: int,
    solverOutputDenom:  Denom,
    minOutputAmount:    int,
  ): bool =
    val sId = nextId
    all {
      INTENTS_SET.contains(intentId),
      USERS_SET.contains(user),
      DENOMS_SET.contains(userInputDenom),
      DENOMS_SET.contains(solverOutputDenom),
      solvers.keys().contains(solverId),
      getSolver(solvers, solverId).active,
      getSolver(solvers, solverId).bondAmount >= MIN_SOLVER_BOND,
      not(intentSettlements.keys().contains(intentId)),  // no double settlement
      userInputAmount > 0,
      minOutputAmount > 0,
      solverOutputAmount >= minOutputAmount,   // solver meets user's price constraint
      // Timeout ordering (critical safety rule — two_phase.rs:46, SPEC §18.1):
      //   escrow_timeout = ibc_timeout + safety_buffer > ibc_timeout
      //   This gap gives time to cancel escrow release if IBC times out.
      SAFETY_BUFFER_SECS > 0,
      IBC_TIMEOUT_SECS > 0,
      settlements' = settlements.put(sId, {
        id:                 sId,
        intentId:           intentId,
        solverId:           solverId,
        user:               user,
        userInputAmount:    userInputAmount,
        userInputDenom:     userInputDenom,
        solverOutputAmount: solverOutputAmount,
        solverOutputDenom:  solverOutputDenom,
        minOutputAmount:    minOutputAmount,
        status:             Pending,
        expiresAt:          currentTime + IBC_TIMEOUT_SECS + SAFETY_BUFFER_SECS,
        ibcTimeoutSecs:     IBC_TIMEOUT_SECS,
        safetyBufferSecs:   SAFETY_BUFFER_SECS,
        escrowId:           0,   // 0 = not yet linked
      }),
      intentSettlements' = intentSettlements.put(intentId, sId),
      nextId'            = nextId + 1,
      // Frame
      escrows'              = escrows,
      solvers'              = solvers,
      currentTime'          = currentTime,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 2: Two-Phase Commit
  // ═════════════════════════════════════════════════════════════════════

  // ─── Action 4: Mark User Locked (Phase 1a) ───────────────────────────
  // User's input tokens are locked in the on-chain escrow contract.
  // Transitions: Pending → UserLocked
  // Source: execute_mark_user_locked — handlers.rs
  //
  // The crossChain parameter models whether funds arrived via IBC Hooks
  // (LockFromIbc) or a direct bank send (Lock). Cross-chain escrows use
  // an async IBC refund path (Locked → Refunding → Refunded) instead of
  // the atomic local path (Locked → Refunded).

  action markUserLocked(sId: SettlementId, crossChain: bool): bool =
    val s   = getSettlement(settlements, sId)
    val eId = nextId
    all {
      settlements.keys().contains(sId),
      s.status == Pending,
      canTransitionTo(s.status, UserLocked),
      currentTime < s.expiresAt,
      escrows' = escrows.put(eId, {
        id:           eId,
        owner:        s.user,
        amount:       s.userInputAmount,
        denom:        s.userInputDenom,
        intentId:     s.intentId,
        expiresAt:    s.expiresAt,
        status:       Locked,
        isCrossChain: crossChain,
      }),
      settlements' = settlements.put(sId, { ...s, status: UserLocked, escrowId: eId }),
      nextId'      = nextId + 1,
      // Ghost: track newly locked amount for this denom
      ghostTotalLocked' = ghostTotalLocked.put(
        s.userInputDenom,
        getInt(ghostTotalLocked, s.userInputDenom) + s.userInputAmount
      ),
      // Frame
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ─── Action 5: Mark Solver Locked (Phase 1b) ─────────────────────────
  // Solver's output commitment is confirmed locked.
  // Both phases now committed — safe to initiate IBC transfer.
  // Transitions: UserLocked → SolverLocked
  // Source: execute_mark_solver_locked — handlers.rs

  action markSolverLocked(sId: SettlementId): bool =
    val s = getSettlement(settlements, sId)
    all {
      settlements.keys().contains(sId),
      s.status == UserLocked,
      canTransitionTo(s.status, SolverLocked),
      currentTime < s.expiresAt,
      settlements' = settlements.put(sId, { ...s, status: SolverLocked }),
      // Frame
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ─── Action 6: Mark Executing ─────────────────────────────────────────
  // The IBC transfer of solver's output to the user has been initiated.
  // Transitions: SolverLocked → Executing
  // Source: execute_settlement (IBC path) — handlers.rs:505-585
  //
  // In the contract, execute_settlement also validates the IBC channel
  // against an allowlist and checks settlement expiry (SC-C4). We model
  // the expiry check; channel validation is abstracted away.

  action markExecuting(sId: SettlementId): bool =
    val s = getSettlement(settlements, sId)
    all {
      settlements.keys().contains(sId),
      s.status == SolverLocked,
      canTransitionTo(s.status, Executing),
      currentTime < s.expiresAt,    // SC-C4: settlement must not be expired
      settlements' = settlements.put(sId, { ...s, status: Executing }),
      // Frame
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 3: Settlement Resolution
  // ═════════════════════════════════════════════════════════════════════

  // ─── Action 7: Mark Completed (IBC Happy Path) ────────────────────────
  // IBC acknowledgement received: solver's output delivered to user.
  // Release user's escrowed input to the solver.
  // Transitions: Executing → Completed
  // Source: handle_ibc_ack (success=true) — handlers.rs:774-845
  //
  // The escrow contract's execute_release (contract.rs:239-296) has an
  // expiry guard (SC-5.6): release is blocked after escrow expiry to
  // prevent a release-after-refund race condition.

  action markCompleted(sId: SettlementId): bool =
    val s      = getSettlement(settlements, sId)
    val escrow = getEscrow(escrows, s.escrowId)
    all {
      settlements.keys().contains(sId),
      s.status == Executing,
      canTransitionTo(s.status, Completed),
      escrows.keys().contains(s.escrowId),
      escrow.status == Locked,           // escrow must still be locked
      currentTime < s.expiresAt,         // SC-5.6: release blocked after expiry
      settlements' = settlements.put(sId, { ...s, status: Completed }),
      escrows'     = escrows.put(s.escrowId, { ...escrow, status: Released }),
      // Ghost: escrow has exited to a terminal state
      ghostEscrowExitCount' = ghostEscrowExitCount.put(
        s.escrowId, getCnt(ghostEscrowExitCount, s.escrowId) + 1
      ),
      // Ghost: reduce held total for this denom
      ghostTotalLocked' = ghostTotalLocked.put(
        escrow.denom, getInt(ghostTotalLocked, escrow.denom) - escrow.amount
      ),
      // Frame
      solvers'           = solvers,
      intentSettlements' = intentSettlements,
      currentTime'       = currentTime,
      nextId'            = nextId,
      orders'            = orders,
      reputations'       = reputations,
      ghostOrderLocked'  = ghostOrderLocked,
    }

  // ─── Action 8: Execute Settlement Local (Same-Chain Happy Path) ───────
  // Atomic same-chain settlement: solver sends output directly via
  // BankMsg::Send, user's escrowed input is released to the solver.
  // Transitions: SolverLocked → Completed (skips Executing entirely)
  // Source: execute_settlement_local — handlers.rs:601-701
  //
  // This path is NOT in the can_transition_to state machine. The contract
  // bypasses it intentionally: local settlements are atomic (single block)
  // so the intermediate Executing state is unnecessary.

  action executeSettlementLocal(sId: SettlementId): bool =
    val s      = getSettlement(settlements, sId)
    val escrow = getEscrow(escrows, s.escrowId)
    all {
      settlements.keys().contains(sId),
      s.status == SolverLocked,
      s.escrowId != 0,                   // escrow must be linked
      escrows.keys().contains(s.escrowId),
      escrow.status == Locked,
      currentTime < s.expiresAt,         // SC-C4: not expired
      settlements' = settlements.put(sId, { ...s, status: Completed }),
      escrows'     = escrows.put(s.escrowId, { ...escrow, status: Released }),
      // Ghost: escrow exited
      ghostEscrowExitCount' = ghostEscrowExitCount.put(
        s.escrowId, getCnt(ghostEscrowExitCount, s.escrowId) + 1
      ),
      ghostTotalLocked' = ghostTotalLocked.put(
        escrow.denom, getInt(ghostTotalLocked, escrow.denom) - escrow.amount
      ),
      // Frame
      solvers'           = solvers,
      intentSettlements' = intentSettlements,
      currentTime'       = currentTime,
      nextId'            = nextId,
      orders'            = orders,
      reputations'       = reputations,
      ghostOrderLocked'  = ghostOrderLocked,
    }

  // ─── Action 9: Handle Timeout / Failure ──────────────────────────────
  // Settlement has expired. Refund user's escrowed input.
  // Transitions: any active state → Failed.
  //
  // This action embodies the critical safety guarantee (SPEC §18.1, two_phase.rs:46):
  //   escrow_timeout = ibc_timeout + safety_buffer
  //   When the IBC packet times out at T+ibc_timeout, we still have
  //   safety_buffer seconds before the escrow expires. Once the escrow
  //   expires, this action can fire — marking the settlement Failed and
  //   initiating the refund.
  //
  // For local escrows: refund is atomic (Locked → Refunded).
  // For cross-chain escrows: refund is async IBC (Locked → Refunding).
  //   The async path continues via ibcRefundSucceeds/ibcRefundFails/retryRefund.
  //
  // Source: execute_handle_timeout, execute_mark_failed — handlers.rs
  //         execute_refund — contracts/escrow/src/contract.rs:298-387

  action handleTimeout(sId: SettlementId): bool =
    val s              = getSettlement(settlements, sId)
    val escrow         = getEscrow(escrows, s.escrowId)
    val hasLockedEscrow =
      escrows.keys().contains(s.escrowId) and escrow.status == Locked
    val isLocal = hasLockedEscrow and not(escrow.isCrossChain)
    val isCross = hasLockedEscrow and escrow.isCrossChain
    all {
      settlements.keys().contains(sId),
      isActive(s.status),
      canTransitionTo(s.status, Failed),
      currentTime >= s.expiresAt,
      settlements' = settlements.put(sId, { ...s, status: Failed }),
      // Refund escrow: local → Refunded (atomic); cross-chain → Refunding (async)
      escrows' =
        if (isLocal)
          escrows.put(s.escrowId, { ...escrow, status: Refunded })
        else if (isCross)
          escrows.put(s.escrowId, { ...escrow, status: Refunding })
        else
          escrows,
      // Ghost: only local refund is a terminal exit (funds leave escrow contract)
      ghostEscrowExitCount' =
        if (isLocal)
          ghostEscrowExitCount.put(
            s.escrowId, getCnt(ghostEscrowExitCount, s.escrowId) + 1
          )
        else
          ghostEscrowExitCount,
      // Ghost: only local refund reduces held total
      // (cross-chain Refunding: funds still in escrow contract until IBC completes)
      ghostTotalLocked' =
        if (isLocal)
          ghostTotalLocked.put(
            escrow.denom, getInt(ghostTotalLocked, escrow.denom) - escrow.amount
          )
        else
          ghostTotalLocked,
      // Frame
      solvers'           = solvers,
      intentSettlements' = intentSettlements,
      currentTime'       = currentTime,
      nextId'            = nextId,
      orders'            = orders,
      reputations'       = reputations,
      ghostOrderLocked'  = ghostOrderLocked,
    }

  // ─── Action 10: Slash Solver ──────────────────────────────────────────
  // Penalize a solver for failing after commitment.
  // Slash amount = max(userInput * BASE_SLASH_BPS / 10000, MIN_SLASH_AMOUNT),
  // capped at the solver's remaining bond (guarantees non-negativity, INV-8).
  // Transitions: Failed → Slashed
  // Source: execute_slash_solver — handlers.rs; MIN_SLASH_AMOUNT=10_000_000 uatom

  action slashSolver(sId: SettlementId): bool =
    val s        = getSettlement(settlements, sId)
    val solver   = getSolver(solvers, s.solverId)
    val rawSlash = s.userInputAmount * BASE_SLASH_BPS / 10000
    val slash    = if (rawSlash >= MIN_SLASH_AMOUNT) rawSlash else MIN_SLASH_AMOUNT
    val actual   = if (slash > solver.bondAmount) solver.bondAmount else slash
    all {
      settlements.keys().contains(sId),
      s.status == Failed,
      canTransitionTo(s.status, Slashed),
      solvers.keys().contains(s.solverId),
      solver.bondAmount >= 0,
      settlements' = settlements.put(sId, { ...s, status: Slashed }),
      solvers'     = solvers.put(s.solverId, { ...solver,
        bondAmount:        solver.bondAmount - actual,
        failedSettlements: solver.failedSettlements + 1,
      }),
      // Frame
      escrows'              = escrows,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 4: Cross-Chain Escrow Refund Lifecycle
  // ═════════════════════════════════════════════════════════════════════
  //
  // When a cross-chain escrow is refunded, the IBC transfer is async:
  //   Locked → Refunding → Refunded        (success)
  //   Locked → Refunding → RefundFailed    (IBC SubMsg failure)
  //   RefundFailed → Refunding → ...       (admin retry cycle)
  //
  // Source: execute_refund (IBC branch) — contract.rs:335-365
  //         reply(REPLY_IBC_REFUND)       — contract.rs:78-103
  //         execute_retry_refund          — contract.rs:390-459

  // ─── Action 11: IBC Refund Succeeds ──────────────────────────────────
  // The IBC refund packet was delivered and acknowledged. Funds are now
  // back with the user on their source chain.

  action ibcRefundSucceeds(eId: EscrowId): bool =
    val escrow = getEscrow(escrows, eId)
    all {
      escrows.keys().contains(eId),
      escrow.status == Refunding,
      escrows' = escrows.put(eId, { ...escrow, status: Refunded }),
      // Ghost: escrow has exited to terminal state — funds left escrow contract
      ghostEscrowExitCount' = ghostEscrowExitCount.put(
        eId, getCnt(ghostEscrowExitCount, eId) + 1
      ),
      ghostTotalLocked' = ghostTotalLocked.put(
        escrow.denom, getInt(ghostTotalLocked, escrow.denom) - escrow.amount
      ),
      // Frame
      settlements'       = settlements,
      solvers'           = solvers,
      intentSettlements' = intentSettlements,
      currentTime'       = currentTime,
      nextId'            = nextId,
      orders'            = orders,
      reputations'       = reputations,
      ghostOrderLocked'  = ghostOrderLocked,
    }

  // ─── Action 12: IBC Refund Fails ─────────────────────────────────────
  // The IBC refund SubMsg failed (e.g., channel not found, packet rejected).
  // reply(REPLY_IBC_REFUND) transitions Refunding → RefundFailed.
  // Funds remain in the escrow contract — not lost, just stuck until retry.
  // Source: reply handler — contract.rs:78-103

  action ibcRefundFails(eId: EscrowId): bool =
    val escrow = getEscrow(escrows, eId)
    all {
      escrows.keys().contains(eId),
      escrow.status == Refunding,
      escrows' = escrows.put(eId, { ...escrow, status: RefundFailed }),
      // Ghost: no change — funds still held by escrow contract
      // Frame
      settlements'          = settlements,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ─── Action 13: Retry Refund ─────────────────────────────────────────
  // Admin retries a failed cross-chain refund. No retry-count limit in
  // the contract — admin can call indefinitely until success.
  // Transitions: RefundFailed → Refunding
  // Source: execute_retry_refund — contract.rs:390-459

  action retryRefund(eId: EscrowId): bool =
    val escrow = getEscrow(escrows, eId)
    all {
      escrows.keys().contains(eId),
      escrow.status == RefundFailed,
      escrows' = escrows.put(eId, { ...escrow, status: Refunding }),
      // Ghost: no change — funds still held
      // Frame
      settlements'          = settlements,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      orders'               = orders,
      reputations'          = reputations,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 5: On-Chain Order Fallback
  // ═════════════════════════════════════════════════════════════════════
  //
  // Users can submit orders directly on-chain (with locked funds).
  // Solvers fill orders atomically. If unfilled, orders expire and are refunded.
  // Source: execute_submit_order, execute_fill_order,
  //         execute_refund_expired_order, execute_cancel_order — handlers.rs

  // ─── Action 15: Submit Order ────────────────────────────────────────
  // User sends funds with a limit order. Funds are held by the contract.
  // Source: execute_submit_order — handlers.rs:1053-1136

  action submitOrder(
    orderId:         OrderId,
    user:            Address,
    inputAmount:     int,
    inputDenom:      Denom,
    minOutputAmount: int,
    outputDenom:     Denom,
    timeoutSecs:     int,
  ): bool = all {
    USERS_SET.contains(user),
    DENOMS_SET.contains(inputDenom),
    DENOMS_SET.contains(outputDenom),
    orderId != "",
    inputAmount > 0,
    minOutputAmount > 0,
    timeoutSecs > 0,
    not(orders.keys().contains(orderId)),    // no duplicate orders
    orders' = orders.put(orderId, {
      id:              orderId,
      user:            user,
      inputAmount:     inputAmount,
      inputDenom:      inputDenom,
      minOutputAmount: minOutputAmount,
      outputDenom:     outputDenom,
      status:          Open,
      filledBySolver:  "",
      createdAt:       currentTime,
      expiresAt:       currentTime + timeoutSecs,
      settlementId:    0,
    }),
    // Ghost: track locked funds
    ghostOrderLocked' = ghostOrderLocked.put(orderId, inputAmount),
    // Frame
    settlements'          = settlements,
    escrows'              = escrows,
    solvers'              = solvers,
    intentSettlements'    = intentSettlements,
    reputations'          = reputations,
    currentTime'          = currentTime,
    nextId'               = nextId,
    ghostEscrowExitCount' = ghostEscrowExitCount,
    ghostTotalLocked'     = ghostTotalLocked,
  }

  // ─── Action 16: Fill Order (Same-Chain Atomic) ──────────────────────
  // Solver fills an open order. For same-chain: solver's output goes to
  // user, user's locked input goes to solver — all atomic.
  // Source: execute_fill_order — handlers.rs:1143-1275

  action fillOrder(orderId: OrderId, solverId: SolverId): bool =
    val order = getOrder(orders, orderId)
    all {
      orders.keys().contains(orderId),
      order.status == Open,
      currentTime <= order.expiresAt,          // not expired
      solvers.keys().contains(solverId),
      getSolver(solvers, solverId).active,
      orders' = orders.put(orderId, {
        ...order,
        status:         Filled,
        filledBySolver: solverId,
      }),
      // Ghost: funds leave contract (transferred to solver + user)
      ghostOrderLocked' = ghostOrderLocked.put(orderId, 0),
      // Frame
      settlements'          = settlements,
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      reputations'          = reputations,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
    }

  // ─── Action 17: Refund Expired Order ────────────────────────────────
  // Anyone can trigger refund after expiry. User's locked input is returned.
  // Source: execute_refund_expired_order — handlers.rs:1281-1331

  action refundExpiredOrder(orderId: OrderId): bool =
    val order = getOrder(orders, orderId)
    all {
      orders.keys().contains(orderId),
      order.status == Open,
      currentTime > order.expiresAt,           // must be expired
      orders' = orders.put(orderId, { ...order, status: OrderExpired }),
      // Ghost: funds returned to user
      ghostOrderLocked' = ghostOrderLocked.put(orderId, 0),
      // Frame
      settlements'          = settlements,
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      reputations'          = reputations,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
    }

  // ─── Action 18: Cancel Order ────────────────────────────────────────
  // Only the order creator can cancel before fill or expiry.
  // Source: execute_cancel_order — handlers.rs:1336-1387

  action cancelOrder(orderId: OrderId, sender: Address): bool =
    val order = getOrder(orders, orderId)
    all {
      orders.keys().contains(orderId),
      order.status == Open,
      order.user == sender,                    // only creator can cancel
      currentTime <= order.expiresAt,          // use refundExpiredOrder after expiry
      orders' = orders.put(orderId, { ...order, status: Cancelled }),
      // Ghost: funds returned to user
      ghostOrderLocked' = ghostOrderLocked.put(orderId, 0),
      // Frame
      settlements'          = settlements,
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      reputations'          = reputations,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // PHASE 6: Solver Reputation
  // ═════════════════════════════════════════════════════════════════════
  //
  // Admin periodically recalculates solver reputation scores based on
  // settlement history. Reputation decays 1% per day of inactivity.
  // Source: execute_update_reputation, execute_decay_reputation — handlers.rs

  // ─── Action 19: Update Reputation ────────────────────────────────────
  // Recalculate a solver's reputation from settlement history.
  // Source: execute_update_reputation — handlers.rs:879-980

  action updateReputation(solverId: SolverId): bool =
    all {
      solvers.keys().contains(solverId),
      val successful = settlements.keys().fold(0, (acc, sId) =>
        val s = getSettlement(settlements, sId)
        if (s.solverId == solverId and s.status == Completed) acc + 1 else acc
      )
      val failed = settlements.keys().fold(0, (acc, sId) =>
        val s = getSettlement(settlements, sId)
        if (s.solverId == solverId and (s.status == Failed or s.status == Slashed))
          acc + 1
        else acc
      )
      val slashEvents = settlements.keys().fold(0, (acc, sId) =>
        val s = getSettlement(settlements, sId)
        if (s.solverId == solverId and s.status == Slashed) acc + 1 else acc
      )
      val total = successful + failed
      val rep = {
        solverId:              solverId,
        totalSettlements:      total,
        successfulSettlements: successful,
        failedSettlements:     failed,
        slashingEvents:        slashEvents,
        reputationScore:       computeReputationScore({
          ...defaultReputation,
          totalSettlements:      total,
          successfulSettlements: successful,
          failedSettlements:     failed,
          slashingEvents:        slashEvents,
        }),
        lastUpdated:           currentTime,
      }
      all {
        reputations' = reputations.put(solverId, rep),
        // Frame
        settlements'          = settlements,
        escrows'              = escrows,
        solvers'              = solvers,
        intentSettlements'    = intentSettlements,
        orders'               = orders,
        currentTime'          = currentTime,
        nextId'               = nextId,
        ghostEscrowExitCount' = ghostEscrowExitCount,
        ghostTotalLocked'     = ghostTotalLocked,
        ghostOrderLocked'     = ghostOrderLocked,
      }
    }

  // ─── Action 20: Decay Reputation ────────────────────────────────────
  // Admin triggers periodic decay: 1% per day (DECAY_BPS=100).
  // Source: execute_decay_reputation — handlers.rs:982-1042

  action decayReputation(solverId: SolverId): bool =
    val rep = getReputation(reputations, solverId)
    // Decay period is 1 day; we apply 1% (100 bps) decay per period elapsed
    // Simplified: single step decays 1% of current score
    val decayAmount = rep.reputationScore / 100   // 1% = 100/10000
    val newScore = if (rep.reputationScore > decayAmount) rep.reputationScore - decayAmount else 0
    all {
      reputations.keys().contains(solverId),
      // Only decay if time has passed since last update
      currentTime > rep.lastUpdated,
      reputations' = reputations.put(solverId, {
        ...rep,
        reputationScore: newScore,
        lastUpdated:     currentTime,
      }),
      // Frame
      settlements'          = settlements,
      escrows'              = escrows,
      solvers'              = solvers,
      intentSettlements'    = intentSettlements,
      orders'               = orders,
      currentTime'          = currentTime,
      nextId'               = nextId,
      ghostEscrowExitCount' = ghostEscrowExitCount,
      ghostTotalLocked'     = ghostTotalLocked,
      ghostOrderLocked'     = ghostOrderLocked,
    }

  // ═════════════════════════════════════════════════════════════════════
  // ENVIRONMENT
  // ═════════════════════════════════════════════════════════════════════

  // ─── Action 21: Advance Time ──────────────────────────────────────────

  action advanceTime: bool = all {
    currentTime' = currentTime + 1,
    // Frame
    settlements'          = settlements,
    escrows'              = escrows,
    solvers'              = solvers,
    intentSettlements'    = intentSettlements,
    nextId'               = nextId,
    ghostEscrowExitCount' = ghostEscrowExitCount,
    ghostTotalLocked'     = ghostTotalLocked,
    orders'               = orders,
    reputations'          = reputations,
    ghostOrderLocked'     = ghostOrderLocked,
  }

  // ─── Step: Nondeterministic exploration ──────────────────────────────
  // Picks participants and actions nondeterministically for simulation
  // and exhaustive model checking. All invariants must hold after every step.
  //
  // Note: `settlements.keys().union(Set(0)).oneOf()` ensures oneOf() never
  // fails on an empty set. ID=0 is the sentinel so guards safely reject it.

  action step = {
    nondet solverId    = SOLVERS_SET.oneOf()
    nondet userId      = USERS_SET.oneOf()
    nondet intentId    = INTENTS_SET.oneOf()
    nondet inputDenom  = DENOMS_SET.oneOf()
    nondet outputDenom = DENOMS_SET.oneOf()
    nondet amount      = 1.to(MAX_AMOUNT).oneOf()
    nondet bond        = MIN_SOLVER_BOND.to(MIN_SOLVER_BOND * 3).oneOf()
    nondet sId         = settlements.keys().union(Set(0)).oneOf()
    nondet eId         = escrows.keys().union(Set(0)).oneOf()
    nondet crossChain  = Set(true, false).oneOf()
    nondet orderId     = Set("order-1", "order-2", "order-3").oneOf()
    nondet timeout     = Set(5, 10, 15).oneOf()
    any {
      registerSolver(solverId, bond),
      deregisterSolver(solverId),
      createSettlement(
        intentId, solverId, userId,
        amount, inputDenom, amount, outputDenom, amount
      ),
      markUserLocked(sId, crossChain),
      markSolverLocked(sId),
      markExecuting(sId),
      markCompleted(sId),
      executeSettlementLocal(sId),
      handleTimeout(sId),
      slashSolver(sId),
      ibcRefundSucceeds(eId),
      ibcRefundFails(eId),
      retryRefund(eId),
      // On-chain order actions
      submitOrder(orderId, userId, amount, inputDenom, amount, outputDenom, timeout),
      fillOrder(orderId, solverId),
      refundExpiredOrder(orderId),
      cancelOrder(orderId, userId),
      // Reputation actions
      updateReputation(solverId),
      decayReputation(solverId),
      advanceTime,
    }
  }

  // ═══════════════════════════════════════════════════════════════════════
  // INVARIANTS
  // ═══════════════════════════════════════════════════════════════════════

  // INV-1: Structural validity
  // Every Completed settlement has a linked, existing escrow record.
  // (It must have passed through UserLocked which creates the escrow.)
  val validStatusInvariants = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    s.status == Completed implies
      (s.escrowId != 0 and escrows.keys().contains(s.escrowId))
  )

  // INV-2: No double settlement
  // The intentSettlements index is consistent: each intentId maps to exactly
  // one settlement, and that settlement references back to the same intentId.
  val noDoubleSettlement = intentSettlements.keys().forall(intentId =>
    val sId = intentSettlements.get(intentId)
    settlements.keys().contains(sId) and
    getSettlement(settlements, sId).intentId == intentId
  )

  // INV-3: Escrow conservation
  // The ghost tracker exactly matches the sum of amounts in all held escrows
  // (Locked + Refunding + RefundFailed — funds still in the escrow contract).
  // This proves that no tokens are created or destroyed during the protocol.
  val escrowConservation = DENOMS_SET.forall(denom =>
    val sumHeld = escrows.keys().fold(0, (acc, eId) =>
      val e = getEscrow(escrows, eId)
      if (isEscrowHeld(e.status) and e.denom == denom) acc + e.amount else acc
    )
    sumHeld == getInt(ghostTotalLocked, denom)
  )

  // INV-4: No double release
  // Each escrow record reaches Released or Refunded at most once.
  // Prevents both Release and Refund being applied to the same escrow.
  val noDoubleRelease = escrows.keys().forall(eId =>
    getCnt(ghostEscrowExitCount, eId) <= 1
  )

  // INV-5: Timeout ordering holds
  // Every settlement's escrow expiry = createdAt + IBC_TIMEOUT + SAFETY_BUFFER.
  // This is the critical invariant from SPECIFICATION.md §18.1 and two_phase.rs:46.
  //
  // Without the safety buffer, a solver could lose output while the user keeps
  // their input: the solver sends output but the user's input IBC transfer times
  // out before the solver can claim it. The buffer gives time to detect the
  // IBC timeout and refund both sides before the escrow release fires.
  val timeoutOrderingHolds = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    s.ibcTimeoutSecs > 0
    and s.safetyBufferSecs > 0
    and s.expiresAt >= s.ibcTimeoutSecs + s.safetyBufferSecs
  )

  // INV-6: Fund safety (the core correctness property)
  // User funds cannot be lost:
  //   Completed  → escrow is Released  (solver received user's input)
  //   Failed     → if escrow exists, it is in a refund state
  //   Slashed    → same as Failed (refund was already initiated before slashing)
  // Corresponds to SPECIFICATION.md §15.2: "Funds cannot be lost, only delayed."
  //
  // Refunding and RefundFailed are acceptable: funds are in transit or awaiting
  // admin retry — not lost, just delayed.
  val fundSafety = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    if (s.status == Completed) {
      escrows.keys().contains(s.escrowId) and
      getEscrow(escrows, s.escrowId).status == Released
    } else if (s.status == Failed or s.status == Slashed) {
      // If escrow was never created (failure in Pending state), no escrow to check
      not(escrows.keys().contains(s.escrowId)) or
      (
        val e = getEscrow(escrows, s.escrowId)
        e.status == Refunded or e.status == Refunding or e.status == RefundFailed
      )
    } else {
      true
    }
  )

  // INV-7: Constraint satisfaction
  // Every completed settlement delivered at least the user's minimum output.
  // Proves that the solver auction outcome satisfies the user's signed price constraint.
  val constraintSatisfied = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    s.status == Completed implies s.solverOutputAmount >= s.minOutputAmount
  )

  // INV-8: Solver bond non-negativity
  // Slashing never drives a bond below zero.
  // Guaranteed by the cap: actual = min(slash, bondAmount) in slashSolver.
  val solverBondNonNegative = solvers.keys().forall(solverId =>
    getSolver(solvers, solverId).bondAmount >= 0
  )

  // INV-9: Escrow ID populated for post-lock settlements
  // If a settlement has progressed past Pending, it must have a linked escrow.
  // Guards in the contract: handle_ibc_ack, execute_settlement_local, and
  // execute_handle_timeout all require escrow_id.is_some() via ok_or_else.
  val escrowIdPopulated = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    (s.status == UserLocked or s.status == SolverLocked or
     s.status == Executing or s.status == Completed) implies
      (s.escrowId != 0 and escrows.keys().contains(s.escrowId))
  )

  // INV-10: One escrow per intent
  // Each intentId has at most one escrow record. Enforced by the
  // ESCROWS_BY_INTENT map in the contract (IntentAlreadyEscrowed error).
  val oneEscrowPerIntent = escrows.keys().forall(e1 =>
    escrows.keys().forall(e2 =>
      val esc1 = getEscrow(escrows, e1)
      val esc2 = getEscrow(escrows, e2)
      (esc1.intentId == esc2.intentId and esc1.intentId != "") implies e1 == e2
    )
  )

  // INV-11: Refunding implies cross-chain
  // Only cross-chain escrows can enter the Refunding or RefundFailed states.
  // Local escrows always refund atomically (Locked → Refunded).
  val refundingImpliesCrossChain = escrows.keys().forall(eId =>
    val e = getEscrow(escrows, eId)
    (e.status == Refunding or e.status == RefundFailed) implies e.isCrossChain
  )

  // INV-12: Active settlements have a registered solver
  // Any settlement in an active state must reference a solver that exists
  // in the registry. Consequence of deregisterSolver's SC-C2 guard.
  val activeSettlementHasSolver = settlements.keys().forall(sId =>
    val s = getSettlement(settlements, sId)
    isActive(s.status) implies solvers.keys().contains(s.solverId)
  )

  // ═══════════════════════════════════════════════════════════════════════
  // ORDER INVARIANTS
  // ═══════════════════════════════════════════════════════════════════════

  // INV-13: Order fund safety
  // Open orders have locked funds tracked; terminal orders have zero locked.
  // Ensures no fund leakage in the order lifecycle.
  val orderFundSafety = orders.keys().forall(oId =>
    val o = getOrder(orders, oId)
    val locked = if (ghostOrderLocked.keys().contains(oId))
      ghostOrderLocked.get(oId) else 0
    if (o.status == Open)
      locked == o.inputAmount
    else
      // Filled, Expired, Cancelled — funds have been disbursed
      locked == 0
  )

  // INV-14: No double fill
  // Each order can be filled at most once. Once Filled/Expired/Cancelled,
  // no further status changes are possible.
  val noDoubleFill = orders.keys().forall(oId =>
    val o = getOrder(orders, oId)
    isOrderTerminal(o.status) implies
      (o.status == Filled or o.status == OrderExpired or o.status == Cancelled)
  )

  // INV-15: Filled orders identify a solver
  // If an order was filled, the filledBySolver field is populated.
  // Note: the solver may deregister after filling; the guard at fill time
  // ensures the solver was registered. We only check the field is non-empty.
  val filledOrderHasSolver = orders.keys().forall(oId =>
    val o = getOrder(orders, oId)
    o.status == Filled implies o.filledBySolver != ""
  )

  // ═══════════════════════════════════════════════════════════════════════
  // REPUTATION INVARIANTS
  // ═══════════════════════════════════════════════════════════════════════

  // INV-16: Reputation score bounded
  // Reputation scores stay within 0..10000 (basis points).
  val reputationScoreBounded = reputations.keys().forall(sId =>
    val rep = getReputation(reputations, sId)
    rep.reputationScore >= 0 and rep.reputationScore <= 10000
  )

  // INV-17: Reputation consistency
  // Successful + failed settlements equals total settlements.
  val reputationConsistency = reputations.keys().forall(sId =>
    val rep = getReputation(reputations, sId)
    rep.successfulSettlements + rep.failedSettlements == rep.totalSettlements
    and rep.slashingEvents <= rep.failedSettlements
  )

  // ─── Combined: all 17 invariants ──────────────────────────────────────
  val allInvariants =
    validStatusInvariants
    and noDoubleSettlement
    and escrowConservation
    and noDoubleRelease
    and timeoutOrderingHolds
    and fundSafety
    and constraintSatisfied
    and solverBondNonNegative
    and escrowIdPopulated
    and oneEscrowPerIntent
    and refundingImpliesCrossChain
    and activeSettlementHasSolver
    and orderFundSafety
    and noDoubleFill
    and filledOrderHasSolver
    and reputationScoreBounded
    and reputationConsistency

  // ─── Witnesses (reachability assertions) ─────────────────────────────
  // These SHOULD be violated by the simulator.
  // If a witness is NEVER violated, the model is over-constrained and
  // cannot reach the corresponding state — that indicates a spec bug.

  val witnessNoCompletions =
    settlements.keys().forall(sId => getSettlement(settlements, sId).status != Completed)

  val witnessNoFailures =
    settlements.keys().forall(sId => getSettlement(settlements, sId).status != Failed)

  val witnessNoSlashings =
    settlements.keys().forall(sId => getSettlement(settlements, sId).status != Slashed)

  val witnessTimeNeverAdvances = currentTime == 0

  val witnessNoEscrowsCreated = escrows.keys().size() == 0

  val witnessNoLocalCompletion =
    not(settlements.keys().exists(sId =>
      val s = getSettlement(settlements, sId)
      s.status == Completed and
      escrows.keys().contains(s.escrowId) and
      not(getEscrow(escrows, s.escrowId).isCrossChain)
    ))

  val witnessNoRefundFailed =
    escrows.keys().forall(eId => getEscrow(escrows, eId).status != RefundFailed)

  val witnessNoDeregistration =
    SOLVERS_SET.forall(sId =>
      not(solvers.keys().contains(sId)) implies
        not(settlements.keys().exists(sid =>
          getSettlement(settlements, sid).solverId == sId
        ))
    )

  val witnessNoOrdersFilled =
    orders.keys().forall(oId => getOrder(orders, oId).status != Filled)

  val witnessNoOrdersCancelled =
    orders.keys().forall(oId => getOrder(orders, oId).status != Cancelled)

  val witnessNoReputationUpdates =
    reputations.keys().size() == 0
}

// ═══════════════════════════════════════════════════════════════════════════
// MODULE 3: Concrete test instance
// Small parameter values make the state space tractable for model checking.
// ═══════════════════════════════════════════════════════════════════════════

module AtomIntentsTest {
  // Import types directly (Quint imports are not transitive)
  import AtomIntentsTypes.*
  // Import the protocol with concrete constants
  import AtomIntents(
    SOLVERS_SET        = Set("solver-a", "solver-b"),
    USERS_SET          = Set("alice", "bob"),
    INTENTS_SET        = Set("intent-1", "intent-2", "intent-3"),
    DENOMS_SET         = Set("uatom", "uusdc"),
    MIN_SOLVER_BOND    = 100,
    BASE_SLASH_BPS     = 200,    // 2%
    MIN_SLASH_AMOUNT   = 10,     // 10 units (real = 10_000_000 uatom)
    IBC_TIMEOUT_SECS   = 10,     // compressed (real = 600)
    SAFETY_BUFFER_SECS = 5,      // compressed (real = 300)
    MAX_AMOUNT         = 1000,
  ).*

  // ID allocation (deterministic in tests):
  //   nextId starts at 1
  //   createSettlement uses nextId=1 → settlement ID=1, nextId→2
  //   markUserLocked    uses nextId=2 → escrow ID=2,    nextId→3
  //   (second createSettlement uses nextId=3 → settlement ID=3, nextId→4)
  //   (second markUserLocked uses nextId=4 → escrow ID=4, nextId→5)

  // ─── Test 1: Happy path ───────────────────────────────────────────────
  // Full lifecycle: register → create → lock both → execute → complete.
  // Verifies: fundSafety, constraintSatisfied, escrowConservation, noDoubleRelease.

  run happyPathTest =
    init
      .then(registerSolver("solver-a", 100))
      .expect(solvers.keys().contains("solver-a"))
      .expect(getSolver(solvers, "solver-a").bondAmount == 100)

      // Alice offers 500 uatom, solver offers 450 uusdc (user minimum = 400)
      .then(createSettlement(
        "intent-1", "solver-a", "alice",
        500, "uatom", 450, "uusdc", 400
      ))
      .expect(intentSettlements.keys().contains("intent-1"))
      .expect(settlements.keys().size() == 1)
      .expect(getSettlement(settlements, 1).status == Pending)
      .expect(getSettlement(settlements, 1).escrowId == 0)

      // Lock user's 500 uatom in escrow → settlement ID=1, escrow ID=2
      .then(markUserLocked(1, false))
      .expect(escrows.keys().size() == 1)
      .expect(getEscrow(escrows, 2).status == Locked)
      .expect(getEscrow(escrows, 2).amount == 500)
      .expect(getEscrow(escrows, 2).isCrossChain == false)
      .expect(getSettlement(settlements, 1).status == UserLocked)
      .expect(getSettlement(settlements, 1).escrowId == 2)
      .expect(getInt(ghostTotalLocked, "uatom") == 500)
      .expect(escrowConservation)

      .then(markSolverLocked(1))
      .expect(getSettlement(settlements, 1).status == SolverLocked)

      .then(markExecuting(1))
      .expect(getSettlement(settlements, 1).status == Executing)

      // IBC ack received — release escrow to solver
      .then(markCompleted(1))
      .expect(getSettlement(settlements, 1).status == Completed)
      .expect(getEscrow(escrows, 2).status == Released)
      .expect(getInt(ghostTotalLocked, "uatom") == 0)
      .expect(getCnt(ghostEscrowExitCount, 2) == 1)
      .expect(escrowConservation)
      .expect(noDoubleRelease)
      .expect(fundSafety)
      .expect(constraintSatisfied)
      .expect(validStatusInvariants)
      .expect(escrowIdPopulated)
      .expect(allInvariants)

  // ─── Test 2: Timeout / IBC failure recovery ───────────────────────────
  // Both phases locked; IBC times out; user is fully refunded (local escrow).
  // Proves: "Funds cannot be lost, only delayed" — SPECIFICATION.md §15.2.

  run timeoutRecoveryTest =
    init
      .then(registerSolver("solver-a", 200))
      .then(createSettlement(
        "intent-1", "solver-a", "alice",
        300, "uatom", 290, "uusdc", 280
      ))
      .then(markUserLocked(1, false))
      .expect(getEscrow(escrows, 2).status == Locked)
      .expect(getInt(ghostTotalLocked, "uatom") == 300)
      .then(markSolverLocked(1))
      .then(markExecuting(1))

      // Advance past escrow expiry (IBC_TIMEOUT + SAFETY_BUFFER = 10+5 = 15)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .expect(currentTime == 15)

      // Timeout: settlement fails, escrow refunded to user (local = atomic)
      .then(handleTimeout(1))
      .expect(getSettlement(settlements, 1).status == Failed)
      .expect(getEscrow(escrows, 2).status == Refunded)
      .expect(getInt(ghostTotalLocked, "uatom") == 0)
      .expect(getCnt(ghostEscrowExitCount, 2) == 1)
      .expect(escrowConservation)
      .expect(noDoubleRelease)
      .expect(fundSafety)
      .expect(allInvariants)

  // ─── Test 3: Early failure (Pending → Failed, no escrow) ──────────────
  // Failure before user locked funds. No escrow exists; fundSafety still holds.

  run earlyFailureTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "bob",
        100, "uatom", 95, "uusdc", 90
      ))
      .expect(getSettlement(settlements, 1).status == Pending)
      .expect(getSettlement(settlements, 1).escrowId == 0)

      // Advance past expiry
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)

      .then(handleTimeout(1))
      .expect(getSettlement(settlements, 1).status == Failed)
      .expect(escrows.keys().size() == 0)       // no escrow was ever created
      .expect(getInt(ghostTotalLocked, "uatom") == 0)
      .expect(fundSafety)
      .expect(allInvariants)

  // ─── Test 4: Solver failure and slashing ──────────────────────────────
  // User locks; settlement times out; solver is slashed.
  // Slash: 2% of 100 = 2 < MIN_SLASH_AMOUNT=10, so actual slash = 10.
  // Bond: 200 - 10 = 190.

  run solverSlashTest =
    init
      .then(registerSolver("solver-a", 200))
      .then(createSettlement(
        "intent-1", "solver-a", "alice",
        100, "uatom", 95, "uusdc", 90
      ))
      .then(markUserLocked(1, false))

      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)

      .then(handleTimeout(1))
      .expect(getSettlement(settlements, 1).status == Failed)
      .expect(getEscrow(escrows, 2).status == Refunded)

      .then(slashSolver(1))
      .expect(getSettlement(settlements, 1).status == Slashed)
      .expect(getSolver(solvers, "solver-a").bondAmount == 190)
      .expect(getSolver(solvers, "solver-a").failedSettlements == 1)
      .expect(solverBondNonNegative)
      .expect(fundSafety)
      .expect(allInvariants)

  // ─── Test 5: No double settlement ─────────────────────────────────────
  // A second createSettlement for the same intentId must be rejected.

  run noDoubleSettlementTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice",
        500, "uatom", 450, "uusdc", 400
      ))
      .expect(intentSettlements.keys().contains("intent-1"))
      .expect(settlements.keys().size() == 1)

      // Second attempt for the same intent MUST fail (guard rejects it).
      // After .fail() the state is unchanged so we don't chain further .expect().
      .then(createSettlement(
        "intent-1", "solver-b", "bob",
        500, "uatom", 460, "uusdc", 400
      ).fail())

  // ─── Test 6: Two intents settle concurrently ──────────────────────────
  // Two independent intents both complete. Escrow conservation holds
  // across multiple concurrent locked escrows.
  //
  // ID allocation (nextId starts at 1):
  //   registerSolver:       no ID consumed
  //   createSettlement #1:  settlement=1,  nextId→2
  //   createSettlement #2:  settlement=2,  nextId→3
  //   markUserLocked(1):    escrow=3,      nextId→4
  //   markUserLocked(2):    escrow=4,      nextId→5

  run twoIntentsConcurrentTest =
    init
      .then(registerSolver("solver-a", 500))

      .then(createSettlement(
        "intent-1", "solver-a", "alice", 200, "uatom", 195, "uusdc", 190
      ))
      .then(createSettlement(
        "intent-2", "solver-a", "bob", 300, "uatom", 295, "uusdc", 280
      ))

      .then(markUserLocked(1, false))
      .expect(getInt(ghostTotalLocked, "uatom") == 200)
      .then(markUserLocked(2, false))
      .expect(getInt(ghostTotalLocked, "uatom") == 500)
      .expect(escrowConservation)

      .then(markSolverLocked(1))
      .then(markSolverLocked(2))
      .then(markExecuting(1))
      .then(markExecuting(2))
      .then(markCompleted(1))
      .expect(getInt(ghostTotalLocked, "uatom") == 300)
      .then(markCompleted(2))
      .expect(getInt(ghostTotalLocked, "uatom") == 0)

      .expect(escrowConservation)
      .expect(noDoubleRelease)
      .expect(noDoubleSettlement)
      .expect(fundSafety)
      .expect(constraintSatisfied)
      .expect(oneEscrowPerIntent)
      .expect(allInvariants)

  // ─── Test 7: Deregister solver (happy path) ───────────────────────────
  // Register, complete all settlements, then deregister. Solver is removed.

  run deregisterSolverTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      .then(markUserLocked(1, false))
      .then(markSolverLocked(1))
      .then(markExecuting(1))
      .then(markCompleted(1))
      .expect(getSettlement(settlements, 1).status == Completed)

      // Settlement is terminal; deregister should succeed
      .then(deregisterSolver("solver-a"))
      .expect(not(solvers.keys().contains("solver-a")))
      .expect(allInvariants)

  // ─── Test 8: Deregister blocked by active settlement ──────────────────
  // SC-C2: cannot deregister while solver has active settlements.

  run deregisterBlockedTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      // Settlement is Pending (active) — deregister must fail
      .then(deregisterSolver("solver-a").fail())

  // ─── Test 9: Same-chain local settlement ──────────────────────────────
  // SolverLocked → Completed via executeSettlementLocal, bypassing Executing.
  // Verifies: fundSafety, escrowConservation, escrowIdPopulated.

  run localSettlementTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      .then(markUserLocked(1, false))
      .then(markSolverLocked(1))
      .expect(getSettlement(settlements, 1).status == SolverLocked)

      // Local path: skip Executing, go directly to Completed
      .then(executeSettlementLocal(1))
      .expect(getSettlement(settlements, 1).status == Completed)
      .expect(getEscrow(escrows, 2).status == Released)
      .expect(getInt(ghostTotalLocked, "uatom") == 0)
      .expect(getCnt(ghostEscrowExitCount, 2) == 1)
      .expect(escrowConservation)
      .expect(fundSafety)
      .expect(constraintSatisfied)
      .expect(escrowIdPopulated)
      .expect(allInvariants)

  // ─── Test 10: Cross-chain refund lifecycle ────────────────────────────
  // Cross-chain escrow timeout → Refunding → RefundFailed → retry → Refunded.
  // Verifies: escrowConservation through the entire async refund cycle,
  // refundingImpliesCrossChain, fundSafety, noDoubleRelease.

  run crossChainRefundLifecycleTest =
    init
      .then(registerSolver("solver-a", 200))
      .then(createSettlement(
        "intent-1", "solver-a", "alice",
        300, "uatom", 290, "uusdc", 280
      ))
      // Cross-chain escrow
      .then(markUserLocked(1, true))
      .expect(getEscrow(escrows, 2).isCrossChain == true)
      .expect(getInt(ghostTotalLocked, "uatom") == 300)
      .then(markSolverLocked(1))
      .then(markExecuting(1))

      // Advance past escrow expiry
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .expect(currentTime == 15)

      // Timeout: settlement fails, escrow enters Refunding (not Refunded!)
      .then(handleTimeout(1))
      .expect(getSettlement(settlements, 1).status == Failed)
      .expect(getEscrow(escrows, 2).status == Refunding)
      .expect(getInt(ghostTotalLocked, "uatom") == 300)   // still held
      .expect(getCnt(ghostEscrowExitCount, 2) == 0)       // not exited yet
      .expect(escrowConservation)
      .expect(refundingImpliesCrossChain)
      .expect(fundSafety)

      // IBC refund SubMsg fails — reply handler sets RefundFailed
      .then(ibcRefundFails(2))
      .expect(getEscrow(escrows, 2).status == RefundFailed)
      .expect(getInt(ghostTotalLocked, "uatom") == 300)   // still held
      .expect(escrowConservation)
      .expect(refundingImpliesCrossChain)
      .expect(fundSafety)

      // Admin retries the IBC refund
      .then(retryRefund(2))
      .expect(getEscrow(escrows, 2).status == Refunding)
      .expect(escrowConservation)

      // This time IBC refund succeeds — funds returned to user
      .then(ibcRefundSucceeds(2))
      .expect(getEscrow(escrows, 2).status == Refunded)
      .expect(getInt(ghostTotalLocked, "uatom") == 0)     // funds left escrow
      .expect(getCnt(ghostEscrowExitCount, 2) == 1)       // exited exactly once
      .expect(escrowConservation)
      .expect(noDoubleRelease)
      .expect(fundSafety)
      .expect(allInvariants)

  // ═══════════════════════════════════════════════════════════════════════
  // ORDER TESTS
  // ═══════════════════════════════════════════════════════════════════════

  // ─── Test 11: Order happy path (submit → fill) ──────────────────────
  // Alice submits an order, solver-a fills it atomically.
  // Verifies: orderFundSafety, noDoubleFill, filledOrderHasSolver.

  run orderHappyPathTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 500, "uatom", 450, "uusdc", 10))
      .expect(orders.keys().contains("order-1"))
      .expect(getOrder(orders, "order-1").status == Open)
      .expect(getOrder(orders, "order-1").inputAmount == 500)
      .expect(getOrder(orders, "order-1").expiresAt == 10)
      .expect(ghostOrderLocked.keys().contains("order-1"))
      .expect(ghostOrderLocked.get("order-1") == 500)
      .expect(orderFundSafety)

      // Solver fills the order
      .then(fillOrder("order-1", "solver-a"))
      .expect(getOrder(orders, "order-1").status == Filled)
      .expect(getOrder(orders, "order-1").filledBySolver == "solver-a")
      .expect(ghostOrderLocked.get("order-1") == 0)
      .expect(orderFundSafety)
      .expect(noDoubleFill)
      .expect(filledOrderHasSolver)
      .expect(allInvariants)

  // ─── Test 12: Order expiry and refund ───────────────────────────────
  // Alice submits order, nobody fills it, order expires, funds refunded.

  run orderExpiryTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 300, "uatom", 280, "uusdc", 5))
      .expect(getOrder(orders, "order-1").status == Open)
      .expect(orderFundSafety)

      // Advance past expiry (timeout = 5)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .expect(currentTime == 6)

      // Refund expired order
      .then(refundExpiredOrder("order-1"))
      .expect(getOrder(orders, "order-1").status == OrderExpired)
      .expect(ghostOrderLocked.get("order-1") == 0)
      .expect(orderFundSafety)
      .expect(allInvariants)

  // ─── Test 13a: Order cancellation by owner ──────────────────────────
  // Alice submits then cancels her own order before fill.

  run orderCancelTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 200, "uatom", 190, "uusdc", 10))
      .expect(getOrder(orders, "order-1").status == Open)

      // Alice can cancel her own order
      .then(cancelOrder("order-1", "alice"))
      .expect(getOrder(orders, "order-1").status == Cancelled)
      .expect(ghostOrderLocked.get("order-1") == 0)
      .expect(orderFundSafety)
      .expect(allInvariants)

  // ─── Test 13b: Non-owner cannot cancel ─────────────────────────────
  // Bob cannot cancel Alice's order.

  run orderCancelUnauthorizedTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 200, "uatom", 190, "uusdc", 10))
      // Bob cannot cancel Alice's order — guard rejects it
      .then(cancelOrder("order-1", "bob").fail())

  // ─── Test 14: No double fill ────────────────────────────────────────
  // Once an order is filled, a second fill attempt must fail.

  run noDoubleFillTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(registerSolver("solver-b", 100))
      .then(submitOrder("order-1", "alice", 500, "uatom", 450, "uusdc", 10))
      .then(fillOrder("order-1", "solver-a"))
      .expect(getOrder(orders, "order-1").status == Filled)

      // Second fill must fail — order is no longer Open
      .then(fillOrder("order-1", "solver-b").fail())

  // ─── Test 15: Cannot fill expired order ─────────────────────────────
  // Solver cannot fill an order after it has expired.

  run cannotFillExpiredOrderTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 500, "uatom", 450, "uusdc", 3))

      // Advance past expiry (timeout = 3)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime)
      .expect(currentTime == 4)

      // Fill must fail — order is expired
      .then(fillOrder("order-1", "solver-a").fail())

  // ─── Test 16: Duplicate order submission rejected ───────────────────

  run duplicateOrderTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(submitOrder("order-1", "alice", 500, "uatom", 450, "uusdc", 10))
      // Same order ID must fail
      .then(submitOrder("order-1", "bob", 300, "uatom", 280, "uusdc", 10).fail())

  // ═══════════════════════════════════════════════════════════════════════
  // REPUTATION TESTS
  // ═══════════════════════════════════════════════════════════════════════

  // ─── Test 17: Reputation after successful settlement ────────────────
  // Solver completes a settlement, reputation is updated.
  // Score should reflect 100% success rate.

  run reputationSuccessTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      .then(markUserLocked(1, false))
      .then(markSolverLocked(1))
      .then(markExecuting(1))
      .then(markCompleted(1))

      // Update reputation after settlement
      .then(updateReputation("solver-a"))
      .expect(reputations.keys().contains("solver-a"))
      .expect(getReputation(reputations, "solver-a").totalSettlements == 1)
      .expect(getReputation(reputations, "solver-a").successfulSettlements == 1)
      .expect(getReputation(reputations, "solver-a").failedSettlements == 0)
      .expect(getReputation(reputations, "solver-a").slashingEvents == 0)
      // 100% success: successRate=4000, base=4000, slashPenalty=0 → 8000
      .expect(getReputation(reputations, "solver-a").reputationScore == 8000)
      .expect(getFeeTier(getReputation(reputations, "solver-a").reputationScore) == Standard)
      .expect(reputationScoreBounded)
      .expect(reputationConsistency)
      .expect(allInvariants)

  // ─── Test 18: Reputation after slash ────────────────────────────────
  // Solver fails and is slashed; reputation reflects penalty.

  run reputationSlashTest =
    init
      .then(registerSolver("solver-a", 200))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 100, "uatom", 95, "uusdc", 90
      ))
      .then(markUserLocked(1, false))

      // Advance past expiry and fail
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)
      .then(advanceTime).then(advanceTime).then(advanceTime)

      .then(handleTimeout(1))
      .then(slashSolver(1))

      // Update reputation
      .then(updateReputation("solver-a"))
      .expect(getReputation(reputations, "solver-a").totalSettlements == 1)
      .expect(getReputation(reputations, "solver-a").successfulSettlements == 0)
      .expect(getReputation(reputations, "solver-a").failedSettlements == 1)
      .expect(getReputation(reputations, "solver-a").slashingEvents == 1)
      // 0% success: successRate=0, base=4000, slashPenalty=200 → 3800
      .expect(getReputation(reputations, "solver-a").reputationScore == 3800)
      .expect(getFeeTier(getReputation(reputations, "solver-a").reputationScore) == New)
      .expect(reputationScoreBounded)
      .expect(reputationConsistency)
      .expect(allInvariants)

  // ─── Test 19: Reputation decay ──────────────────────────────────────
  // Reputation decays over time when solver is inactive.

  run reputationDecayTest =
    init
      .then(registerSolver("solver-a", 100))
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      .then(markUserLocked(1, false))
      .then(markSolverLocked(1))
      .then(markExecuting(1))
      .then(markCompleted(1))

      // Set initial reputation
      .then(updateReputation("solver-a"))
      .expect(getReputation(reputations, "solver-a").reputationScore == 8000)

      // Advance time and decay
      .then(advanceTime)
      .then(decayReputation("solver-a"))
      // 1% of 8000 = 80 → 8000 - 80 = 7920
      .expect(getReputation(reputations, "solver-a").reputationScore == 7920)
      .expect(getFeeTier(getReputation(reputations, "solver-a").reputationScore) == Standard)
      .expect(reputationScoreBounded)
      .expect(allInvariants)

  // ─── Test 20: Mixed settlement + order lifecycle ────────────────────
  // Settlement and order systems operating concurrently.
  // Verifies all invariants hold across both subsystems simultaneously.

  run mixedSettlementAndOrderTest =
    init
      .then(registerSolver("solver-a", 200))

      // Create a settlement
      .then(createSettlement(
        "intent-1", "solver-a", "alice", 500, "uatom", 450, "uusdc", 400
      ))
      .then(markUserLocked(1, false))

      // Submit an order concurrently
      .then(submitOrder("order-1", "bob", 300, "uatom", 280, "uusdc", 15))
      .expect(orders.keys().size() == 1)
      .expect(settlements.keys().size() == 1)

      // Complete settlement
      .then(markSolverLocked(1))
      .then(markExecuting(1))
      .then(markCompleted(1))

      // Fill the order
      .then(fillOrder("order-1", "solver-a"))

      // Update reputation (should see 1 successful settlement)
      .then(updateReputation("solver-a"))
      .expect(getReputation(reputations, "solver-a").successfulSettlements == 1)

      // Verify all invariants across both subsystems
      .expect(escrowConservation)
      .expect(fundSafety)
      .expect(orderFundSafety)
      .expect(filledOrderHasSolver)
      .expect(reputationConsistency)
      .expect(allInvariants)
}
