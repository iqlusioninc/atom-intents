//! gRPC client for Cosmos chain interactions
//!
//! This module provides gRPC-based queries for more reliable
//! chain state access compared to REST/JSON-RPC.

use cosmos_sdk_proto::cosmos::auth::v1beta1::{
    query_client::QueryClient as AuthQueryClient, BaseAccount, QueryAccountRequest,
};
use cosmos_sdk_proto::cosmos::bank::v1beta1::{
    query_client::QueryClient as BankQueryClient, QueryBalanceRequest,
};
use cosmos_sdk_proto::cosmos::base::tendermint::v1beta1::{
    service_client::ServiceClient as TendermintServiceClient, GetLatestBlockRequest,
};
use cosmos_sdk_proto::cosmos::tx::v1beta1::{
    service_client::ServiceClient as TxServiceClient, BroadcastMode, BroadcastTxRequest,
    GetTxRequest,
};
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
};
use cosmos_sdk_proto::traits::Message;
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tracing::{debug, info, warn};

use super::tx_builder::AccountInfo;
use super::BackendError;

/// gRPC client for a Cosmos chain
pub struct CosmosGrpcClient {
    /// Chain ID
    chain_id: String,
    /// gRPC endpoint URL
    endpoint: String,
    /// gRPC channel (lazily connected)
    channel: Option<Channel>,
    /// Connection timeout
    timeout: Duration,
    /// Whether to use TLS
    use_tls: bool,
}

impl CosmosGrpcClient {
    /// Create a new gRPC client
    pub fn new(chain_id: &str, grpc_endpoint: &str, timeout_ms: u64) -> Self {
        // Determine if TLS should be used based on endpoint
        let use_tls = grpc_endpoint.starts_with("https://") || grpc_endpoint.contains(":443");

        Self {
            chain_id: chain_id.to_string(),
            endpoint: grpc_endpoint.to_string(),
            channel: None,
            timeout: Duration::from_millis(timeout_ms),
            use_tls,
        }
    }

    /// Get or create the gRPC channel
    async fn get_channel(&mut self) -> Result<Channel, BackendError> {
        if let Some(ref channel) = self.channel {
            return Ok(channel.clone());
        }

        // Parse endpoint
        let endpoint_url = if self.endpoint.starts_with("http") {
            self.endpoint.clone()
        } else {
            format!("http://{}", self.endpoint)
        };

        debug!(endpoint = %endpoint_url, "Connecting to gRPC endpoint");

        let mut endpoint = Endpoint::from_shared(endpoint_url.clone())
            .map_err(|e| BackendError::ConnectionFailed(format!("Invalid endpoint: {}", e)))?
            .timeout(self.timeout)
            .connect_timeout(Duration::from_secs(10));

        if self.use_tls {
            let tls_config = ClientTlsConfig::new();
            endpoint = endpoint
                .tls_config(tls_config)
                .map_err(|e| BackendError::ConnectionFailed(format!("TLS config error: {}", e)))?;
        }

        let channel = endpoint
            .connect()
            .await
            .map_err(|e| BackendError::ConnectionFailed(format!("gRPC connect failed: {}", e)))?;

        info!(
            chain_id = %self.chain_id,
            endpoint = %endpoint_url,
            "Connected to gRPC endpoint"
        );

        self.channel = Some(channel.clone());
        Ok(channel)
    }

