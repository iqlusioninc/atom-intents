//! Testnet execution backend
//!
//! This backend connects to real Cosmos testnets and executes
//! actual smart contract calls for settlements.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cosmos_sdk_proto::cosmos::tx::v1beta1::TxRaw;
use cosmos_sdk_proto::traits::Message;
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::config::TestnetConfig;
use super::tx_builder::{AccountInfo, TxBuilder};
use super::wallet::{load_wallet_from_env, CosmosWallet};
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

    /// Get account info (account_number and sequence) for signing
    pub async fn get_account_info(&self, address: &str) -> Result<AccountInfo, BackendError> {
        // Use REST endpoint for account info
        let rpc_base = self.rpc_url.trim_end_matches('/');
        // Convert RPC port to REST port (typically 26657 -> 1317)
        let rest_url = rpc_base
            .replace(":26657", ":1317")
            .replace(":26667", ":1327");

        let url = format!("{}/cosmos/auth/v1beta1/accounts/{}", rest_url, address);

        debug!(url = %url, address = %address, "Querying account info");

        let response = self
            .client
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| BackendError::ConnectionFailed(format!("Account query failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Check if account doesn't exist (new account)
            if status.as_u16() == 404 || body.contains("not found") {
                debug!(address = %address, "Account not found, returning default (0, 0)");
                return Ok(AccountInfo {
                    account_number: 0,
                    sequence: 0,
                });
            }

            return Err(BackendError::ConnectionFailed(format!(
                "Account query failed with status {}: {}",
                status, body
            )));
        }

        let json: serde_json::Value = response.json().await.map_err(|e| {
            BackendError::ConnectionFailed(format!("Failed to parse account response: {}", e))
        })?;

        // Handle different account types (BaseAccount, ModuleAccount, etc.)
        let account = &json["account"];

        // Try to get from base_account first (for ModuleAccount, etc.)
        let base_account = account.get("base_account").unwrap_or(account);

        let account_number = base_account["account_number"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let sequence = base_account["sequence"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        debug!(
            address = %address,
            account_number = account_number,
            sequence = sequence,
            "Got account info"
        );

        Ok(AccountInfo {
            account_number,
            sequence,
        })
    }

    /// Broadcast a signed transaction
    pub async fn broadcast_tx(&self, tx: &TxRaw) -> Result<BroadcastResult, BackendError> {
        let tx_bytes = tx.encode_to_vec();
        let tx_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &tx_bytes,
        );

        // Use broadcast_tx_sync for faster response, then poll for confirmation
        let url = format!("{}/broadcast_tx_sync", self.rpc_url);

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "broadcast_tx_sync",
            "params": {
                "tx": tx_b64
            }
        });

        debug!(url = %url, "Broadcasting transaction");

        let response = self
            .client
            .post(&url)
            .json(&body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| BackendError::TransactionFailed(format!("Broadcast failed: {}", e)))?;

        let json: serde_json::Value = response.json().await.map_err(|e| {
            BackendError::TransactionFailed(format!("Failed to parse broadcast response: {}", e))
        })?;

        // Check for RPC-level error
        if let Some(error) = json.get("error") {
            return Err(BackendError::TransactionFailed(format!(
                "RPC error: {}",
                error
            )));
        }

        let result = &json["result"];

        // Check for transaction-level error (CheckTx failure)
        let code = result["code"].as_u64().unwrap_or(0);
        if code != 0 {
            let log = result["log"].as_str().unwrap_or("Unknown error");
            return Err(BackendError::TransactionFailed(format!(
                "Transaction rejected (code {}): {}",
                code, log
            )));
        }

        let tx_hash = result["hash"]
            .as_str()
            .ok_or_else(|| BackendError::TransactionFailed("No tx hash in response".to_string()))?
            .to_string();

        info!(
            tx_hash = %tx_hash,
            chain_id = %self.chain_id,
            "Transaction broadcast successful"
        );

        Ok(BroadcastResult {
            tx_hash,
            code: 0,
        })
    }

    /// Wait for a transaction to be included in a block
    pub async fn wait_for_tx(
        &self,
        tx_hash: &str,
        max_attempts: u32,
        poll_interval_ms: u64,
    ) -> Result<TxResult, BackendError> {
        let url = format!("{}/tx?hash=0x{}", self.rpc_url, tx_hash);

        for attempt in 1..=max_attempts {
            debug!(
                tx_hash = %tx_hash,
                attempt = attempt,
                max_attempts = max_attempts,
                "Polling for transaction"
            );

            let response = self
                .client
                .get(&url)
                .timeout(self.timeout)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let json: serde_json::Value = resp.json().await.map_err(|e| {
                        BackendError::TransactionFailed(format!("Failed to parse tx response: {}", e))
                    })?;

                    // Check if tx was found
                    if let Some(result) = json.get("result") {
                        let height = result["height"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        let tx_result = &result["tx_result"];
                        let code = tx_result["code"].as_u64().unwrap_or(0);

                        if code != 0 {
                            let log = tx_result["log"].as_str().unwrap_or("Unknown error");
                            return Err(BackendError::TransactionFailed(format!(
                                "Transaction failed (code {}): {}",
                                code, log
                            )));
                        }

                        info!(
                            tx_hash = %tx_hash,
                            height = height,
                            "Transaction confirmed"
                        );

                        return Ok(TxResult {
                            tx_hash: tx_hash.to_string(),
                            height,
                            code: 0,
                        });
                    }
                }
                Err(e) => {
                    debug!(
                        tx_hash = %tx_hash,
                        attempt = attempt,
                        error = %e,
                        "Failed to query transaction, retrying"
                    );
                }
            }

            // Wait before next poll
            if attempt < max_attempts {
                tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
            }
        }

        Err(BackendError::Timeout(format!(
            "Transaction {} not confirmed after {} attempts",
            tx_hash, max_attempts
        )))
    }
}

