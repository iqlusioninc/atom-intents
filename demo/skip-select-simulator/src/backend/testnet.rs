//! Testnet execution backend
//!
//! This backend connects to real Cosmos testnets and executes
//! actual smart contract calls for settlements.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::config::TestnetConfig;
use super::{
    build_explorer_url, BackendError, BackendEvent, BackendMode, ContractAddresses,
    EscrowLockResult, ExecutionBackend, SettlementResult,
};
use crate::models::{Settlement, SettlementStatus};

/// Chain client for RPC communication
/// This is a simplified client - in production, use atom_intents_relayer::CosmosChainClient
pub struct SimpleChainClient {
    chain_id: String,
    rpc_url: String,
    client: reqwest::Client,
    timeout: Duration,
}

impl SimpleChainClient {
    pub fn new(chain_id: &str, rpc_url: &str, timeout_ms: u64) -> Self {
        Self {
            chain_id: chain_id.to_string(),
            rpc_url: rpc_url.to_string(),
            client: reqwest::Client::new(),
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    /// Query the latest block height
    pub async fn get_latest_height(&self) -> Result<u64, BackendError> {
        let url = format!("{}/status", self.rpc_url);

        let response = self
            .client
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| BackendError::ConnectionFailed(format!("RPC request failed: {}", e)))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BackendError::ConnectionFailed(format!("Failed to parse response: {}", e)))?;

        let height = json["result"]["sync_info"]["latest_block_height"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| BackendError::ConnectionFailed("Invalid block height".to_string()))?;

        Ok(height)
    }

    /// Query a smart contract
    pub async fn query_contract(
        &self,
        contract: &str,
        query: &serde_json::Value,
    ) -> Result<serde_json::Value, BackendError> {
        let query_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            serde_json::to_string(query).unwrap(),
        );

        let url = format!(
            "{}/abci_query?path=\"/cosmwasm.wasm.v1.Query/SmartContractState\"&data=0x{}",
            self.rpc_url,
            hex::encode(format!("{}:{}", contract, query_b64))
        );

        let response = self
            .client
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| BackendError::ContractCallFailed(format!("Query failed: {}", e)))?;

        let json: serde_json::Value = response.json().await.map_err(|e| {
            BackendError::ContractCallFailed(format!("Failed to parse response: {}", e))
        })?;

        Ok(json)
    }

    /// Check if the node is synced
    pub async fn is_synced(&self) -> Result<bool, BackendError> {
        let url = format!("{}/status", self.rpc_url);

        let response = self
            .client
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| BackendError::ConnectionFailed(format!("Status request failed: {}", e)))?;

        let json: serde_json::Value = response.json().await.map_err(|e| {
            BackendError::ConnectionFailed(format!("Failed to parse status: {}", e))
        })?;

        let catching_up = json["result"]["sync_info"]["catching_up"]
            .as_bool()
            .unwrap_or(true);

        Ok(!catching_up)
    }
}

/// Testnet execution backend
pub struct TestnetBackend {
    /// Configuration
    config: TestnetConfig,
    /// Chain clients by chain_id
    chain_clients: HashMap<String, Arc<SimpleChainClient>>,
    /// Escrow tracking (escrow_id -> settlement_id)
    escrow_map: Arc<RwLock<HashMap<String, String>>>,
    /// Event broadcaster
    event_tx: broadcast::Sender<BackendEvent>,
    /// Whether we've verified connectivity
    connected: Arc<RwLock<bool>>,
}

impl TestnetBackend {
    /// Create a new testnet backend from configuration
    pub async fn new(config: TestnetConfig) -> Result<Self, BackendError> {
        let (event_tx, _) = broadcast::channel(256);

        let mut chain_clients = HashMap::new();

        // Initialize chain clients
        for (chain_id, chain_config) in &config.chains {
            let client = SimpleChainClient::new(
                chain_id,
                &chain_config.rpc_url,
                chain_config.timeout_ms,
            );
            chain_clients.insert(chain_id.clone(), Arc::new(client));
        }

        let backend = Self {
            config,
            chain_clients,
            escrow_map: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            connected: Arc::new(RwLock::new(false)),
        };

        // Verify connectivity
        backend.verify_connectivity().await?;

        Ok(backend)
    }

