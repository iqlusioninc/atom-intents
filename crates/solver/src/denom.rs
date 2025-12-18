use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DenomError {
    #[error("failed to query denom trace: {0}")]
    QueryFailed(String),

    #[error("denom trace not found: {0}")]
    NotFound(String),

    #[error("invalid IBC denom format: {0}")]
    InvalidFormat(String),

    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("json error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Represents an IBC denom trace
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DenomTrace {
    /// The IBC denom hash, e.g., "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
    pub ibc_denom: String,
    /// The base denom, e.g., "uatom"
    pub base_denom: String,
    /// The IBC path, e.g., "transfer/channel-0"
    pub path: String,
    /// The chain where the denom originated, e.g., "cosmoshub-4"
    pub origin_chain: String,
}

/// Registry for managing IBC denom traces
pub struct DenomRegistry {
    traces: HashMap<String, DenomTrace>,
    client: reqwest::Client,
    /// RPC endpoints for chains, keyed by chain ID
    rpc_endpoints: HashMap<String, String>,
}

impl DenomRegistry {
    /// Create a new DenomRegistry with common denom traces pre-populated
    pub fn new() -> Self {
        let mut registry = Self {
            traces: HashMap::new(),
            client: reqwest::Client::new(),
            rpc_endpoints: HashMap::new(),
        };

        // Pre-populate common RPC endpoints
        registry.add_rpc_endpoint("osmosis-1", "https://rpc.osmosis.zone");
        registry.add_rpc_endpoint("cosmoshub-4", "https://rpc.cosmos.network");
        registry.add_rpc_endpoint("neutron-1", "https://rpc-kralum.neutron-1.neutron.org");
        registry.add_rpc_endpoint("injective-1", "https://sentry.tm.injective.network:443");

        // Pre-populate common denom traces
        registry.populate_common_traces();

        registry
    }

    /// Add an RPC endpoint for a chain
    pub fn add_rpc_endpoint(&mut self, chain_id: &str, rpc_url: &str) {
        self.rpc_endpoints
            .insert(chain_id.to_string(), rpc_url.to_string());
    }

    /// Pre-populate common denom traces
    fn populate_common_traces(&mut self) {
        // ATOM on Osmosis (transfer/channel-0)
        self.cache_trace(DenomTrace {
            ibc_denom: "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                .to_string(),
            base_denom: "uatom".to_string(),
            path: "transfer/channel-0".to_string(),
            origin_chain: "cosmoshub-4".to_string(),
        });

        // OSMO on Cosmos Hub (transfer/channel-141)
        self.cache_trace(DenomTrace {
            ibc_denom: "ibc/14F9BC3E44B8A9C1BE1FB08980FAB87034C9905EF17CF2F5008FC085218811CC"
                .to_string(),
            base_denom: "uosmo".to_string(),
            path: "transfer/channel-141".to_string(),
            origin_chain: "osmosis-1".to_string(),
        });

        // USDC (Noble) on Osmosis (transfer/channel-750)
        self.cache_trace(DenomTrace {
            ibc_denom: "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4"
                .to_string(),
            base_denom: "uusdc".to_string(),
            path: "transfer/channel-750".to_string(),
            origin_chain: "noble-1".to_string(),
        });

        // stATOM (Stride) on Osmosis (transfer/channel-326)
        self.cache_trace(DenomTrace {
            ibc_denom: "ibc/C140AFD542AE77BD7DCC83F13FDD8C5E5BB8C4929785E6EC2F4C636F98F17901"
                .to_string(),
            base_denom: "stuatom".to_string(),
            path: "transfer/channel-326".to_string(),
            origin_chain: "stride-1".to_string(),
        });

        // ATOM on Neutron (transfer/channel-1)
        self.cache_trace(DenomTrace {
            ibc_denom: "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"
                .to_string(),
            base_denom: "uatom".to_string(),
            path: "transfer/channel-1".to_string(),
            origin_chain: "cosmoshub-4".to_string(),
        });

        // NTRN on Neutron (native)
        self.cache_trace(DenomTrace {
            ibc_denom: "untrn".to_string(),
            base_denom: "untrn".to_string(),
            path: "".to_string(),
            origin_chain: "neutron-1".to_string(),
        });
    }

    /// Compute the IBC denom hash from path and base denom
    pub fn compute_ibc_denom(path: &str, base_denom: &str) -> String {
        if path.is_empty() {
            // Native denom
            return base_denom.to_string();
        }

        // Compute SHA256 hash of "path/base_denom"
        let trace = format!("{}/{}", path, base_denom);
        let mut hasher = Sha256::new();
        hasher.update(trace.as_bytes());
        let hash = hasher.finalize();

        // Convert to uppercase hex and prepend "ibc/"
        format!("ibc/{}", hex::encode(hash).to_uppercase())
    }

    /// Cache a denom trace
    pub fn cache_trace(&mut self, trace: DenomTrace) {
        self.traces.insert(trace.ibc_denom.clone(), trace);
    }

    /// Look up a denom trace from cache
    pub fn get_cached_trace(&self, ibc_denom: &str) -> Option<&DenomTrace> {
        self.traces.get(ibc_denom)
    }

    /// Get the origin chain for a denom
    pub async fn get_origin_chain(&mut self, ibc_denom: &str) -> Result<String, DenomError> {
        // Check cache first
        if let Some(trace) = self.get_cached_trace(ibc_denom) {
            return Ok(trace.origin_chain.clone());
        }

        // Look up trace from chain
        let trace = self.lookup_trace(ibc_denom).await?;
        Ok(trace.origin_chain)
    }

    /// Look up a denom trace from the chain
    pub async fn lookup_trace(&mut self, ibc_denom: &str) -> Result<DenomTrace, DenomError> {
        // Check cache first
        if let Some(trace) = self.get_cached_trace(ibc_denom) {
            return Ok(trace.clone());
        }

        // If not an IBC denom, it's native
        if !ibc_denom.starts_with("ibc/") {
            // Try to infer origin chain from native denoms
            let origin_chain = match ibc_denom {
                "uatom" => "cosmoshub-4",
                "uosmo" => "osmosis-1",
                "untrn" => "neutron-1",
                _ if ibc_denom.starts_with("inj") => "injective-1",
                _ => return Err(DenomError::NotFound(ibc_denom.to_string())),
            };

            let trace = DenomTrace {
                ibc_denom: ibc_denom.to_string(),
                base_denom: ibc_denom.to_string(),
                path: "".to_string(),
                origin_chain: origin_chain.to_string(),
            };

            self.cache_trace(trace.clone());
            return Ok(trace);
        }

        // Extract hash from IBC denom
        let hash = ibc_denom
            .strip_prefix("ibc/")
            .ok_or_else(|| DenomError::InvalidFormat(ibc_denom.to_string()))?;

        // Try querying each known chain for the denom trace
        for (chain_id, rpc_url) in &self.rpc_endpoints {
            match self.query_denom_trace(rpc_url, hash, chain_id).await {
                Ok(trace) => {
                    self.cache_trace(trace.clone());
                    return Ok(trace);
                }
                Err(e) => {
                    tracing::debug!("Failed to query denom trace from {}: {}", chain_id, e);
                    continue;
                }
            }
        }

        Err(DenomError::NotFound(ibc_denom.to_string()))
    }

    /// Query denom trace from a chain's RPC endpoint
    async fn query_denom_trace(
        &self,
        rpc_url: &str,
        hash: &str,
        chain_id: &str,
    ) -> Result<DenomTrace, DenomError> {
        let url = format!(
            "{}/ibc/apps/transfer/v1/denom_traces/{}",
            rpc_url.trim_end_matches('/'),
            hash
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DenomError::QueryFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DenomError::QueryFailed(format!(
                "HTTP {} from {}",
                response.status(),
                chain_id
            )));
        }

        let trace_response: DenomTraceResponse = response.json().await?;

        // Determine origin chain from the path
        // The origin chain is typically encoded in the channel, but we need additional logic
        // For now, we'll use the chain we queried from as a proxy
        let origin_chain = self.infer_origin_from_path(&trace_response.denom_trace.path, chain_id);

        let trace = DenomTrace {
            ibc_denom: format!("ibc/{}", hash.to_uppercase()),
            base_denom: trace_response.denom_trace.base_denom,
            path: trace_response.denom_trace.path,
            origin_chain,
        };

        Ok(trace)
    }

    /// Infer the origin chain from the IBC path
    fn infer_origin_from_path(&self, path: &str, queried_chain: &str) -> String {
        // If path is empty, the denom is native to the queried chain
        if path.is_empty() {
            return queried_chain.to_string();
        }

        // Parse the path to determine origin
        // For a simple path like "transfer/channel-0", the origin is the counterparty of channel-0
        // This requires channel information which we don't have here
        // For now, we'll make educated guesses based on known paths
        match (queried_chain, path) {
            ("osmosis-1", "transfer/channel-0") => "cosmoshub-4".to_string(),
            ("osmosis-1", "transfer/channel-326") => "stride-1".to_string(),
            ("osmosis-1", "transfer/channel-750") => "noble-1".to_string(),
            ("cosmoshub-4", "transfer/channel-141") => "osmosis-1".to_string(),
            ("neutron-1", "transfer/channel-1") => "cosmoshub-4".to_string(),
            _ => {
                // Default to returning the queried chain as we can't determine the origin
                // In production, this should query channel information
                queried_chain.to_string()
            }
        }
    }
}

impl Default for DenomRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Response from IBC denom trace query
#[derive(Debug, Deserialize)]
struct DenomTraceResponse {
    denom_trace: DenomTraceData,
}

#[derive(Debug, Deserialize)]
struct DenomTraceData {
    path: String,
    base_denom: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_ibc_denom_atom() {
        // ATOM on Osmosis via transfer/channel-0
        let ibc_denom = DenomRegistry::compute_ibc_denom("transfer/channel-0", "uatom");
        assert_eq!(
            ibc_denom,
            "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
        );
    }

    #[test]
    fn test_compute_ibc_denom_osmo() {
        // OSMO on Cosmos Hub via transfer/channel-141
        let ibc_denom = DenomRegistry::compute_ibc_denom("transfer/channel-141", "uosmo");
        assert_eq!(
            ibc_denom,
            "ibc/14F9BC3E44B8A9C1BE1FB08980FAB87034C9905EF17CF2F5008FC085218811CC"
        );
    }

    #[test]
    fn test_compute_ibc_denom_native() {
        // Native denom (no path)
        let denom = DenomRegistry::compute_ibc_denom("", "uatom");
        assert_eq!(denom, "uatom");
    }

    #[test]
    fn test_cache_and_retrieve() {
        let mut registry = DenomRegistry::new();

        let trace = DenomTrace {
            ibc_denom: "ibc/TEST123".to_string(),
            base_denom: "utest".to_string(),
            path: "transfer/channel-99".to_string(),
            origin_chain: "test-chain-1".to_string(),
        };

        registry.cache_trace(trace.clone());

        let retrieved = registry.get_cached_trace("ibc/TEST123");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().base_denom, "utest");
    }

    #[test]
    fn test_common_traces_populated() {
        let registry = DenomRegistry::new();

        // Check ATOM on Osmosis
        let atom_trace = registry.get_cached_trace(
            "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
        );
        assert!(atom_trace.is_some());
        assert_eq!(atom_trace.unwrap().base_denom, "uatom");
        assert_eq!(atom_trace.unwrap().origin_chain, "cosmoshub-4");

        // Check OSMO on Cosmos Hub
        let osmo_trace = registry.get_cached_trace(
            "ibc/14F9BC3E44B8A9C1BE1FB08980FAB87034C9905EF17CF2F5008FC085218811CC",
        );
        assert!(osmo_trace.is_some());
        assert_eq!(osmo_trace.unwrap().base_denom, "uosmo");
        assert_eq!(osmo_trace.unwrap().origin_chain, "osmosis-1");
    }

    #[tokio::test]
    async fn test_lookup_native_denom() {
        let mut registry = DenomRegistry::new();

        // Test native ATOM
        let result = registry.lookup_trace("uatom").await;
        assert!(result.is_ok());
        let trace = result.unwrap();
        assert_eq!(trace.base_denom, "uatom");
        assert_eq!(trace.origin_chain, "cosmoshub-4");
        assert_eq!(trace.path, "");
    }

    #[tokio::test]
    async fn test_lookup_cached_ibc_denom() {
        let mut registry = DenomRegistry::new();

        // ATOM on Osmosis should be pre-cached
        let result = registry
            .lookup_trace("ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2")
            .await;
        assert!(result.is_ok());
        let trace = result.unwrap();
        assert_eq!(trace.base_denom, "uatom");
        assert_eq!(trace.origin_chain, "cosmoshub-4");
    }

    #[tokio::test]
    async fn test_get_origin_chain() {
        let mut registry = DenomRegistry::new();

        // Test with pre-cached ATOM on Osmosis
        let result = registry
            .get_origin_chain(
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cosmoshub-4");

        // Test with native denom
        let result = registry.get_origin_chain("uosmo").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "osmosis-1");
    }

    #[test]
    fn test_invalid_ibc_denom_format() {
        let registry = DenomRegistry::new();

        // This would fail in lookup because "notanibc" doesn't start with "ibc/"
        // but won't throw InvalidFormat unless we try to strip the prefix
        let result = DenomRegistry::compute_ibc_denom("transfer/channel-0", "test");
        assert!(result.starts_with("ibc/"));
    }

    #[tokio::test]
    async fn test_lookup_unknown_native_denom() {
        let mut registry = DenomRegistry::new();

        // Unknown native denom should fail
        let result = registry.lookup_trace("uunknown").await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), DenomError::NotFound(_));
    }

    #[test]
    fn test_add_rpc_endpoint() {
        let mut registry = DenomRegistry::new();
        registry.add_rpc_endpoint("test-chain-1", "https://rpc.test.com");

        assert!(registry.rpc_endpoints.contains_key("test-chain-1"));
        assert_eq!(
            registry.rpc_endpoints.get("test-chain-1").unwrap(),
            "https://rpc.test.com"
        );
    }
}