/// Result of broadcasting a transaction
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    pub tx_hash: String,
    pub code: u64,
}

/// Result of a confirmed transaction
#[derive(Debug, Clone)]
pub struct TxResult {
    pub tx_hash: String,
    pub height: u64,
    pub code: u64,
}

/// Testnet execution backend
pub struct TestnetBackend {
    /// Configuration
    config: TestnetConfig,
    /// Chain clients by chain_id
    chain_clients: HashMap<String, Arc<SimpleChainClient>>,
    /// Wallet for signing transactions (loaded from env)
    wallet: Option<CosmosWallet>,
    /// Escrow tracking (escrow_id -> settlement_id)
    escrow_map: Arc<RwLock<HashMap<String, String>>>,
    /// Cached account sequence to avoid race conditions
    account_sequence: Arc<RwLock<u64>>,
    /// Event broadcaster
    event_tx: broadcast::Sender<BackendEvent>,
    /// Whether we've verified connectivity
    connected: Arc<RwLock<bool>>,
    /// Whether to use simulation mode (no real broadcasts)
    simulation_mode: bool,
}

impl TestnetBackend {
    /// Create a new testnet backend from configuration
    pub async fn new(config: TestnetConfig) -> Result<Self, BackendError> {
        Self::new_with_options(config, false).await
    }

