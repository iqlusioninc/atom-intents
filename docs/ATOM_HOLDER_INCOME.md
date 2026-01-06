# ATOM Intents: A New Income Stream for ATOM Holders

## Executive Summary

ATOM Intents creates a novel income opportunity for financially sophisticated ATOM holders by enabling them to run **solvers**—entities that compete to fulfill cross-chain trading intents. Unlike traditional market making which requires liquid capital sitting idle, ATOM Intents allows holders to use their **staked ATOM** (via Liquid Staking Tokens) as solver bond collateral while continuing to earn staking rewards.

This means ATOM holders can now earn **stacking yields**: staking rewards + solver income + relayer revenue, all from the same underlying ATOM position.

---

## The Opportunity

### What Solvers Do

Solvers are the economic engines of ATOM Intents. When a user wants to swap tokens across chains, solvers compete in an auction to provide the best execution. The winning solver:

1. Commits to delivering the user's desired output
2. Locks collateral (bond) to guarantee performance
3. Executes the trade and earns fees

### Why ATOM and ATOM LSTs Are Required

Solver bonds must be denominated in ATOM ecosystem assets:

| Collateral Type | Haircut | Effective Coverage | Benefit |
|-----------------|---------|-------------------|---------|
| **Native ATOM** | 0% | 1.5x fill value | Maximum capital efficiency |
| **LSM Shares** (tokenized delegations) | 10% | 1.35x fill value | Earn staking rewards while bonded |
| **Hydro Vault Shares** | 20% | 1.2x fill value | Earn vault yield while bonded |

**The key insight**: With LSM shares or Hydro vault shares as collateral, your ATOM continues earning staking/vault yield even while locked as solver bond. You're not choosing between staking and solving—you're doing both.

---

## Income Streams for Solver Operators

### 1. Surplus Capture (DEX Routing)

When routing trades through DEXs like Osmosis or Astroport, solvers capture a portion of price improvement:

```
User's limit price:     10.40 ATOM per USDC
Best route achieves:    10.45 ATOM per USDC
Surplus:                0.05 ATOM (0.48% improvement)

Solver keeps:           10% of surplus = 0.005 ATOM
User receives:          90% of surplus = 0.045 ATOM better than limit
```

**Capital required: Zero.** DEX routing solvers don't need inventory—they route through existing liquidity pools.

### 2. Spread Income (CEX Backstop)

For larger trades or illiquid pairs, solvers can operate as market makers using CEX liquidity:

```
Trade size:             100,000 USDC
CEX mid-market:         10.45 ATOM/USDC
Solver quote:           10.40 ATOM/USDC (0.5% spread)

Gross revenue:          $500
Less CEX fees:          -$10
Less hedging slippage:  -$5
Less capital cost:      -$7/day

Net per trade:          ~$280
```

**Capital required: ~$50,000** buffer for hedging and inventory management.

### 3. Relayer Revenue

Solvers who run their own IBC relayers earn additional income:

| Service Level | Fee per Packet |
|--------------|----------------|
| Base relay | $0.10 |
| Fast relay (<2 blocks) | $0.50 |
| Guaranteed SLA | $1.00 |

Running a relayer also provides strategic advantages:
- **Win more auctions** through faster execution
- **Reduce IBC exposure** with shorter settlement windows
- **Avoid timeouts** that would trigger slashing

---

## Economic Model: Stacking Yields

Here's how a sophisticated ATOM holder can layer multiple income streams:

### Base Case: 10,000 ATOM Position

**Traditional Approach (Staking Only)**:
```
Staking APR:            ~15%
Annual yield:           1,500 ATOM
Monthly income:         125 ATOM
```

**ATOM Intents Approach (Staking + Solving)**:
```
Step 1: Convert ATOM to LSM shares (tokenized delegation)
        - Still earning 15% staking APR
        - Now liquid and usable as collateral

Step 2: Deposit LSM shares as solver bond
        - 10% haircut → 9,000 ATOM effective collateral
        - Can support ~6,000 ATOM of concurrent settlements (1.5x requirement)

Step 3: Run DEX routing solver (zero additional capital)
        - Process 50 trades/day averaging 1,000 ATOM each
        - Surplus capture: ~5 bps per trade = 0.5 ATOM/trade
        - Daily revenue: 25 ATOM
        - Monthly revenue: 750 ATOM

Step 4: Run relayer for competitive advantage
        - Monthly relay revenue: ~50 ATOM
        - Faster fills → more auction wins → higher volume

Combined Monthly Income:
  Staking rewards:      125 ATOM
  Solver surplus:       750 ATOM
  Relayer fees:          50 ATOM
  ─────────────────────────────
  Total:                925 ATOM/month

Effective APR:          111% (vs 15% staking only)
```

