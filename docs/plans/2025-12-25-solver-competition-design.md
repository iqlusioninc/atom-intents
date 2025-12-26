# Solver Competition Advantages Design

## Overview

Make solver competition in the demo more realistic by giving solvers edges based on order characteristics (size, token pair, chains). Advantages influence pricing but don't guarantee wins, creating competitive auctions where patterns emerge over time.

## Design Decisions

- **Advantage types**: Order size, token pair specialization, cross-chain vs same-chain
- **Win rate for advantaged solver**: 55-65% (competitive, not deterministic)
- **UI feedback**: Subtle tags on winners ("Native pair", "Size specialist")
- **Order size thresholds**: Small <$500, Medium $500-5K, Large >$5K

## Solver Advantage Profiles

| Solver | Pair Advantage | Size Advantage | Chain Advantage |
|--------|----------------|----------------|-----------------|
| Osmosis DEX Router | ATOM/OSMO, OSMO/* | Small, Medium | Same-chain Osmosis |
| Intent Matcher | Any (when counter-order exists) | Medium | None |
| CEX Backstop | Stablecoin pairs (USDC, USDT) | Large | None (chain-agnostic) |
| Astroport Router | NTRN pairs, ATOM/NTRN | Small, Medium | Neutron-based |
| Celestia Bridge | TIA/* pairs | Medium | Celestia swaps |

## Advantage Score Calculation

Each order scores against a solver's profile across three dimensions:

| Dimension | Score Range | Notes |
|-----------|-------------|-------|
| Pair match | 0.0 - 0.4 | Full score for native pairs, partial for related |
| Size match | 0.0 - 0.3 | Based on order USD value vs solver specialty |
| Chain match | 0.0 - 0.3 | Cross-chain orders favor specialists |

**Total advantage score** = pair + size + chain (max 1.0)

## Spread Calculation

Current base spreads per solver type:
- Intent Matcher: 0.0%
- DEX Router: 0.1-0.5%
- CEX Backstop: 0.3-0.8%
- Hybrid: 0.15-0.4%

New formula:
```
spread_range = base_spread_max - base_spread_min
effective_spread = base_spread_max - (advantage_score * spread_range) + noise
noise = random(-0.05%, +0.05%)
```

High advantage score → spread near minimum → better price → more likely to win.

## Winner Tags

| Tag | Condition |
|-----|-----------|
| "Native pair" | Pair score was highest contributor |
| "Size specialist" | Size score was highest contributor |
| "Cross-chain expert" | Chain score was highest (cross-chain order) |
| "Direct match" | Intent Matcher won via counter-order |
| "Best execution" | No clear advantage — won on general execution |

## Intent Matcher Special Logic

Intent Matcher only competes when a matching counter-order exists:

1. Two intents match when: A wants X→Y, B wants Y→X, amounts within 20%
2. If match found: quote with 0% spread (P2P, best price)
3. If no match: Intent Matcher doesn't submit a quote

**Synthetic counter-orders**: 15% chance of generating a matching counter-intent when an order is submitted, ensuring Intent Matcher participates occasionally.

## Data Model Changes

### SolverQuote (models.rs)

Add field:
```rust
pub advantage_reason: Option<String>,
```

### Solver (state.rs)

Add advantage profile:
```rust
pub struct SolverAdvantageProfile {
    pub preferred_pairs: Vec<(String, String)>,  // (input, output) pairs
    pub size_preference: SizePreference,          // Small, Medium, Large, Any
    pub chain_specialty: Vec<String>,             // chain_ids
}
```

## Implementation Files

1. `demo/skip-select-simulator/src/models.rs` — Add advantage_reason to SolverQuote
2. `demo/skip-select-simulator/src/state.rs` — Add advantage profiles to solvers
3. `demo/skip-select-simulator/src/auction.rs` — Implement scoring and spread logic
4. `demo/skip-select-simulator/src/solver.rs` — Intent Matcher counter-order logic
5. `demo/web-ui/src/components/SolverDashboard.tsx` — Display advantage tags
6. `demo/web-ui/src/components/AuctionView.tsx` — Show tag on winning quote
7. `demo/web-ui/src/types/index.ts` — Add advantage_reason to types