    /// Create a testnet backend with simulation mode option
    pub async fn new_with_options(config: TestnetConfig, simulation_mode: bool) -> Result<Self, BackendError> {
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

        // Try to load wallet from environment
        let wallet = match load_wallet_from_env(&config.primary_chain) {
            Ok(w) => {
                let addr = w.address().unwrap_or_default();
                info!(
                    chain_id = %config.primary_chain,
                    address = %addr,
                    "Loaded wallet for testnet transactions"
                );
                Some(w)
            }
            Err(e) => {
                if simulation_mode {
                    warn!(
                        error = %e,
                        "No wallet configured, running in simulation mode"
                    );
                    None
                } else {
                    // In non-simulation mode, we need a wallet
                    warn!(
                        error = %e,
                        "No wallet configured. Set COSMOS_PRIVATE_KEY or chain-specific key. Falling back to simulation mode."
                    );
                    None
                }
            }
        };

        // Determine if we should use simulation mode
        let use_simulation = simulation_mode || wallet.is_none();

        let backend = Self {
            config,
            chain_clients,
            wallet,
            escrow_map: Arc::new(RwLock::new(HashMap::new())),
            account_sequence: Arc::new(RwLock::new(0)),
            event_tx,
            connected: Arc::new(RwLock::new(false)),
            simulation_mode: use_simulation,
        };

        // Verify connectivity
        backend.verify_connectivity().await?;

        // If we have a wallet, fetch initial sequence
        if let Some(ref wallet) = backend.wallet {
            if let Ok(addr) = wallet.address() {
                if let Ok(client) = backend.primary_client() {
                    if let Ok(info) = client.get_account_info(&addr).await {
                        *backend.account_sequence.write().await = info.sequence;
                        info!(
                            address = %addr,
                            account_number = info.account_number,
                            sequence = info.sequence,
                            "Initialized account sequence"
                        );
                    }
                }
            }
        }

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

    /// Execute a contract call, either simulated or real
    async fn execute_contract_call(
        &self,
        contract: &str,
        msg: &serde_json::Value,
        funds: Vec<(u128, &str)>,
        msg_type: &str,
    ) -> Result<(String, u64), BackendError> {
        if self.simulation_mode {
            return self.simulate_tx_broadcast(msg_type).await;
        }

        // Real transaction execution
        let wallet = self.wallet.as_ref().ok_or_else(|| {
            BackendError::ConfigError("No wallet configured for real transactions".to_string())
        })?;

        let client = self.primary_client()?;
        let sender = wallet.address().map_err(|e| {
            BackendError::Internal(format!("Failed to get wallet address: {}", e))
        })?;

        // Get account info (use cached sequence for performance)
        let account_info = client.get_account_info(&sender).await?;

        // Build the transaction
        let memo = format!("atom-intents demo: {}", msg_type);
        let tx_builder = TxBuilder::new(&self.config.primary_chain)
            .with_memo(&memo);

        let execute_msg = tx_builder
            .build_execute_msg(&sender, contract, msg, funds)
            .map_err(|e| BackendError::ContractCallFailed(format!("Failed to build message: {}", e)))?;

        let tx = tx_builder
            .build_and_sign(wallet, &account_info, execute_msg)
            .map_err(|e| BackendError::TransactionFailed(format!("Failed to sign transaction: {}", e)))?;

        // Broadcast the transaction
        let broadcast_result = client.broadcast_tx(&tx).await?;

        // Update cached sequence
        {
            let mut seq = self.account_sequence.write().await;
            *seq = account_info.sequence + 1;
        }

        // Wait for confirmation
        let tx_result = client
            .wait_for_tx(&broadcast_result.tx_hash, 30, 1000)
            .await?;

        Ok((tx_result.tx_hash, tx_result.height))
    }

    /// Simulate a successful transaction for demo purposes
    /// Used when no wallet is configured or in simulation mode
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
            simulation_mode = true,
            "Simulated transaction broadcast (no wallet configured)"
        );

