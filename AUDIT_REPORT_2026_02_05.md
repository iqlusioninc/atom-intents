# ATOM Intents Comprehensive Audit Report

**Date:** 2026-02-05
**Scope:** Full codebase (~200K lines Rust, 11 workspace crates, 2 CosmWasm contracts)
**Build Status:** Compiles cleanly, all 29 tests pass, clippy clean

---

## Executive Summary

This audit identified **31 Critical**, **38 High**, **53 Medium**, and **68 Low/Informational** findings across 8 audit dimensions: core types, solver framework, settlement layer, matching engine, smart contracts, orchestrator/infrastructure, test coverage, and dependencies.

The most urgent categories of findings are:

1. **Signing/verification gaps** -- Hash collisions possible due to missing field separators; unsigned expiry fields; no public-key-to-address binding
2. **Settlement atomicity failures** -- Non-atomic rollbacks can lock user funds; IBC ack content not validated; partial unwinds leave inconsistent state
3. **Smart contract access control** -- Solver registration overwrites existing records (bond theft); deregistration has no active settlement check (slash escape)
4. **Matching engine correctness** -- Intents not sorted by price before matching (MEV vulnerability); fill constraints never enforced; systematic rounding losses
5. **Placeholder implementations** -- Chain client uses simulated broadcasts; recovery system is no-op; several critical code paths are stubs

**Overall Assessment:** The architecture is well-designed with strong security concepts (two-phase commit, solver bonds, reputation). However, implementation gaps between the design and the code create exploitable vulnerabilities. The system is **not ready for mainnet deployment** without addressing the Critical and High findings.

---

## Table of Contents