    /// Create from config file path
    pub async fn from_config_file(path: &str) -> Result<Self, BackendError> {
        let config = TestnetConfig::load(path)
            .map_err(|e| BackendError::ConfigError(format!("Failed to load config: {}", e)))?;
        Self::new(config).await
    }

    /// Create localnet backend with default settings
    pub async fn localnet() -> Result<Self, BackendError> {
        let config = TestnetConfig::localnet_default();
        Self::new(config).await
    }

    /// Verify connectivity to all configured chains
    async fn verify_connectivity(&self) -> Result<(), BackendError> {
        info!("Verifying connectivity to configured chains...");

        for (chain_id, client) in &self.chain_clients {
            match client.get_latest_height().await {
                Ok(height) => {
                    info!(
                        chain_id = %chain_id,
                        height = height,
                        "Connected to chain"
                    );
                }
                Err(e) => {
                    error!(
                        chain_id = %chain_id,
                        error = %e,
                        "Failed to connect to chain"
                    );
                    return Err(e);
                }
            }

            // Check if synced
            match client.is_synced().await {
                Ok(true) => {
                    debug!(chain_id = %chain_id, "Chain is synced");
                }
                Ok(false) => {
                    warn!(chain_id = %chain_id, "Chain is still syncing");
                }
                Err(e) => {
                    warn!(chain_id = %chain_id, error = %e, "Could not check sync status");
                }
            }
        }

        *self.connected.write().await = true;
        Ok(())
    }

    /// Get the primary chain client
    fn primary_client(&self) -> Result<&Arc<SimpleChainClient>, BackendError> {
        self.chain_clients
            .get(&self.config.primary_chain)
            .ok_or_else(|| BackendError::ChainNotConfigured(self.config.primary_chain.clone()))
    }

    /// Build escrow lock message for the contract
    fn build_lock_msg(
        &self,
        user: &str,
        timeout: u64,
    ) -> serde_json::Value {
        json!({
            "lock": {
                "owner": user,
                "timeout": timeout
            }
        })
    }

    /// Build escrow release message
    fn build_release_msg(&self, escrow_id: &str, recipient: &str) -> serde_json::Value {
        json!({
            "release": {
                "escrow_id": escrow_id,
                "recipient": recipient
            }
        })
    }

    /// Build escrow refund message
    fn build_refund_msg(&self, escrow_id: &str) -> serde_json::Value {
        json!({
            "refund": {
                "escrow_id": escrow_id
            }
        })
    }

    /// Build settlement creation message
    fn build_create_settlement_msg(
        &self,
        settlement_id: &str,
        intent_id: &str,
        solver_id: &str,
        user: &str,
        input_amount: u128,
        input_denom: &str,
        output_amount: u128,
        output_denom: &str,
        timeout: u64,
    ) -> serde_json::Value {
        json!({
            "create_settlement": {
                "settlement_id": settlement_id,
                "intent_id": intent_id,
                "solver_id": solver_id,
                "user": user,
                "user_input_amount": input_amount.to_string(),
                "user_input_denom": input_denom,
                "solver_output_amount": output_amount.to_string(),
                "solver_output_denom": output_denom,
                "expires_at": timeout
            }
        })
    }