### Capital Efficiency Comparison

| Strategy | Capital Locked | Monthly Income | Capital Efficiency |
|----------|---------------|----------------|-------------------|
| Staking only | 10,000 ATOM | 125 ATOM | 1.25%/month |
| Traditional MM | 10,000 ATOM (liquid) | 200-400 ATOM | 2-4%/month |
| ATOM Intents Solver | 10,000 ATOM (staked) | 750-925 ATOM | 7.5-9.25%/month |

The key advantage: **Your ATOM stays staked** earning base yield while simultaneously working as solver collateral.

---

## Risk-Reward Profile

### Risks

1. **Slashing Risk**: Failed settlements result in 2% slashing (minimum 10 ATOM)
   - Mitigated by: Only committing to trades you can execute
   - Mitigated by: Running your own relayer for reliability

2. **IBC Timeout Risk**: Asymmetric failures where you deliver but user's transfer times out
   - Mitigated by: 1.5x overcollateralization protects your capital
   - Mitigated by: Bond returned when IBC succeeds in either direction

3. **Liquidation Risk** (for LST collateral): If slashed, your LSM shares are auctioned
   - Mitigated by: Auction to other solvers gets fair market price
   - Mitigated by: Excess returned to you after compensation

4. **Operational Risk**: Infrastructure failures, bugs, downtime
   - Mitigated by: Start with DEX routing (simpler) before CEX backstop
   - Mitigated by: Monitor systems, set conservative limits

### Risk Mitigation Through Design

The system is designed so that **honest, competent solvers cannot lose money**:

- Bonds only lock when you **commit** to a settlement (after winning auction)
- You control which trades to bid on
- 1.5x overcollateralization protects against IBC failures
- Slashing only occurs for actual failures to deliver

---

## Getting Started

### Minimum Requirements

| Solver Type | ATOM Required | Technical Skill | Expected Monthly Return |
|-------------|---------------|-----------------|------------------------|
| DEX Router (basic) | 1,000 ATOM bond | Medium | 50-100 ATOM |
| DEX Router (active) | 5,000 ATOM bond | Medium-High | 300-500 ATOM |
| Full Stack (DEX + Relayer) | 10,000 ATOM bond | High | 750-1,000 ATOM |
| CEX Backstop | 10,000 ATOM bond + $50k liquid | Very High | 1,000-2,000 ATOM |

### Technical Stack

1. **Infrastructure**: Cloud VM or dedicated server
2. **Software**: ATOM Intents solver node (open source)
3. **Connectivity**: RPC access to Cosmos Hub, Osmosis, Neutron
4. **Optional**: Hermes relayer for competitive advantage

### Path to Profitability

```
Month 1-2: Learn the system
  - Run solver in testnet
  - Understand auction dynamics
  - Monitor successful solvers

Month 3: Go live conservatively
  - Small bond (1,000 ATOM)
  - DEX routing only
  - Manual oversight

Month 4-6: Scale up
  - Increase bond as confidence grows
  - Add relayer for speed advantage
  - Expand to more trading pairs

Month 6+: Optimize
  - Fine-tune pricing algorithms
  - Consider CEX backstop integration
  - Build reputation for larger fills
```

---

## Why This Matters for ATOM

### Network Effects

As more ATOM holders become solvers:

1. **More competition** → Better prices for users → More volume
2. **More volume** → More solver revenue → More ATOM bonded
3. **More ATOM bonded** → Higher ATOM utility → Stronger tokenomics

### ATOM as Productive Collateral

ATOM Intents transforms ATOM from a "stake and forget" asset into **productive collateral**:

- Staking secures the network
- LSM shares enable liquidity without unbonding
- Solver bonds enable cross-chain commerce
- All from the same underlying ATOM

This creates genuine, sustainable demand for ATOM beyond speculation.

---

## Conclusion

ATOM Intents offers financially sophisticated ATOM holders a new way to earn yield:

1. **Keep staking rewards** via LSM shares as collateral
2. **Add solver income** from competitive order execution
3. **Earn relayer fees** by running infrastructure
4. **Compound returns** as volume and reputation grow

For holders with technical capability and risk tolerance, running an ATOM Intents solver represents one of the highest-yielding opportunities in the Cosmos ecosystem—while keeping your ATOM productively staked and securing the network.

The future of ATOM is not just staking. It's **staking + solving + relaying**. Stack your yields.
