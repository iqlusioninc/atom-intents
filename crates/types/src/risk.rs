//! Settlement risk pricing for Eureka fronting
//!
//! When solvers front funds before Eureka escrow is finalized,
//! they take on settlement risk. This module provides types for
//! calculating required bonds and risk-adjusted quotes.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Configuration for settlement risk pricing
#[cw_serde]
pub struct SettlementRiskPricing {
    /// Base probability of Eureka failure (e.g., 0.001 = 0.1%)
    pub failure_probability: String, // Store as string for cw_serde compatibility

    /// Expected time until ZK proof finality (seconds)
    pub expected_finality_secs: u64,

    /// Bond multiplier (e.g., "2.0" = 200% of fronted amount)
    pub bond_multiplier: String,

    /// Risk premium in basis points (e.g., 50 = 0.5%)
    pub risk_premium_bps: u32,
}

impl SettlementRiskPricing {
    /// Create default pricing for Eureka settlements
    pub fn default_eureka() -> Self {
        Self {
            failure_probability: "0.001".to_string(), // 0.1% failure rate
            expected_finality_secs: 30,
            bond_multiplier: "2.0".to_string(), // 2x bond
            risk_premium_bps: 50,               // 0.5% premium
        }
    }

    /// Create conservative pricing for higher risk tolerance
    pub fn conservative() -> Self {
        Self {
            failure_probability: "0.005".to_string(), // 0.5% failure rate
            expected_finality_secs: 60,
            bond_multiplier: "3.0".to_string(), // 3x bond
            risk_premium_bps: 100,              // 1% premium
        }
    }

    /// Calculate required bond for fronting a given amount
    pub fn required_bond(&self, fronted_amount: Uint128) -> Uint128 {
        let multiplier = Decimal::from_str(&self.bond_multiplier).unwrap_or(Decimal::from(2));
        let result = Decimal::from(fronted_amount.u128()) * multiplier;
        // Truncate to integer before parsing
        Uint128::new(
            result
                .trunc()
                .to_string()
                .parse::<u128>()
                .unwrap_or(fronted_amount.u128() * 2),
        )
    }

    /// Calculate risk-adjusted output amount
    pub fn adjust_quote(&self, base_output: Uint128) -> Uint128 {
        let premium = base_output.u128() * self.risk_premium_bps as u128 / 10000;
        Uint128::new(base_output.u128().saturating_sub(premium))
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
#[cw_serde]
pub struct FrontingRiskAssessment {
    /// Whether fronting is recommended given the risk
    pub should_front: bool,

    /// Required bond amount
    pub required_bond: Uint128,

    /// Risk-adjusted output (after premium deduction)
    pub adjusted_output: Uint128,

    /// Expected profit/loss if fronting
    pub expected_value: i128,

    /// Reason for recommendation
    pub reason: String,
}

impl FrontingRiskAssessment {
    /// Assess whether to front a settlement
    pub fn assess(
        pricing: &SettlementRiskPricing,
        fronted_amount: Uint128,
        expected_output: Uint128,
    ) -> Self {
        let required_bond = pricing.required_bond(fronted_amount);
        let adjusted_output = pricing.adjust_quote(expected_output);
        let failure_prob = pricing.failure_prob();

        // Expected value = (1 - failure_prob) * profit - failure_prob * bond
        let profit = adjusted_output.u128() as i128 - fronted_amount.u128() as i128;
        let success_prob = Decimal::from(1) - failure_prob;
        let expected_profit = Decimal::from(profit) * success_prob;
        let expected_loss = Decimal::from(required_bond.u128() as i128) * failure_prob;
        let ev_decimal = expected_profit - expected_loss;
        // Truncate to integer for i128 storage
        let expected_value = ev_decimal.trunc().to_string().parse::<i128>().unwrap_or(0);

        let should_front = expected_value > 0 && profit > 0;
        let reason = if should_front {
            format!(
                "Positive expected value: {} with {} profit margin",
                expected_value, profit
            )
        } else if profit <= 0 {
            "Negative profit margin before risk adjustment".to_string()
        } else {
            format!(
                "Negative expected value: {} - risk outweighs reward",
                expected_value
            )
        };

        Self {
            should_front,
            required_bond,
            adjusted_output,
            expected_value,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_eureka_pricing() {
        let pricing = SettlementRiskPricing::default_eureka();
        assert_eq!(pricing.expected_finality_secs, 30);
        assert_eq!(pricing.risk_premium_bps, 50);
        assert_eq!(pricing.bond_multiplier, "2.0");
    }

    #[test]
    fn test_required_bond_calculation() {
        let pricing = SettlementRiskPricing::default_eureka();
        let fronted = Uint128::new(1_000_000);
        let bond = pricing.required_bond(fronted);
        assert_eq!(bond, Uint128::new(2_000_000)); // 2x multiplier
    }

    #[test]
    fn test_conservative_bond() {
        let pricing = SettlementRiskPricing::conservative();
        let fronted = Uint128::new(1_000_000);
        let bond = pricing.required_bond(fronted);
        assert_eq!(bond, Uint128::new(3_000_000)); // 3x multiplier
    }

    #[test]
    fn test_risk_adjusted_quote() {
        let pricing = SettlementRiskPricing {
            failure_probability: "0.001".to_string(),
            expected_finality_secs: 30,
            bond_multiplier: "2.0".to_string(),
            risk_premium_bps: 100, // 1%
        };

        let base_output = Uint128::new(1_000_000);
        let adjusted = pricing.adjust_quote(base_output);
        assert_eq!(adjusted, Uint128::new(990_000)); // 1% deducted
    }

    #[test]
    fn test_fronting_assessment_positive() {
        let pricing = SettlementRiskPricing::default_eureka();
        let fronted = Uint128::new(1_000_000);
        let expected = Uint128::new(1_100_000); // 10% profit

        let assessment = FrontingRiskAssessment::assess(&pricing, fronted, expected);
        assert!(assessment.should_front);
        assert!(assessment.expected_value > 0);
    }

    #[test]
    fn test_fronting_assessment_negative_margin() {
        let pricing = SettlementRiskPricing::default_eureka();
        let fronted = Uint128::new(1_000_000);
        let expected = Uint128::new(900_000); // Negative margin

        let assessment = FrontingRiskAssessment::assess(&pricing, fronted, expected);
        assert!(!assessment.should_front);
        assert!(assessment.reason.contains("Negative profit margin"));
    }
}
