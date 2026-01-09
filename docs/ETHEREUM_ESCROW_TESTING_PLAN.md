# Ethereum Escrow Testing Plan

This document outlines the comprehensive testing strategy for the Ethereum Escrow via IBC Eureka feature.

## Overview

The Ethereum escrow system enables users to escrow funds on Ethereum and have solvers front the settlement on Cosmos before ZK proof finality. This creates a complex state machine with multiple actors (users, solvers, relayers, admin) and requires thorough testing at multiple levels.

## Test Levels

### Level 1: Unit Tests

**Status: Complete**

Unit tests verify individual components in isolation.

#### Escrow Contract Tests (68 tests)

| Test Category | Count | Description |
|---------------|-------|-------------|
| State transitions | 6 | Pending → Received → Finalized → Claimed |
| Authorization | 8 | Admin, settlement contract, solver permissions |
| Bond validation | 3 | Insufficient bond rejected, bond returned on failure |
| Sender verification | 2 | Ethereum address matching |
| Timeout handling | 4 | Packet received after timeout fails |
| Error cases | 10 | Duplicate intents, wrong status, wrong solver |
| Query functions | 5 | Config, escrow status, escrow by intent |

#### Risk Pricing Tests (in `crates/types`)

| Test | Description |
|------|-------------|
| `test_default_ethereum_l1_pricing` | 20 min finality, 1.5x bond |
| `test_default_op_stack_l2_pricing` | 20 min finality for Base/Optimism |
| `test_default_zk_rollup_pricing` | 15 min finality |
| `test_required_bond_calculation` | 1.5x multiplier calculation |
| `test_fronting_assessment_*` | Expected value calculations |

### Level 2: Adversarial Tests

**Status: Complete (21 tests)**

Adversarial tests verify security and economic robustness against attack vectors.

#### Economic Attack Tests

| Test | Attack Vector | Expected Result |
|------|--------------|-----------------|
| `test_griefing_register_without_sending_eth` | Register intent, never send ETH | Admin can fail, no solver loss |
| `test_frontrunning_double_front_blocked` | Solver1 fronts, Solver2 tries to front | Second attempt fails |
| `test_claim_theft_blocked` | Attacker claims escrow they didn't front | Unauthorized error |
| `test_bond_returned_on_non_fault_failure` | Fail escrow after fronting | Bond returned to solver |
| `test_timeout_edge_case_exact_boundary` | Submit packet at exact timeout | Rejected |
| `test_timeout_edge_case_just_before` | Submit packet 1 second before timeout | Accepted |

#### Security Tests

| Test | Attack Vector | Expected Result |
|------|--------------|-----------------|
| `test_replay_protection_duplicate_intent` | Same intent ID twice | Second rejected |
| `test_amount_mismatch_less_than_expected` | Received < expected | Accepted (solver decides) |
| `test_fake_finalization_blocked` | Non-admin finalizes | Unauthorized |
| `test_fake_packet_notification_blocked` | Non-admin notifies packet | Unauthorized |
| `test_unauthorized_registration_blocked` | Random user registers | Unauthorized |
| `test_wrong_packet_id_claim_blocked` | Wrong packet ID on claim | PacketIdMismatch |
| `test_ethereum_sender_spoofing_blocked` | Packet from different ETH address | SenderMismatch |
| `test_double_claim_blocked` | Claim same escrow twice | Invalid status |
| `test_unauthorized_failure_marking_blocked` | Attacker fails escrow | Unauthorized |
| `test_fail_claimed_escrow_blocked` | Fail already claimed | Invalid status |
| `test_front_insufficient_bond_blocked` | Bond less than declared | InsufficientBond |
| `test_claim_without_fronting_blocked` | Claim without having fronted | NotFronted |
| `test_front_wrong_state_blocked` | Front when not Received | Invalid status |
| `test_finalize_wrong_state_blocked` | Finalize when not Received/Fronted | Invalid status |
| `test_solver_multiple_intents` | Solver fronts multiple intents | All work independently |

### Level 3: Integration Tests

**Status: Partially Complete (28 tests)**

Integration tests verify multi-component interactions.

#### Completed

- Full happy path: Register → Receive → Front → Finalize → Claim
- Failure paths: Timeout, ZK proof failure, invalid packet
- Mock Eureka flow with `MockEurekaMonitor`

#### Still Needed

```
□ Multi-contract integration (escrow + settlement contract interaction)
□ IBC packet simulation (mock IBC module responses)
□ Solver competition (multiple solvers racing to front)
□ Reorg simulation (packet received then reverted)
□ Cross-chain message verification
```

### Level 4: Testnet Testing

**Status: Not Started**

#### Phase A: Cosmos Hub Testnet (provider)

Prerequisites:
- Deploy updated escrow contract to `provider` testnet
- Fund test accounts with testnet ATOM
- Configure admin account for simulating relayer

Test Steps:
```
□ 1. Deploy escrow contract with settlement contract configured
□ 2. Register Ethereum escrow intent via settlement contract
□ 3. Simulate packet receipt (admin calls NotifyEurekaPacketReceived)
□ 4. Test solver fronting with real ATOM testnet tokens
□ 5. Simulate finalization (admin calls NotifyEurekaFinalized)
□ 6. Verify claim releases correct amounts
□ 7. Test failure scenarios (timeout, invalid packet)
□ 8. Verify bond handling in failure cases
```

Expected Results:
- All state transitions work correctly
- Funds move correctly between accounts
- Events emitted for indexing

#### Phase B: IBC Eureka Testnet

Prerequisites:
- Access to Eureka-enabled testnet
- Ethereum Sepolia testnet setup
- Relayer configured for Eureka packets