    /// Query account info (account number and sequence)
    pub async fn get_account_info(&mut self, address: &str) -> Result<AccountInfo, BackendError> {
        let channel = self.get_channel().await?;

        let mut client = AuthQueryClient::new(channel);

        let request = QueryAccountRequest {
            address: address.to_string(),
        };

        debug!(address = %address, "Querying account via gRPC");

        match client.account(request).await {
            Ok(response) => {
                let account_any = response
                    .into_inner()
                    .account
                    .ok_or_else(|| BackendError::NotFound("Account not found".to_string()))?;

                // Decode the Any type to BaseAccount
                let base_account = if account_any.type_url.contains("BaseAccount") {
                    BaseAccount::decode(account_any.value.as_slice()).map_err(|e| {
                        BackendError::Internal(format!("Failed to decode account: {}", e))
                    })?
                } else {
                    // Try to extract base_account from other types (e.g., ModuleAccount)
                    // For now, just try direct decode
                    BaseAccount::decode(account_any.value.as_slice()).map_err(|e| {
                        BackendError::Internal(format!(
                            "Unknown account type {}: {}",
                            account_any.type_url, e
                        ))
                    })?
                };

                debug!(
                    address = %address,
                    account_number = base_account.account_number,
                    sequence = base_account.sequence,
                    "Got account info via gRPC"
                );

                Ok(AccountInfo {
                    account_number: base_account.account_number,
                    sequence: base_account.sequence,
                })
            }
            Err(status) => {
                if status.code() == tonic::Code::NotFound {
                    // New account, return defaults
                    debug!(address = %address, "Account not found, returning default (0, 0)");
                    Ok(AccountInfo {
                        account_number: 0,
                        sequence: 0,
                    })
                } else {
                    Err(BackendError::ConnectionFailed(format!(
                        "gRPC account query failed: {}",
                        status
                    )))
                }
            }
        }
    }

    /// Query account balance
    pub async fn get_balance(
        &mut self,
        address: &str,
        denom: &str,
    ) -> Result<u128, BackendError> {
        let channel = self.get_channel().await?;

        let mut client = BankQueryClient::new(channel);

        let request = QueryBalanceRequest {
            address: address.to_string(),
            denom: denom.to_string(),
        };

        let response = client.balance(request).await.map_err(|e| {
            BackendError::ConnectionFailed(format!("gRPC balance query failed: {}", e))
        })?;

        let coin = response.into_inner().balance;

        let amount = coin
            .map(|c| c.amount.parse::<u128>().unwrap_or(0))
            .unwrap_or(0);

        debug!(
            address = %address,
            denom = %denom,
            amount = amount,
            "Got balance via gRPC"
        );

        Ok(amount)
    }

    /// Query latest block height
    pub async fn get_latest_height(&mut self) -> Result<u64, BackendError> {
        let channel = self.get_channel().await?;

        let mut client = TendermintServiceClient::new(channel);

        let request = GetLatestBlockRequest {};

        let response = client.get_latest_block(request).await.map_err(|e| {
            BackendError::ConnectionFailed(format!("gRPC block query failed: {}", e))
        })?;

        let height = response
            .into_inner()
            .block
            .and_then(|b| b.header)
            .map(|h| h.height as u64)
            .unwrap_or(0);

        debug!(chain_id = %self.chain_id, height = height, "Got latest block height via gRPC");

        Ok(height)
    }

    /// Query smart contract state
    pub async fn query_contract(
        &mut self,
        contract: &str,
        query: &serde_json::Value,
    ) -> Result<serde_json::Value, BackendError> {
        let channel = self.get_channel().await?;

        let mut client = WasmQueryClient::new(channel);

        let query_data = serde_json::to_vec(query)
            .map_err(|e| BackendError::ContractCallFailed(format!("JSON encode error: {}", e)))?;

        let request = QuerySmartContractStateRequest {
            address: contract.to_string(),
            query_data,
        };

        let response = client.smart_contract_state(request).await.map_err(|e| {
            BackendError::ContractCallFailed(format!("gRPC contract query failed: {}", e))
        })?;

        let result_data = response.into_inner().data;

        let result: serde_json::Value = serde_json::from_slice(&result_data).map_err(|e| {
            BackendError::ContractCallFailed(format!("JSON decode error: {}", e))
        })?;

        debug!(contract = %contract, "Got contract state via gRPC");

        Ok(result)
    }

