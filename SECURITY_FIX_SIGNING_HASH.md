# SECURITY FIX: Intent Signing Hash Vulnerability

## Summary

Fixed a critical security vulnerability in the Intent signing hash implementation that allowed signature bypass attacks by excluding security-critical fields from the hash computation.

## Vulnerability Details

### Original Issue

The `signing_hash()` method in both `Intent` and `UnsignedIntent` only included a subset of fields:
- ✓ version, nonce, user
- ✓ input (chain_id, denom, amount)
- ✓ output (chain_id, denom, min_amount, limit_price, recipient)
- ✓ constraints.deadline
- ✗ **MISSING: fill_config (all fields)**
- ✗ **MISSING: constraints.max_hops**
- ✗ **MISSING: constraints.excluded_venues**
- ✗ **MISSING: constraints.max_solver_fee_bps**
- ✗ **MISSING: constraints.allow_cross_ecosystem**
- ✗ **MISSING: constraints.max_bridge_time_secs**

### Attack Vectors

An attacker could modify any of the missing fields after a user signed the intent:

1. **Partial Fill Exploitation**: Change `allow_partial` from false to true, enabling incremental fund draining
2. **Route Manipulation**: Remove `max_hops` constraint to enable longer, more costly routes
3. **Venue Bypass**: Remove entries from `excluded_venues` to route through malicious DEXes
4. **Cross-Ecosystem Attack**: Enable `allow_cross_ecosystem` to route through less secure bridges
5. **Strategy Manipulation**: Change `FillStrategy` to enable unfavorable execution
6. **Fee Manipulation**: Modify `max_solver_fee_bps` to charge higher fees

## Fix Implementation

### Changes to `/Users/zakimanian/cosmos/atom-intents/crates/types/src/intent.rs`

Updated both `Intent::signing_hash()` and `UnsignedIntent::signing_hash()` to include ALL execution-critical fields:

#### Fill Configuration
```rust
// Fill configuration - ALL fields affect execution
hasher.update([self.fill_config.allow_partial as u8]);
hasher.update(self.fill_config.min_fill_amount.u128().to_le_bytes());
hasher.update(self.fill_config.min_fill_pct.as_bytes());
hasher.update(self.fill_config.aggregation_window_ms.to_le_bytes());

// Fill strategy (serialize as JSON for deterministic representation)
let strategy_json = serde_json::to_string(&self.fill_config.strategy)
    .unwrap_or_else(|_| "null".to_string());
hasher.update(strategy_json.as_bytes());
```

#### Execution Constraints
```rust
// max_hops (Option<u32>)
if let Some(max_hops) = self.constraints.max_hops {
    hasher.update([1u8]); // Some marker
    hasher.update(max_hops.to_le_bytes());
} else {
    hasher.update([0u8]); // None marker
}

// excluded_venues (sorted for determinism)
let mut excluded_venues = self.constraints.excluded_venues.clone();
excluded_venues.sort();
hasher.update((excluded_venues.len() as u32).to_le_bytes());
for venue in excluded_venues {
    hasher.update(venue.as_bytes());
}

// max_solver_fee_bps (Option<u32>)
if let Some(fee_bps) = self.constraints.max_solver_fee_bps {
    hasher.update([1u8]); // Some marker
    hasher.update(fee_bps.to_le_bytes());
} else {
    hasher.update([0u8]); // None marker
}

// allow_cross_ecosystem
hasher.update([self.constraints.allow_cross_ecosystem as u8]);

// max_bridge_time_secs (Option<u64>)
if let Some(bridge_time) = self.constraints.max_bridge_time_secs {
    hasher.update([1u8]); // Some marker
    hasher.update(bridge_time.to_le_bytes());
} else {
    hasher.update([0u8]); // None marker
}
```

### Key Design Decisions

1. **Deterministic Serialization**:
   - `excluded_venues` are sorted before hashing to ensure order doesn't affect the hash
   - `FillStrategy` uses JSON serialization for nested enum variants