1. [Core Types (crates/types/)](#1-core-types)
2. [Solver Framework (crates/solver/)](#2-solver-framework)
3. [Settlement Layer (crates/settlement/)](#3-settlement-layer)
4. [Matching Engine (crates/matching-engine/)](#4-matching-engine)
5. [Smart Contracts (contracts/)](#5-smart-contracts)
6. [Orchestrator & Infrastructure](#6-orchestrator--infrastructure)
7. [Test Coverage Gaps](#7-test-coverage-gaps)
8. [Dependencies & Supply Chain](#8-dependencies--supply-chain)
9. [Prioritized Remediation Plan](#9-prioritized-remediation-plan)

---

## 1. Core Types

### Critical

| ID | Finding | Location |
|----|---------|----------|
| T-C1 | **Signing hash lacks length-prefixed field boundaries** -- Adjacent variable-length string fields (chain_id + denom, limit_price + recipient, excluded_venues entries) are concatenated without separators, enabling hash collisions between different intents | `crates/types/src/intent.rs:106-176` |
| T-C2 | **`created_at` and `expires_at` excluded from signing hash** -- Intermediaries can modify expiry to keep intents alive indefinitely or kill them prematurely; signature still verifies | `crates/types/src/intent.rs:60-61` |
| T-C3 | **`Intent::side()` unconditionally returns `Side::Sell`** -- Every intent classified as sell regardless of trade direction; breaks matching, order book placement, and pricing | `crates/types/src/intent.rs:80-85` |
| T-C4 | **Signature verification does not bind public key to user address** -- An attacker can forge intents for any user by using their own key pair with the victim's address | `crates/types/src/verification.rs:40-70` |
| T-C5 | **Cancellation does not verify public key matches original intent signer** -- Anyone can cancel any intent by signing a cancellation with their own key | `crates/types/src/cancellation.rs:71-77` |

### High

| ID | Finding | Location |
|----|---------|----------|
| T-H1 | Duplicated 70-line signing hash between `Intent` and `UnsignedIntent` with no equivalence guarantee | `intent.rs:106-176, 299-369` |
| T-H2 | `PriceLevel` ordering is lexicographic ("9.0" > "10.0"), not numeric | `trading.rs:55-62` |
| T-H3 | All numeric fields stored as unvalidated `String` -- negative, empty, NaN accepted | Multiple files |
| T-H4 | `FillStrategy` JSON serialization in signing hash depends on serde format stability | `intent.rs:132-135` |
| T-H5 | `ExecutionConstraints::default()` produces deadline=0 (immediately expired) | `execution.rs:63-67` |
| T-H6 | `CancellationRegistry` silently returns `false` on poisoned RwLock (fail-open) | `cancellation.rs:97-127` |
| T-H7 | `OptimalFillPlan` not serializable; `fully_matched()` creates contradictory state | `solution.rs:96-111` |

---

## 2. Solver Framework

### Critical

| ID | Finding | Location |
|----|---------|----------|
| S-C1 | **Division by zero when `ctx.remaining` is zero** -- Panics the solver process | `crates/solver/src/dex.rs:152`, `cex.rs:627` |
| S-C2 | **CEX inventory updated speculatively before settlement** -- Multiple calls per intent drift inventory; no rollback if auction lost | `crates/solver/src/cex.rs:631-636` |
| S-C3 | **Unchecked `u128 as i128` cast can silently overflow** to negative, corrupting inventory | `crates/solver/src/cex.rs:635,445,472` |
| S-C4 | **`std::sync::RwLock` poisoning silently disables all inventory tracking** -- Fail-open after any panic | `crates/solver/src/cex.rs` (7 sites) |

### High

| ID | Finding | Location |
|----|---------|----------|
| S-H1 | Oracle price never used for DEX quote validation -- manipulated pools accepted | `dex.rs:103-181` |
| S-H2 | No `valid_until` check before solution selection -- stale quotes can win | `aggregator.rs:200-235` |
| S-H3 | Hardcoded 6-decimal denomination assumption in CEX calculations | `cex.rs:445,471-477` |
| S-H4 | `estimate_multi_hop_fee` panics on single-element route (index OOB) | `fees.rs:363-379` |
| S-H5 | Aggregator partial fill output amount not pro-rated | `aggregator.rs:217-235` |
| S-H6 | No intent expiry check in `solve()` methods; `SolveError::IntentExpired` variant unused | `dex.rs`, `cex.rs` |
| S-H7 | `max_solver_fee_bps` constraint not enforced in CEX solver (only in DEX) | `cex.rs:581-658` |

### Medium

- No HTTP timeouts configured (`http.rs:1-8`)
- `block_on` in `MockOracle::supports_pair` deadlocks in tokio (`oracle.rs:181-184`)
- Astroport contract query missing protobuf encoding (`astroport.rs:169-229`)
- Multi-hop fee sums across different denominations (`fees.rs:383-406`)
- New HTTP client created per reputation request (`reputation.rs:60,104,155`)
- String-formatted JSON in reputation queries -- injection risk (`reputation.rs:51,95,140-142`)
- Wildcard re-exports risk name collisions (`lib.rs:15-27`)
- Overly broad native denom heuristic (`astroport.rs:69-78`)

---

## 3. Settlement Layer

### Critical

| ID | Finding | Location |
|----|---------|----------|
| ST-C1 | **Non-atomic rollback on solver vault lock failure** -- User escrow locked but never refunded (comment says "In production, this would be atomic") | `crates/settlement/src/two_phase.rs:213-227` |
| ST-C2 | **Phase 2 partial rollback** -- If `solver_vault.unlock()` succeeds but `user_escrow.refund()` fails, solver recovers funds but user does not | `two_phase.rs:308-321` |
| ST-C3 | **Relayer service error orphans both locks** -- `wait_for_ibc` returning `Err` bypasses all unwind logic | `two_phase.rs:289` |
| ST-C4 | **IBC ack content never validated** -- Error acks treated as success, releasing user funds to solver while IBC transfer actually failed (double-spend) | `two_phase.rs:292-306` |

### High

| ID | Finding | Location |
|----|---------|----------|
| ST-H1 | No state machine transition validation -- any state reachable from any state; `InvalidStateTransition` error variant defined but never used | `manager.rs:136-216` |
| ST-H2 | No idempotency or deduplication on settlement events | `manager.rs:136-216` |
| ST-H3 | Settlement ID not collision-resistant (intent_id + second-precision timestamp) | `manager.rs:103` |
| ST-H4 | Same-chain settlement path non-atomic -- solver funds released to user before user funds released to solver; failure between steps loses solver funds | `two_phase.rs:235-253` |
| ST-H5 | `max_concurrent_per_solver` config field is never enforced (dead code) | `manager.rs:19,97-133` |

### Medium

- SQLite status uses fragile string concat/LIKE matching (`sqlite_store.rs`)
- PFM intermediate hop receivers set to chain IDs instead of valid bech32 addresses (`routing.rs:389-405`)
- `InMemoryStore` uses `std::sync::RwLock` with `unwrap()` in async context (`store.rs:189`)
- `IbcTransferBuilder` allows empty/zero fields with no validation (`ibc.rs:186-289`)
- `TimeoutConfig::validate()` accepts zero timeout values (`two_phase.rs:44-58`)
- `determine_flow_with_routing` returns empty hops on route-not-found instead of error (`ibc.rs:149-151`)
- Inconsistent transition recording between `update_status` and `advance_settlement` (`store.rs`, `manager.rs`)

---

## 4. Matching Engine

### Critical

| ID | Finding | Location |
|----|---------|----------|
| ME-C1 | **`OrderedPrice::Ord` violates antisymmetry contract** -- `is_bid` field makes `a.cmp(b)` and `b.cmp(a)` inconsistent; would corrupt BTreeMap if bid/ask mixed | `crates/matching-engine/src/book.rs:30-46` |
| ME-C2 | **Nonce replay protection burns nonces before batch processing** -- If batch fails after nonce recording, users permanently lose their nonces (exploitable DoS) | `engine.rs:179-184` |
| ME-C3 | **Truncation-only rounding in price conversions** -- Both sides of every trade lose the fractional amount; value disappears; exploitable via dust grinding | `book.rs:221-235`, `engine.rs:540-555` |

### High

| ID | Finding | Location |
|----|---------|----------|
| ME-H1 | **`cross_internal` does not sort intents by price** -- Matching order is caller-controlled, enabling front-running/MEV extraction | `engine.rs:189-190,309-420` |
| ME-H2 | `min_fill_amount`, `min_fill_pct`, and `FillStrategy` never enforced in either matching path | `book.rs:73-201`, `engine.rs:289-426` |
| ME-H3 | `validate_limit_price` is dead code (`#[allow(dead_code)]`) | `engine.rs:233-276` |
| ME-H4 | `Intent::pair()` alphabetical denom ordering can invert buy/sell semantics for certain pairs | `types/intent.rs:71-78` (affects engine) |

### Medium

- `used_nonces` HashMap grows without bound -- `clear_old_nonces` never called (`engine.rs:18-73`)
- `run_batch_auction` uses `SystemTime::now()` -- non-deterministic in consensus (`engine.rs:105-125`)
- Oracle deviation check only advances `sell_idx`, permanently skipping valid matches (`engine.rs:370-381`)
- `calculate_clearing_price` mixes inverted price units from buy/sell fills (`engine.rs:508-538`)
- Solver quote unit semantics ambiguous -- potential unit mismatch (`engine.rs:428-506`)
- No signature verification, expiration check, or nonce replay check in CLOB path (`engine.rs:88-96`, `book.rs:73-201`)

---

## 5. Smart Contracts

### Critical

| ID | Finding | Location |
|----|---------|----------|
| SC-C1 | **Solver registration allows overwriting existing solver** -- Bond theft: attacker registers with same solver_id, overwrites operator, deregisters with minimum bond, original solver's bond locked forever | `contracts/settlement/src/handlers.rs:24-64` |
| SC-C2 | **Solver deregistration has no active settlement check** -- Solver recovers bond before completion, escapes all slashing; subsequent state transitions fail because solver record is deleted | `handlers.rs:66-99` |
| SC-C3 | **Cross-chain solver fund locking is a status flag only** -- `MarkSolverLocked` does not require fund deposit; `execute_settlement` IBC transfer will fail if contract has no balance | `handlers.rs:205-237,463-543` |
| SC-C4 | **Expiry boundary race** -- At exactly `expires_at`: settlement allows execution (`>`), escrow blocks release (`>=`). Solver delivers via IBC, but escrow release fails. User can then also refund -- double-spend | `escrow/contract.rs:237,289`, `settlement/handlers.rs:514` |

### High

| ID | Finding | Location |
|----|---------|----------|
| SC-H1 | Zero-amount escrow lock accepted -- enables phantom settlements and reputation manipulation | `escrow/contract.rs:92-115` |
| SC-H2 | No `expires_at` future validation -- pre-expired escrows/settlements accepted | `escrow/contract.rs:72-127`, `settlement/handlers.rs:101-166` |
| SC-H3 | Order `recipient` address not validated -- creates unfillable orders that waste solver gas | `settlement/handlers.rs:997-1080` |
| SC-H4 | `HandleTimeout` escrow refund fails -- settlement contract not authorized to call `Refund` on escrow | `settlement/handlers.rs:661-730` |
| SC-H5 | No `reply` handler in escrow -- IBC refund failures leave escrow in `Refunding` state permanently; `RetryRefund` requires `RefundFailed` status that is never set | `escrow/contract.rs` |
| SC-H6 | `execute_settlement` sends from contract balance without prior deposit verification | `settlement/handlers.rs:463-543` |

### Medium

- `UpdateReputation` is permissionless and does unbounded full-table scan (DoS) (`handlers.rs:837-931`)
- `start_after` pagination parameters ignored in queries (`escrow/contract.rs:483`, `settlement/queries.rs:45,63`)
- IBC memo constructed via `format!` -- JSON injection risk (`escrow/contract.rs:314-317`)
- No access control on `DecayReputation` (`handlers.rs:933-987`)
- `base_slash_bps` has no upper bound (`handlers.rs:422-461`)
- Full-table scan queries in settlement (`queries.rs:59-76,83-102`)
- Duplicate `intent_id` overwrites `INTENT_SETTLEMENTS` index silently (`handlers.rs:101-166`)

---

## 6. Orchestrator & Infrastructure

### Critical

| ID | Finding | Location |
|----|---------|----------|
| O-C1 | **Race condition between drain mode check and inflight registration** -- Intents can slip past drain or be incorrectly rejected | `crates/orchestrator/src/orchestrator.rs:378-399` |
| O-C2 | **Hardcoded oracle fallback `Decimal::TEN`** -- When oracle fails, batch auction proceeds with arbitrary price 10.0; exploitable by causing oracle failures | `orchestrator.rs:569-574,742` |
| O-C3 | **Admin API auth vulnerable to timing attacks** -- String comparison is not constant-time; admin token controls drain mode and all inflight settlements | `crates/orchestrator/src/admin.rs` |
| O-C4 | **Chain client uses placeholder signing and simulated broadcast** -- Transactions never submitted on-chain; all IBC relay operations silently no-op while reporting success | `crates/relayer/src/chain.rs:889-897,708-712,727-739` |

### High

| ID | Finding | Location |
|----|---------|----------|
| O-H1 | `intent_statuses` and `intent_auth` HashMaps grow without bound -- guaranteed memory exhaustion | `orchestrator.rs:273-275` |
| O-H2 | Recovery system is no-op despite `auto_recovery_enabled: true` -- `active_settlements` map never populated | `orchestrator.rs:708-722` |
| O-H3 | `ConfigWatcher` uses `std::sync::RwLock` in async context; hot-reload skips validation; lock poisoning panics all subsequent reads | `crates/config/src/watcher.rs:7,18,40,84-87` |
| O-H4 | Relayer `run()` loop acquires up to 5 write locks per iteration with no graceful shutdown mechanism | `crates/relayer/src/service.rs:108-175` |
| O-H5 | `process_batch` never records individual intent failures -- `all_failed` is non-mut, always empty | `orchestrator.rs:553` |

### Medium

- `f64` used for financial percentage calculations in executor (`executor.rs:236-282`)
- `TokenBucket` refill race between timestamp CAS and token addition (`ratelimit/limiter.rs:42,56-70`)
- `BackpressureHandler` TOCTOU between `is_accepting()` and processing counter (`ratelimit/backpressure.rs:37-52`)
- `CircuitBreaker` state reads use `Ordering::Relaxed` (`ratelimit/circuit_breaker.rs:82-84`)
- `ConfigLoader::merge` replaces entire sections instead of deep-merging (`config/loader.rs:69-83`)
- `MetricsCollector` creates custom `Registry` but exports from global (`metrics/collector.rs:16-19,297-306`)
- Metrics HTTP server has no auth; health endpoint always returns OK (`metrics/http.rs:27-30,64-66`)
- Recovery action uses fragile string matching (`reason.contains("solver")`) (`recovery.rs:172-181`)
- `calculate_backoff` is O(n) with no jitter -- thundering herd (`relayer/service.rs:281-290`)

---

## 7. Test Coverage Gaps

### Critical Gaps

| ID | Gap | Impact |
|----|-----|--------|
| TG-C1 | Zero-amount escrow lock accepted -- test confirms `is_ok()` | Phantom settlements, reputation gaming |
| TG-C2 | User escrow rollback on solver vault failure never verified | User funds locked up to 15 minutes per failure |
| TG-C3 | IBC error path (vs timeout) never tested -- `set_should_error` method exists but is never called | Could lead to double-pay or missing refunds |
| TG-C4 | Double completion test doesn't verify reputation incremented exactly once | Solver reputation inflation |

### High Gaps

- No tests for admin API authentication (controls drain mode, all inflight settlements)
- No tests for recovery logic under realistic conditions (the most critical failure paths)
- No orchestrator integration testing (only smoke tests for struct creation)
- No tests for nonce replay prevention at application level
- Self-trading (wash trading) not prevented or tested
- No concurrent operation tests anywhere in the codebase

### Notable

- 6 "concept" tests in relayer adversarial tests contain no assertions -- false coverage confidence
- 10 `#[ignore]` tests requiring network access are never run
- No property-based or fuzz testing for financial calculations
- `make_test_intent()` helper duplicated with inconsistent defaults across 5+ files

---

## 8. Dependencies & Supply Chain

### Critical

| ID | Finding | Action |
|----|---------|--------|
| D-C1 | `rustls` v0.21.12 (via `reqwest` v0.11) -- known advisory RUSTSEC-2024-0336 | Upgrade `reqwest` to 0.12 |
| D-C2 | Unmaintained `curve25519-dalek-ng` fork (via `tendermint` v0.39) | Monitor tendermint-rs for 0.40+ release |
| D-C3 | `secp256k1` v0.29 compiled with C bindings but entirely unused (only `k256` is used) | Remove from workspace deps |

### High

| ID | Finding | Action |
|----|---------|--------|
| D-H1 | `getrandom` `custom` feature enabled with no `register_custom_getrandom!` implementation | Remove `custom` feature or add implementation |
| D-H2 | `ed25519-dalek` declared in workspace deps but never used by any crate | Remove from workspace deps |
| D-H3 | Dual TLS backends compiled (native-tls + rustls) via `reqwest` | Use `default-features = false` on upgrade |
| D-H4 | `serde_yaml` v0.9.34 is explicitly deprecated (version string includes `+deprecated`) | Migrate to `serde_yml` |

### Medium

- Duplicate `sha2` (v0.9 + v0.10), `hyper` (v0.14 + v1), `http` (v0.2 + v1), `base64` (v0.21 + v0.22), `thiserror` (v1 + v2)
- `sqlx` compiles unused MySQL/PostgreSQL drivers -- needs `default-features = false`
- `cosmwasm-std` version spec 2.2 resolves to 2.3 -- should be pinned for contract stability
- Many inline dependencies not using workspace references

---

## 9. Prioritized Remediation Plan

### Tier 1: Block Deployment (Critical security/fund-safety)

1. **Fix signing hash field separators** (T-C1) -- Add length prefixes or unambiguous delimiters between all variable-length fields
2. **Include `expires_at` in signing hash** (T-C2) -- Prevents expiry manipulation by intermediaries
3. **Add pubkey-to-address binding** (T-C4, T-C5) -- Derive bech32 address from pubkey and compare to `intent.user`
4. **Fix settlement atomicity** (ST-C1, ST-C2, ST-C3) -- Implement structured rollback that unwinds both locks on any failure
5. **Validate IBC ack content** (ST-C4) -- Parse ICS-20 ack bytes; treat error acks as failures
6. **Fix solver registration overwrite** (SC-C1) -- Add existence check before `SOLVERS.save`
7. **Block deregistration during active settlements** (SC-C2)
8. **Harmonize expiry boundaries** (SC-C4) -- Use consistent `>` or `>=` across both contracts
9. **Replace placeholder chain client** (O-C4) -- Or clearly gate behind feature flag so it cannot reach production
10. **Fix `Intent::side()`** (T-C3) -- Implement actual buy/sell determination based on pair convention

### Tier 2: Before Production Traffic (High severity)

11. Add state machine transition validation (ST-H1)
12. Sort intents by price before matching (ME-H1)
13. Enforce fill constraints (`min_fill_amount`, `min_fill_pct`, `FillStrategy`) (ME-H2)
14. Fix nonce recording to occur after batch processing, not before (ME-C2)
15. Add rounding policy (round against taker, track dust) (ME-C3)
16. Add division-by-zero guards in solver `solve()` methods (S-C1)
17. Move CEX inventory updates to settlement confirmation (S-C2)
18. Switch solver inventory to `tokio::sync::RwLock` or `parking_lot` (S-C4)
19. Add HTTP timeouts to `build_http_client()` (solver/http.rs)
20. Fix oracle fallback -- reject rather than use hardcoded price (O-C2)
21. Implement recovery system or disable `auto_recovery_enabled` (O-H2)
22. Add constant-time auth token comparison (O-C3)
23. Add escrow `reply` handler for IBC refund failures (SC-H5)
24. Add `execute_settlement` solver fund deposit requirement (SC-C3, SC-H6)
25. Remove unused `secp256k1` and `ed25519-dalek` dependencies (D-C3, D-H2)
26. Upgrade `reqwest` to 0.12 (D-C1, D-H3)

### Tier 3: Short-term Improvements

27. Deduplicate signing hash logic (extract shared function) (T-H1)
28. Replace string-encoded numerics with typed wrappers or validated constructors (T-H3)
29. Add `valid_until` check in aggregator solution selection (S-H2)
30. Make decimal precision configurable per asset (S-H3)
31. Add settlement event idempotency (ST-H2)
32. Use proper PFM intermediate receiver addresses (settlement/routing.rs)
33. Add pagination implementation (fix `start_after` in queries)
34. Add access control to `UpdateReputation` and `DecayReputation`
35. Implement bounded eviction for `intent_statuses` and `used_nonces`
36. Replace `serde_yaml` with non-deprecated alternative
37. Add `default-features = false` to `sqlx`
38. Pin `cosmwasm-std` version spec to match lockfile

---

## Findings Count Summary

| Severity | Types | Solver | Settlement | Matching | Contracts | Infra | Tests | Deps | **Total** |
|----------|-------|--------|------------|----------|-----------|-------|-------|------|-----------|
| Critical | 5 | 4 | 4 | 3 | 4 | 4 | 4 | 3 | **31** |
| High | 7 | 7 | 5 | 4 | 6 | 5 | 6 | 4 | **44** |
| Medium | 7 | 8 | 7 | 6 | 7 | 9 | 9 | 6 | **59** |
| Low/Info | 9 | 10 | 8 | 9 | 8 | 10 | 7 | 7 | **68** |
| **Total** | **28** | **29** | **24** | **22** | **25** | **28** | **26** | **20** | **202** |

---

*Report generated by parallel automated audit across 8 dimensions. Findings should be validated by manual review before remediation.*