Test Steps:
```
□ 1. Connect to Eureka-enabled testnet
□ 2. Deploy escrow contract on Cosmos side
□ 3. Send actual packet from Ethereum Sepolia
□ 4. Verify packet receipt detection by relayer
□ 5. Wait for ZK proof finality (~20 min)
□ 6. Verify NotifyEurekaFinalized called by relayer
□ 7. Complete full flow with real cross-chain transfer
□ 8. Test timeout scenarios (let packet expire)
```

Expected Results:
- End-to-end flow completes successfully
- ZK proof finality detected correctly
- Timing matches expected finality windows

### Level 5: Performance Testing

**Status: Not Started**

| Test | Metric | Target |
|------|--------|--------|
| High volume | 100+ concurrent intents | No failures, <100ms per operation |
| State bloat | Storage growth rate | <1KB per intent |
| Gas costs | Gas per operation | Document baseline |
| Finality latency | Time from packet to ZK proof | ~20 min for Ethereum L1 |

### Level 6: Mainnet Readiness

**Status: Not Started**

Pre-mainnet checklist:
```
□ All unit tests passing
□ All adversarial tests passing
□ Integration tests with mock Eureka passing
□ Testnet validation complete
□ Performance benchmarks documented
□ Security audit completed
□ Runbook for incident response
□ Monitoring dashboards configured
□ Circuit breaker mechanisms tested
```

## Test Infrastructure

| Component | Status | Description |
|-----------|--------|-------------|
| Mock Eureka Monitor | Complete | Simulates packet status in tests |
| Escrow Test Helpers | Complete | Setup contracts, create intents |
| Adversarial Test Framework | Complete | Attack simulation helpers |
| IBC Mock Module | Not Started | Simulate IBC callbacks |
| Ethereum Testnet Integration | Not Started | Sepolia connection |
| Metrics Collection | Not Started | Track test results over time |

## Test Scenarios

### Scenario 1: Happy Path

```
Actors: User, Solver, Relayer (simulated by Admin)

1. User initiates intent for 1 ETH → 100 ATOM swap
2. Settlement contract calls RegisterEthereumEscrowIntent
   - Expected: Escrow created in Pending status
3. User sends 1 ETH on Ethereum via Eureka
4. Relayer detects packet, calls NotifyEurekaPacketReceived
   - Expected: Status → Received
5. Solver evaluates and calls FrontSettlement with 150 ATOM (100 output + 50 bond)
   - Expected: Status → Fronted, FrontingInfo recorded
6. User receives 100 ATOM immediately
7. ZK proof finalizes after ~20 min
8. Relayer calls NotifyEurekaFinalized
   - Expected: Status → Finalized
9. Solver calls ClaimEurekaEscrow
   - Expected: Status → Claimed, Solver receives 1 ETH equivalent + 50 ATOM bond
```

### Scenario 2: Solver Takes Loss on Failure

```
Actors: User, Solver, Relayer, Admin

1. User registers intent, solver fronts with bond
2. User receives funds immediately
3. Ethereum reorg causes Eureka packet to fail
4. Relayer detects failure, Admin calls HandleEurekaEscrowFailure
   - Expected: Status → Failed
   - Expected: Solver bond returned (non-fault failure)
   - Note: Solver loses fronted capital but keeps bond
```

### Scenario 3: Timeout Before Packet

```
Actors: User, Admin

1. User registers intent with 20 min timeout
2. No packet arrives within timeout
3. Admin marks as failed after timeout
   - Expected: Status → Failed
   - Expected: Intent cleaned up, no solver loss (no one fronted)
```

### Scenario 4: Solver Competition

```
Actors: User, Solver1, Solver2, Relayer

1. User registers intent
2. Relayer notifies packet received
3. Solver1 and Solver2 both try to front
4. Solver1's transaction included first
   - Expected: Solver1 fronts successfully
   - Expected: Solver2's front attempt fails (already fronted)
5. Only Solver1 can claim after finality
```

### Scenario 5: Amount Mismatch

```
Actors: User, Solver, Relayer

1. User registers intent expecting 1 ETH
2. Due to fees, only 0.99 ETH arrives
3. Relayer notifies with actual amount (0.99 ETH)
   - Expected: Accepted (amount mismatch is informational)
4. Solver sees mismatch, decides whether to front
5. If solver fronts, they accept the risk of the mismatch
```

## Running Tests

### Unit Tests
```bash
cargo test -p atom-intents-escrow
```

### Adversarial Tests
```bash
cargo test -p atom-intents-escrow --test adversarial_ethereum_escrow_tests
```

### All Escrow Tests
```bash
cargo test -p atom-intents-escrow 2>&1 | grep -E "test.*ok|passed"
```

### Full Workspace
```bash
cargo test --workspace
```

## Test Coverage Goals

| Category | Target | Current |
|----------|--------|---------|
| State transitions | 100% | 100% |
| Authorization checks | 100% | 100% |
| Error conditions | 95% | 90% |
| Economic attacks | 90% | 85% |
| Security attacks | 95% | 95% |
| Integration flows | 80% | 60% |
| Testnet validation | 100% | 0% |

## Appendix: Error Types Tested

| Error | Test Coverage |
|-------|--------------|
| `Unauthorized` | 8 tests |
| `EthereumEscrowNotFound` | Implicit in many tests |
| `InvalidEthereumEscrowStatus` | 6 tests |
| `EthereumSenderMismatch` | 1 test |
| `EurekaTimeout` | 2 tests |
| `PacketIdMismatch` | 1 test |
| `NotFronted` | 1 test |
| `InsufficientBond` | 2 tests |
| `IntentAlreadyEscrowed` | 2 tests |