2. **Option Handling**:
   - Uses marker bytes (0x00/0x01) to distinguish between Some/None
   - Prevents attacks that change Some(value) to None or vice versa

3. **Comprehensive Coverage**:
   - Every field that affects execution is now included
   - No field can be modified without invalidating the signature

## Test Coverage

Created comprehensive security test suite in `/Users/zakimanian/cosmos/atom-intents/crates/types/tests/signing_hash_security_tests.rs`:

### Test Categories

1. **Fill Config Security Tests** (6 tests)
   - `test_changing_allow_partial_changes_signature`
   - `test_changing_min_fill_amount_changes_signature`
   - `test_changing_min_fill_pct_changes_signature`
   - `test_changing_aggregation_window_changes_signature`
   - `test_changing_fill_strategy_changes_signature`
   - `test_changing_strategy_variant_params_changes_signature`

2. **Execution Constraints Security Tests** (8 tests)
   - `test_changing_max_hops_changes_signature`
   - `test_changing_max_hops_from_some_to_none_changes_signature`
   - `test_changing_excluded_venues_changes_signature`
   - `test_adding_excluded_venue_changes_signature`
   - `test_excluded_venues_order_is_normalized`
   - `test_changing_allow_cross_ecosystem_changes_signature`
   - `test_changing_max_solver_fee_bps_changes_signature`
   - `test_changing_max_bridge_time_secs_changes_signature`

3. **Attack Scenario Tests** (5 tests)
   - `test_attack_scenario_partial_fill_exploitation`
   - `test_attack_scenario_remove_max_hops_constraint`
   - `test_attack_scenario_remove_excluded_venues`
   - `test_attack_scenario_enable_cross_ecosystem`
   - `test_attack_scenario_change_fill_strategy`

4. **Regression Tests** (3 tests)
   - Verify original fields still protected after changes

### Test Results

All tests pass successfully:
- 8 unit tests (verification module)
- 15 signature verification tests
- **22 new security tests** ← NEW
- Total: 45 tests passing

```
test result: ok. 45 passed; 0 failed; 0 ignored
```

## Security Impact

### Before Fix
- ⚠️ CRITICAL: Attackers could modify 11+ security-critical fields without invalidating signatures
- ⚠️ HIGH: Users' funds at risk through partial fill exploitation
- ⚠️ HIGH: Users could be routed through malicious venues against their will
- ⚠️ MEDIUM: Cross-ecosystem execution could be enabled on Cosmos-only intents

### After Fix
- ✅ ALL fields affecting execution are now cryptographically protected
- ✅ Any modification to execution parameters invalidates the signature
- ✅ Comprehensive test coverage prevents regression
- ✅ Deterministic serialization ensures cross-platform compatibility

## Breaking Changes

**IMPORTANT**: This is a breaking change to the signing hash algorithm.

- All existing signed intents using the old algorithm are **INVALID**
- Users must re-sign all pending intents with the new algorithm
- The change is necessary to prevent critical security vulnerabilities

## Recommendations

1. **Immediate Deployment**: This fix should be deployed immediately
2. **Intent Migration**: Invalidate all existing signed intents and require re-signing
3. **Security Audit**: Consider full security audit of signature verification flow
4. **Documentation**: Update all documentation about signing requirements
5. **Client Updates**: All client libraries must be updated to use the new signing algorithm

## Files Modified

1. `/Users/zakimanian/cosmos/atom-intents/crates/types/src/intent.rs`
   - Updated `Intent::signing_hash()` (lines 103-180)
   - Updated `UnsignedIntent::signing_hash()` (lines 292-366)

2. `/Users/zakimanian/cosmos/atom-intents/crates/types/tests/signing_hash_security_tests.rs` (NEW)
   - 554 lines of comprehensive security tests
   - 22 test cases covering all attack vectors

## Verification

To verify the fix:
```bash
cd /Users/zakimanian/cosmos/atom-intents
cargo test -p atom-intents-types
```

Expected output:
```
test result: ok. 45 passed; 0 failed; 0 ignored
```

## Credits

Security vulnerability identified and fixed as part of security audit.