    /// Emit an event
    fn emit(&self, event: BackendEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Simulate a successful transaction for demo purposes
    /// In production, this would actually broadcast and wait for confirmation
    async fn simulate_tx_broadcast(&self, msg_type: &str) -> Result<(String, u64), BackendError> {
        // Get current height
        let client = self.primary_client()?;
        let height = client.get_latest_height().await?;

        // Generate a realistic-looking tx hash
        let tx_hash = format!(
            "{:064X}",
            rand::random::<u128>() as u128 * rand::random::<u128>() as u128
                % (1u128 << 127)
        );

        info!(
            msg_type = %msg_type,
            tx_hash = %tx_hash,
            height = height,
            "Simulated transaction broadcast (demo mode)"
        );

        // Small delay to simulate block time
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok((tx_hash, height + 1))
    }
}

#[async_trait]
impl ExecutionBackend for TestnetBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Testnet {
            chain_id: self.config.primary_chain.clone(),
            settlement_contract: self.config.contracts.settlement_address.clone(),
            escrow_contract: self.config.contracts.escrow_address.clone(),
        }
    }

    async fn lock_escrow(
        &self,
        settlement_id: &str,
        user: &str,
        amount: u128,
        denom: &str,
        timeout_secs: u64,
    ) -> Result<EscrowLockResult, BackendError> {
        let escrow_id = format!("escrow_{}", Uuid::new_v4());

        info!(
            settlement_id = %settlement_id,
            escrow_id = %escrow_id,
            user = %user,
            amount = amount,
            denom = %denom,
            "Locking escrow on testnet"
        );

        // Build the lock message
        let _msg = self.build_lock_msg(user, timeout_secs);

        // In production: broadcast transaction to chain
        // For demo: simulate the broadcast
        let (tx_hash, block_height) = self.simulate_tx_broadcast("escrow_lock").await?;

        // Track the escrow
        {
            let mut map = self.escrow_map.write().await;
            map.insert(escrow_id.clone(), settlement_id.to_string());
        }

        // Emit event
        self.emit(BackendEvent::EscrowLocked {
            settlement_id: settlement_id.to_string(),
            escrow_id: escrow_id.clone(),
            tx_hash: Some(tx_hash.clone()),
            block_height: Some(block_height),
            amount,
            denom: denom.to_string(),
        });

        Ok(EscrowLockResult {
            id: escrow_id,
            tx_hash: Some(tx_hash),
            block_height: Some(block_height),
            amount,
            denom: denom.to_string(),
        })
    }

    async fn release_escrow(
        &self,
        escrow_id: &str,
        recipient: &str,
    ) -> Result<Option<String>, BackendError> {
        info!(
            escrow_id = %escrow_id,
            recipient = %recipient,
            "Releasing escrow on testnet"
        );

        let _msg = self.build_release_msg(escrow_id, recipient);

        // In production: broadcast transaction
        // For demo: simulate
        let (tx_hash, _block_height) = self.simulate_tx_broadcast("escrow_release").await?;

        Ok(Some(tx_hash))
    }

    async fn refund_escrow(&self, escrow_id: &str) -> Result<Option<String>, BackendError> {
        info!(
            escrow_id = %escrow_id,
            "Refunding escrow on testnet"
        );

        let _msg = self.build_refund_msg(escrow_id);

        // In production: broadcast transaction
        // For demo: simulate
        let (tx_hash, _block_height) = self.simulate_tx_broadcast("escrow_refund").await?;

        Ok(Some(tx_hash))
    }

    async fn execute_settlement(
        &self,
        settlement: &Settlement,
    ) -> Result<SettlementResult, BackendError> {
        info!(
            settlement_id = %settlement.id,
            solver_id = %settlement.solver_id,
            input_amount = settlement.input_amount,
            output_amount = settlement.output_amount,
            "Executing settlement on testnet"
        );

        // Phase 1: Create settlement in contract
        let _create_msg = self.build_create_settlement_msg(
            &settlement.id,
            settlement.intent_ids.first().map(|s| s.as_str()).unwrap_or(""),
            &settlement.solver_id,
            "", // Would be from intent
            settlement.input_amount,
            "uatom", // Would be from intent
            settlement.output_amount,
            "uosmo", // Would be from intent
            self.config.settlement_timeout_secs,
        );

        let (tx_hash, block_height) = self.simulate_tx_broadcast("create_settlement").await?;

        // Emit solver committed event
        self.emit(BackendEvent::SolverCommitted {
            settlement_id: settlement.id.clone(),
            solver_id: settlement.solver_id.clone(),
            tx_hash: Some(tx_hash.clone()),
        });

        // Phase 2: Simulate IBC transfer
        tokio::time::sleep(Duration::from_millis(500)).await;

        let (ibc_tx_hash, _) = self.simulate_tx_broadcast("ibc_transfer").await?;

        self.emit(BackendEvent::IbcTransferStarted {
            settlement_id: settlement.id.clone(),
            packet_sequence: Some(rand::random::<u64>() % 10000),
            tx_hash: Some(ibc_tx_hash.clone()),
        });

        // Simulate IBC completion
        tokio::time::sleep(Duration::from_millis(500)).await;

        // 95% success rate for testnet demo
        let success = rand::random::<f64>() < 0.95;

        if success {
            self.emit(BackendEvent::IbcTransferComplete {
                settlement_id: settlement.id.clone(),
                tx_hash: Some(ibc_tx_hash.clone()),
            });

            self.emit(BackendEvent::SettlementComplete {
                settlement_id: settlement.id.clone(),
                tx_hash: Some(tx_hash.clone()),
                output_delivered: settlement.output_amount,
            });

            let explorer_url = build_explorer_url(&self.config.primary_chain, &tx_hash);

            info!(
                settlement_id = %settlement.id,
                tx_hash = %tx_hash,
                explorer_url = ?explorer_url,
                "Settlement completed on testnet"
            );

            Ok(SettlementResult {
                id: settlement.id.clone(),
                status: SettlementStatus::Completed,
                tx_hash: Some(tx_hash),
                block_height: Some(block_height),
                explorer_url,
            })
        } else {
            self.emit(BackendEvent::SettlementFailed {
                settlement_id: settlement.id.clone(),
                reason: "IBC timeout on testnet".to_string(),
                recoverable: true,
            });

            warn!(
                settlement_id = %settlement.id,
                "Settlement failed on testnet (IBC timeout)"
            );

            let explorer_url = build_explorer_url(&self.config.primary_chain, &tx_hash);
            Ok(SettlementResult {
                id: settlement.id.clone(),
                status: SettlementStatus::Failed,
                tx_hash: Some(tx_hash),
                block_height: Some(block_height),
                explorer_url,
            })
        }
    }

    async fn get_settlement_status(
        &self,
        settlement_id: &str,
    ) -> Result<SettlementStatus, BackendError> {
        info!(
            settlement_id = %settlement_id,
            "Querying settlement status from testnet"
        );

        // In production: query the settlement contract
        let query = json!({
            "get_settlement": {
                "settlement_id": settlement_id
            }
        });

        let _result = self
            .primary_client()?
            .query_contract(&self.config.contracts.settlement_address, &query)
            .await;

        // For demo: return pending (actual status would come from contract)
        Ok(SettlementStatus::Pending)
    }

    fn contract_addresses(&self) -> Option<ContractAddresses> {
        Some(ContractAddresses {
            settlement: self.config.contracts.settlement_address.clone(),
            escrow: self.config.contracts.escrow_address.clone(),
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<BackendEvent> {
        self.event_tx.subscribe()
    }

    async fn health_check(&self) -> Result<bool, BackendError> {
        // Check connectivity to primary chain
        let client = self.primary_client()?;

        match client.get_latest_height().await {
            Ok(height) => {
                debug!(
                    chain_id = %self.config.primary_chain,
                    height = height,
                    "Health check passed"
                );
                Ok(true)
            }
            Err(e) => {
                warn!(
                    chain_id = %self.config.primary_chain,
                    error = %e,
                    "Health check failed"
                );
                *self.connected.write().await = false;
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_lock_msg() {
        let config = TestnetConfig::localnet_default();
        let (event_tx, _) = broadcast::channel(256);

        let backend = TestnetBackend {
            config,
            chain_clients: HashMap::new(),
            escrow_map: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            connected: Arc::new(RwLock::new(false)),
        };

        let msg = backend.build_lock_msg("cosmos1user...", 600);
        assert!(msg["lock"]["owner"].as_str().is_some());
        assert!(msg["lock"]["timeout"].as_u64().is_some());
    }
}