        // Small delay to simulate block time
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok((tx_hash, height + 1))
    }

    /// Check if running in simulation mode
    pub fn is_simulation_mode(&self) -> bool {
        self.simulation_mode
    }

    /// Get the wallet address (if configured)
    pub fn wallet_address(&self) -> Option<String> {
        self.wallet.as_ref().and_then(|w| w.address().ok())
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
            simulation_mode = self.simulation_mode,
            "Locking escrow on testnet"
        );

        // Build the lock message
        let msg = self.build_lock_msg(user, timeout_secs);

        // Execute contract call (real or simulated)
        let (tx_hash, block_height) = self
            .execute_contract_call(
                &self.config.contracts.escrow_address,
                &msg,
                vec![(amount, denom)],
                "escrow_lock",
            )
            .await?;

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
            simulation_mode = self.simulation_mode,
            "Releasing escrow on testnet"
        );

        let msg = self.build_release_msg(escrow_id, recipient);

        let (tx_hash, _block_height) = self
            .execute_contract_call(
                &self.config.contracts.escrow_address,
                &msg,
                vec![],
                "escrow_release",
            )
            .await?;

        Ok(Some(tx_hash))
    }

    async fn refund_escrow(&self, escrow_id: &str) -> Result<Option<String>, BackendError> {
        info!(
            escrow_id = %escrow_id,
            simulation_mode = self.simulation_mode,
            "Refunding escrow on testnet"
        );

        let msg = self.build_refund_msg(escrow_id);

        let (tx_hash, _block_height) = self
            .execute_contract_call(
                &self.config.contracts.escrow_address,
                &msg,
                vec![],
                "escrow_refund",
            )
            .await?;

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
            simulation_mode = self.simulation_mode,
            "Executing settlement on testnet"
        );

        // Extract denomination info from intent (fallback to defaults)
        // TODO: Wire this properly from the intent data
        let input_denom = "uatom";
        let output_denom = "uosmo";
        let user_output_address = ""; // Would come from intent

        // Phase 1: Create settlement in contract
        let create_msg = self.build_create_settlement_msg(
            &settlement.id,
            settlement.intent_ids.first().map(|s| s.as_str()).unwrap_or(""),
            &settlement.solver_id,
            user_output_address,
            settlement.input_amount,
            input_denom,
            settlement.output_amount,
            output_denom,
            self.config.settlement_timeout_secs,
        );

        let (tx_hash, block_height) = self
            .execute_contract_call(
                &self.config.contracts.settlement_address,
                &create_msg,
                vec![],
                "create_settlement",
            )
            .await?;

        // Emit solver committed event
        self.emit(BackendEvent::SolverCommitted {
            settlement_id: settlement.id.clone(),
            solver_id: settlement.solver_id.clone(),
            tx_hash: Some(tx_hash.clone()),
        });

        // Phase 2: IBC transfer
        // Note: Real IBC transfers require relayer infrastructure
        // For now, we simulate this step even in real mode
        tokio::time::sleep(Duration::from_millis(500)).await;

        let ibc_tx_hash = if self.simulation_mode {
            let (hash, _) = self.simulate_tx_broadcast("ibc_transfer").await?;
            hash
        } else {
            // In real mode, we would initiate an actual IBC transfer here
            // For now, still simulate but log that this needs implementation
            warn!(
                settlement_id = %settlement.id,
                "IBC transfer requires relayer - simulating for now"
            );
            let (hash, _) = self.simulate_tx_broadcast("ibc_transfer").await?;
            hash
        };

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
                simulation_mode = self.simulation_mode,
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
            wallet: None,
            escrow_map: Arc::new(RwLock::new(HashMap::new())),
            account_sequence: Arc::new(RwLock::new(0)),
            event_tx,
            connected: Arc::new(RwLock::new(false)),
            simulation_mode: true,
        };

        let msg = backend.build_lock_msg("cosmos1user...", 600);
        assert!(msg["lock"]["owner"].as_str().is_some());
        assert!(msg["lock"]["timeout"].as_u64().is_some());
    }

    #[test]
    fn test_wallet_address() {
        let config = TestnetConfig::localnet_default();
        let (event_tx, _) = broadcast::channel(256);

        let backend = TestnetBackend {
            config,
            chain_clients: HashMap::new(),
            wallet: None,
            escrow_map: Arc::new(RwLock::new(HashMap::new())),
            account_sequence: Arc::new(RwLock::new(0)),
            event_tx,
            connected: Arc::new(RwLock::new(false)),
            simulation_mode: true,
        };

        // No wallet configured
        assert!(backend.wallet_address().is_none());
        assert!(backend.is_simulation_mode());
    }
}
