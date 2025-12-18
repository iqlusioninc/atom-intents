use cosmwasm_std::Uint128;
use serde::{Deserialize, Serialize};

use crate::ReputationError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverReputation {
    pub solver_id: String,
    pub total_settlements: u64,
    pub successful_settlements: u64,
    pub failed_settlements: u64,
    pub total_volume: Uint128,
    pub average_settlement_time: u64,
    pub slashing_events: u64,
    pub reputation_score: u64,
    pub fee_tier: String,
    pub last_updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopSolversResponse {
    pub solvers: Vec<SolverReputation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolversByReputationResponse {
    pub solvers: Vec<SolverReputation>,
}

/// Client for querying solver reputation from the settlement contract
pub struct ReputationClient {
    pub contract_address: String,
    pub rpc_endpoint: String,
}

impl ReputationClient {
    /// Create a new ReputationClient
    pub fn new(contract_address: String, rpc_endpoint: String) -> Self {
        Self {
            contract_address,
            rpc_endpoint,
        }
    }

    /// Get reputation for a specific solver
    pub async fn get_reputation(&self, solver_id: &str) -> Result<SolverReputation, ReputationError> {
        let query = format!(
            r#"{{"solver_reputation":{{"solver_id":"{}"}}}}"#,
            solver_id
        );

        let url = format!(
            "{}/cosmwasm/wasm/v1/contract/{}/smart/{}",
            self.rpc_endpoint,
            self.contract_address,
            base64::encode(query)
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ReputationError::Network(format!(
                "Failed to query reputation: {}",
                response.status()
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        let reputation: SolverReputation = serde_json::from_value(
            result
                .get("data")
                .ok_or_else(|| ReputationError::Network("Missing data field".to_string()))?
                .clone(),
        )
        .map_err(|e| ReputationError::Network(e.to_string()))?;

        Ok(reputation)
    }

    /// Get top solvers by reputation score
    pub async fn get_top_solvers(&self, limit: u32) -> Result<Vec<SolverReputation>, ReputationError> {
        let query = format!(r#"{{"top_solvers":{{"limit":{}}}}}"#, limit);

        let url = format!(
            "{}/cosmwasm/wasm/v1/contract/{}/smart/{}",
            self.rpc_endpoint,
            self.contract_address,
            base64::encode(query)
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ReputationError::Network(format!(
                "Failed to query top solvers: {}",
                response.status()
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        let top_solvers: TopSolversResponse = serde_json::from_value(
            result
                .get("data")
                .ok_or_else(|| ReputationError::Network("Missing data field".to_string()))?
                .clone(),
        )
        .map_err(|e| ReputationError::Network(e.to_string()))?;

        Ok(top_solvers.solvers)
    }

    /// Get solvers by minimum reputation score
    pub async fn get_solvers_by_reputation(
        &self,
        min_score: u64,
        limit: u32,
    ) -> Result<Vec<SolverReputation>, ReputationError> {
        let query = format!(
            r#"{{"solvers_by_reputation":{{"min_score":{},"limit":{}}}}}"#,
            min_score, limit
        );

        let url = format!(
            "{}/cosmwasm/wasm/v1/contract/{}/smart/{}",
            self.rpc_endpoint,
            self.contract_address,
            base64::encode(query)
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ReputationError::Network(format!(
                "Failed to query solvers by reputation: {}",
                response.status()
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ReputationError::Network(e.to_string()))?;

        let solvers_response: SolversByReputationResponse = serde_json::from_value(
            result
                .get("data")
                .ok_or_else(|| ReputationError::Network("Missing data field".to_string()))?
                .clone(),
        )
        .map_err(|e| ReputationError::Network(e.to_string()))?;

        Ok(solvers_response.solvers)
    }

    /// Check if a solver meets minimum reputation requirements
    pub async fn should_use_solver(&self, solver_id: &str, min_score: u64) -> bool {
        match self.get_reputation(solver_id).await {
            Ok(reputation) => reputation.reputation_score >= min_score,
            Err(_) => false, // If we can't get reputation, don't use the solver
        }
    }

    /// Get solver's success rate as a percentage
    pub async fn get_success_rate(&self, solver_id: &str) -> Result<f64, ReputationError> {
        let reputation = self.get_reputation(solver_id).await?;

        if reputation.total_settlements == 0 {
            return Ok(0.0);
        }

        let success_rate =
            (reputation.successful_settlements as f64 / reputation.total_settlements as f64) * 100.0;
        Ok(success_rate)
    }

    /// Get recommended solvers for a given intent
    /// Returns solvers sorted by reputation score that meet minimum requirements
    pub async fn get_recommended_solvers(
        &self,
        min_score: u64,
        min_success_rate: f64,
        limit: u32,
    ) -> Result<Vec<SolverReputation>, ReputationError> {
        let solvers = self.get_solvers_by_reputation(min_score, limit * 2).await?;

        // Filter by success rate
        let filtered: Vec<SolverReputation> = solvers
            .into_iter()
            .filter(|s| {
                if s.total_settlements == 0 {
                    return false;
                }
                let success_rate =
                    (s.successful_settlements as f64 / s.total_settlements as f64) * 100.0;
                success_rate >= min_success_rate
            })
            .take(limit as usize)
            .collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reputation_client_creation() {
        let client = ReputationClient::new(
            "cosmos1contract".to_string(),
            "https://rpc.cosmos.network".to_string(),
        );
        assert_eq!(client.contract_address, "cosmos1contract");
        assert_eq!(client.rpc_endpoint, "https://rpc.cosmos.network");
    }

    #[test]
    fn test_success_rate_calculation() {
        let mut rep = SolverReputation {
            solver_id: "test".to_string(),
            total_settlements: 100,
            successful_settlements: 90,
            failed_settlements: 10,
            total_volume: Uint128::new(1_000_000),
            average_settlement_time: 60,
            slashing_events: 0,
            reputation_score: 9000,
            fee_tier: "premium".to_string(),
            last_updated: 0,
        };

        let success_rate =
            (rep.successful_settlements as f64 / rep.total_settlements as f64) * 100.0;
        assert_eq!(success_rate, 90.0);

        rep.total_settlements = 0;
        let success_rate =
            if rep.total_settlements == 0 { 0.0 } else { (rep.successful_settlements as f64 / rep.total_settlements as f64) * 100.0 };
        assert_eq!(success_rate, 0.0);
    }
}
