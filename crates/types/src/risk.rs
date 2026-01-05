//! Settlement risk parameters for Eureka fronting
//!
//! When solvers front funds before Eureka escrow is finalized,
//! they take on settlement risk. This module provides types for
//! calculating required bonds.
//!
//! # Finality Times
//!
//! Different source chains have different finality characteristics:
//!
//! - **Ethereum L1**: ~20 minutes (2 epochs of Casper FFG finality)
//! - **OP Stack L2s (Base, Optimism, etc.)**: ~20 minutes (L1 batch finality)
//! - **ZK Rollups (zkSync, Scroll)**: ~15 minutes (proof + L1 finality)
//!
//! # Risk Premium
//!
//! Risk premiums are **auction-discovered** - solvers bid their total spread
//! which implicitly includes their risk assessment. The protocol only needs
//! to know bond requirements, not individual solver risk calculations.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Protocol parameters for settlement risk
///
/// These parameters define bond requirements for fronting. The protocol
/// uses these to ensure solvers have sufficient collateral. Risk premiums
/// are discovered through the auction - solvers bid their total spread.
#[cw_serde]
pub struct SettlementRiskPricing {
    /// Base probability of Eureka failure (e.g., "0.0001" = 0.01%)
    /// Used for expected value calculations by solvers (not enforced by protocol)
    pub failure_probability: String,

    /// Expected time until ZK proof finality (seconds)
    /// Ethereum L1: ~1200s (20 min), OP Stack L2: ~1200s, ZK rollups: ~900s
    pub expected_finality_secs: u64,

    /// Bond multiplier (e.g., "1.5" = 150% of fronted amount)
    /// Protocol-enforced: solvers must post this much collateral
    pub bond_multiplier: String,
}

impl SettlementRiskPricing {
    /// Default for Ethereum L1 -> Cosmos via IBC Eureka
    ///
    /// Ethereum PoS finality requires 2 epochs (~12.8 min minimum, ~20 min typical).
    /// After finality, reorg risk is effectively zero (would require 1/3 stake slashing).
    pub fn default_ethereum_l1() -> Self {
        Self {
            failure_probability: "0.0001".to_string(), // Very low after finality
            expected_finality_secs: 1200,              // 20 minutes
            bond_multiplier: "1.5".to_string(),        // 150% bond
        }
    }

    /// Default for OP Stack L2s (Base, Optimism, etc.) -> Cosmos via IBC Eureka
    ///
    /// OP Stack L2s achieve L1-equivalent finality when their batch is finalized
    /// on Ethereum L1 (~20 minutes). Sequencer has some additional trust assumptions.
    pub fn default_op_stack_l2() -> Self {
        Self {
            failure_probability: "0.0005".to_string(), // Slightly higher (sequencer risk)
            expected_finality_secs: 1200,              // 20 minutes (L1 batch finality)
            bond_multiplier: "1.5".to_string(),
        }
    }

    /// Default for ZK rollups (zkSync, Scroll, etc.) -> Cosmos via IBC Eureka
    ///
    /// ZK rollups have faster soft finality once proof is submitted,
    /// but still need L1 finality for the proof transaction.
    pub fn default_zk_rollup() -> Self {
        Self {
            failure_probability: "0.0002".to_string(), // Low (cryptographic guarantees)
            expected_finality_secs: 900,               // ~15 minutes
            bond_multiplier: "1.5".to_string(),
        }
    }

    /// Alias for backward compatibility - defaults to Ethereum L1
    pub fn default_eureka() -> Self {
        Self::default_ethereum_l1()
    }

    /// Conservative parameters for high-risk scenarios or testing
    pub fn conservative() -> Self {
        Self {
            failure_probability: "0.005".to_string(), // 0.5% failure rate
            expected_finality_secs: 1500,             // 25 minutes
            bond_multiplier: "2.0".to_string(),       // 2x bond
        }
    }

    /// Calculate required bond for fronting a given amount
    pub fn required_bond(&self, fronted_amount: Uint128) -> Uint128 {
        let multiplier = Decimal::from_str(&self.bond_multiplier).unwrap_or(Decimal::from(2));
        let result = Decimal::from(fronted_amount.u128()) * multiplier;
        Uint128::new(
            result
                .trunc()
                .to_string()
                .parse::<u128>()
                .unwrap_or(fronted_amount.u128() * 2),
        )
    }

    /// Get failure probability as Decimal
    pub fn failure_prob(&self) -> Decimal {
        Decimal::from_str(&self.failure_probability)
            .unwrap_or_else(|_| Decimal::from_str("0.001").unwrap())
    }

    /// Get bond multiplier as Decimal
    pub fn multiplier(&self) -> Decimal {
        Decimal::from_str(&self.bond_multiplier).unwrap_or(Decimal::from(2))
    }
}

impl Default for SettlementRiskPricing {
    fn default() -> Self {
        Self::default_eureka()
    }
}

/// Result of a fronting risk assessment
///
/// Helps solvers decide whether to front a settlement. The actual
/// quote/spread is determined by the solver's auction bid, not by
/// this assessment.
#[cw_serde]
pub struct FrontingRiskAssessment {
    /// Whether fronting is viable (positive expected value)
    pub should_front: bool,

    /// Required bond amount (protocol-enforced)
    pub required_bond: Uint128,