    /// Broadcast a signed transaction
    pub async fn broadcast_tx(
        &mut self,
        tx_bytes: Vec<u8>,
        mode: BroadcastMode,
    ) -> Result<BroadcastTxResult, BackendError> {
        let channel = self.get_channel().await?;

        let mut client = TxServiceClient::new(channel);

        let request = BroadcastTxRequest {
            tx_bytes,
            mode: mode as i32,
        };

        let response = client.broadcast_tx(request).await.map_err(|e| {
            BackendError::TransactionFailed(format!("gRPC broadcast failed: {}", e))
        })?;

        let tx_response = response
            .into_inner()
            .tx_response
            .ok_or_else(|| BackendError::TransactionFailed("No tx response".to_string()))?;

        if tx_response.code != 0 {
            return Err(BackendError::TransactionFailed(format!(
                "Transaction failed (code {}): {}",
                tx_response.code, tx_response.raw_log
            )));
        }

        info!(
            tx_hash = %tx_response.txhash,
            chain_id = %self.chain_id,
            "Transaction broadcast successful via gRPC"
        );

        Ok(BroadcastTxResult {
            tx_hash: tx_response.txhash,
            height: tx_response.height as u64,
            code: tx_response.code,
        })
    }

    /// Wait for a transaction to be confirmed
    pub async fn wait_for_tx(
        &mut self,
        tx_hash: &str,
        max_attempts: u32,
        poll_interval_ms: u64,
    ) -> Result<TxResult, BackendError> {
        for attempt in 1..=max_attempts {
            debug!(
                tx_hash = %tx_hash,
                attempt = attempt,
                max_attempts = max_attempts,
                "Polling for transaction via gRPC"
            );

            let channel = self.get_channel().await?;
            let mut client = TxServiceClient::new(channel);

            let request = GetTxRequest {
                hash: tx_hash.to_string(),
            };

            match client.get_tx(request).await {
                Ok(response) => {
                    let tx_response = response.into_inner().tx_response.ok_or_else(|| {
                        BackendError::TransactionFailed("No tx response".to_string())
                    })?;

                    if tx_response.code != 0 {
                        return Err(BackendError::TransactionFailed(format!(
                            "Transaction failed (code {}): {}",
                            tx_response.code, tx_response.raw_log
                        )));
                    }

                    info!(
                        tx_hash = %tx_hash,
                        height = tx_response.height,
                        "Transaction confirmed via gRPC"
                    );

                    return Ok(TxResult {
                        tx_hash: tx_response.txhash,
                        height: tx_response.height as u64,
                        code: tx_response.code,
                    });
                }
                Err(e) => {
                    if e.code() != tonic::Code::NotFound {
                        warn!(
                            tx_hash = %tx_hash,
                            error = %e,
                            "gRPC query error, retrying"
                        );
                    }
                }
            }

            if attempt < max_attempts {
                tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
            }
        }

        Err(BackendError::Timeout(format!(
            "Transaction {} not confirmed after {} attempts",
            tx_hash, max_attempts
        )))
    }

    /// Health check
    pub async fn health_check(&mut self) -> Result<bool, BackendError> {
        match self.get_latest_height().await {
            Ok(height) => {
                debug!(
                    chain_id = %self.chain_id,
                    height = height,
                    "gRPC health check passed"
                );
                Ok(true)
            }
            Err(e) => {
                warn!(
                    chain_id = %self.chain_id,
                    error = %e,
                    "gRPC health check failed"
                );
                Ok(false)
            }
        }
    }
}

/// Result of broadcasting a transaction
#[derive(Debug, Clone)]
pub struct BroadcastTxResult {
    pub tx_hash: String,
    pub height: u64,
    pub code: u32,
}

/// Result of a confirmed transaction
#[derive(Debug, Clone)]
pub struct TxResult {
    pub tx_hash: String,
    pub height: u64,
    pub code: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_client_creation() {
        let client = CosmosGrpcClient::new("theta-testnet-001", "grpc.testnet.cosmos.network:443", 10000);
        assert_eq!(client.chain_id, "theta-testnet-001");
        assert!(client.use_tls); // Port 443 should enable TLS
    }

    #[test]
    fn test_grpc_client_local() {
        let client = CosmosGrpcClient::new("localhub-1", "localhost:9090", 5000);
        assert_eq!(client.chain_id, "localhub-1");
        assert!(!client.use_tls); // Local port shouldn't use TLS
    }
}