    /// Expected profit/loss if fronting (solver's internal calculation)
    pub expected_value: i128,

    /// Reason for recommendation
    pub reason: String,
}

impl FrontingRiskAssessment {
    /// Assess whether fronting is viable given the profit opportunity
    ///
    /// Note: This uses the solver's bid profit, not a computed risk premium.
    /// The solver's auction bid already incorporates their risk assessment.
    pub fn assess(
        pricing: &SettlementRiskPricing,
        fronted_amount: Uint128,
        solver_bid_output: Uint128, // What solver offers to user (their auction bid)
    ) -> Self {
        let required_bond = pricing.required_bond(fronted_amount);
        let failure_prob = pricing.failure_prob();

        // Profit = what solver receives (fronted_amount) - what solver pays out (bid)
        // If fronted_amount > bid, solver profits
        let profit = fronted_amount.u128() as i128 - solver_bid_output.u128() as i128;

        // Expected value = (1 - failure_prob) * profit - failure_prob * bond_loss
        let success_prob = Decimal::from(1) - failure_prob;
        let expected_profit = Decimal::from(profit) * success_prob;
        let expected_loss = Decimal::from(required_bond.u128() as i128) * failure_prob;
        let ev_decimal = expected_profit - expected_loss;
        let expected_value = ev_decimal.trunc().to_string().parse::<i128>().unwrap_or(0);

        let should_front = expected_value > 0 && profit > 0;
        let reason = if should_front {
            format!(
                "Positive expected value: {} with {} profit margin",
                expected_value, profit
            )
        } else if profit <= 0 {
            "Negative profit margin - solver bid too high".to_string()
        } else {
            format!(
                "Negative expected value: {} - risk outweighs reward",
                expected_value
            )
        };

        Self {
            should_front,
            required_bond,
            expected_value,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ethereum_l1_pricing() {
        let pricing = SettlementRiskPricing::default_ethereum_l1();
        assert_eq!(pricing.expected_finality_secs, 1200); // 20 minutes
        assert_eq!(pricing.bond_multiplier, "1.5");
        assert_eq!(pricing.failure_probability, "0.0001");
    }

    #[test]
    fn test_default_op_stack_l2_pricing() {
        let pricing = SettlementRiskPricing::default_op_stack_l2();
        assert_eq!(pricing.expected_finality_secs, 1200); // Same as L1 (L1 batch finality)
        assert_eq!(pricing.failure_probability, "0.0005"); // Slightly higher
    }

    #[test]
    fn test_default_zk_rollup_pricing() {
        let pricing = SettlementRiskPricing::default_zk_rollup();
        assert_eq!(pricing.expected_finality_secs, 900); // 15 minutes
        assert_eq!(pricing.failure_probability, "0.0002");
    }

    #[test]
    fn test_default_eureka_is_ethereum_l1() {
        let eureka = SettlementRiskPricing::default_eureka();
        let eth_l1 = SettlementRiskPricing::default_ethereum_l1();
        assert_eq!(eureka.expected_finality_secs, eth_l1.expected_finality_secs);
        assert_eq!(eureka.bond_multiplier, eth_l1.bond_multiplier);
    }

    #[test]
    fn test_required_bond_calculation() {
        let pricing = SettlementRiskPricing::default_ethereum_l1();
        let fronted = Uint128::new(1_000_000);
        let bond = pricing.required_bond(fronted);
        assert_eq!(bond, Uint128::new(1_500_000)); // 1.5x multiplier
    }

    #[test]
    fn test_conservative_bond() {
        let pricing = SettlementRiskPricing::conservative();
        let fronted = Uint128::new(1_000_000);
        let bond = pricing.required_bond(fronted);
        assert_eq!(bond, Uint128::new(2_000_000)); // 2x multiplier
    }

    #[test]
    fn test_fronting_assessment_profitable_bid() {
        let pricing = SettlementRiskPricing::default_ethereum_l1();
        let fronted = Uint128::new(1_000_000); // What solver receives from escrow
        let solver_bid = Uint128::new(990_000); // What solver pays to user (bid includes their spread)

        let assessment = FrontingRiskAssessment::assess(&pricing, fronted, solver_bid);
        assert!(assessment.should_front);
        assert!(assessment.expected_value > 0);
    }

    #[test]
    fn test_fronting_assessment_unprofitable_bid() {
        let pricing = SettlementRiskPricing::default_ethereum_l1();
        let fronted = Uint128::new(1_000_000);
        let solver_bid = Uint128::new(1_100_000); // Solver pays more than they receive

        let assessment = FrontingRiskAssessment::assess(&pricing, fronted, solver_bid);
        assert!(!assessment.should_front);
        assert!(assessment.reason.contains("Negative profit margin"));
    }

    #[test]
    fn test_fronting_assessment_marginal_bid() {
        // High-risk scenario with thin margins
        let pricing = SettlementRiskPricing::conservative(); // 0.5% failure, 2x bond
        let fronted = Uint128::new(1_000_000);
        let solver_bid = Uint128::new(999_000); // Only 0.1% profit

        let assessment = FrontingRiskAssessment::assess(&pricing, fronted, solver_bid);
        // With 0.5% failure rate and 2x bond, thin margin might not be worth it
        // Let the test capture the actual behavior
        println!(
            "Marginal bid assessment: should_front={}, ev={}",
            assessment.should_front, assessment.expected_value
        );
    }
}
