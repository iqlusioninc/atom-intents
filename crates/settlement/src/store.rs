use async_trait::async_trait;
use atom_intents_types::{Asset, SettlementStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

// ═══════════════════════════════════════════════════════════════════════════
// CORE TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Persistent settlement record
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettlementRecord {
    pub id: String,
    pub intent_id: String,
    pub solver_id: Option<String>,
    pub user_address: String,
    pub input_asset: Asset,
    pub output_asset: Asset,
    pub status: SettlementStatus,
    pub escrow_id: Option<String>,
    pub solver_bond_id: Option<String>,
    pub ibc_packet_sequence: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub completed_at: Option<u64>,
    pub error_message: Option<String>,
}

impl SettlementRecord {
    /// Create a new settlement record
    pub fn new(
        id: String,
        intent_id: String,
        user_address: String,
        input_asset: Asset,
        output_asset: Asset,
        expires_at: u64,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            intent_id,
            solver_id: None,
            user_address,
            input_asset,
            output_asset,
            status: SettlementStatus::Pending,
            escrow_id: None,
            solver_bond_id: None,
            ibc_packet_sequence: None,
            created_at,
            updated_at: created_at,
            expires_at,
            completed_at: None,
            error_message: None,
        }
    }

    /// Check if settlement is stuck (past timeout and not complete)
    pub fn is_stuck(&self, current_time: u64) -> bool {
        current_time > self.expires_at
            && !matches!(
                self.status,
                SettlementStatus::Complete
                    | SettlementStatus::Failed { .. }
                    | SettlementStatus::TimedOut
            )
    }
}

/// State transition record
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateTransition {
    pub from_status: SettlementStatus,
    pub to_status: SettlementStatus,
    pub timestamp: u64,
    pub details: Option<String>,
    pub tx_hash: Option<String>,
}

impl StateTransition {
    pub fn new(from_status: SettlementStatus, to_status: SettlementStatus, timestamp: u64) -> Self {
        Self {
            from_status,
            to_status,
            timestamp,
            details: None,
            tx_hash: None,
        }
    }

    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }

    pub fn with_tx_hash(mut self, tx_hash: String) -> Self {
        self.tx_hash = Some(tx_hash);
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ERROR TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("settlement not found: {0}")]
    NotFound(String),

    #[error("duplicate settlement ID: {0}")]
    DuplicateId(String),

    #[error("database error: {0}")]
    DatabaseError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("connection error: {0}")]
    ConnectionError(String),
}

// ═══════════════════════════════════════════════════════════════════════════
// STORE TRAIT
// ═══════════════════════════════════════════════════════════════════════════

/// Settlement storage trait - can be implemented for different backends
#[async_trait]
pub trait SettlementStore: Send + Sync {
    /// Store a new settlement
    async fn create(&self, settlement: &SettlementRecord) -> Result<(), StoreError>;

    /// Update settlement status
    async fn update_status(
        &self,
        id: &str,
        status: SettlementStatus,
        details: Option<String>,
    ) -> Result<(), StoreError>;

    /// Update full settlement record
    async fn update(&self, settlement: &SettlementRecord) -> Result<(), StoreError>;

    /// Get settlement by ID
    async fn get(&self, id: &str) -> Result<Option<SettlementRecord>, StoreError>;

    /// Get settlement by intent ID
    async fn get_by_intent(&self, intent_id: &str) -> Result<Option<SettlementRecord>, StoreError>;

    /// List settlements by status
    async fn list_by_status(
        &self,
        status: SettlementStatus,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError>;

    /// List settlements needing recovery (past timeout)
    async fn list_stuck(&self, timeout_threshold: u64)
        -> Result<Vec<SettlementRecord>, StoreError>;

    /// List settlements by solver
    async fn list_by_solver(
        &self,
        solver_id: &str,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError>;

    /// Record a state transition
    async fn record_transition(
        &self,
        id: &str,
        transition: StateTransition,
    ) -> Result<(), StoreError>;

    /// Get transition history for a settlement
    async fn get_history(&self, id: &str) -> Result<Vec<StateTransition>, StoreError>;
}

// ═══════════════════════════════════════════════════════════════════════════
// IN-MEMORY STORE (for testing)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
pub struct InMemoryStore {
    settlements: Arc<RwLock<HashMap<String, SettlementRecord>>>,
    transitions: Arc<RwLock<HashMap<String, Vec<StateTransition>>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            settlements: Arc::new(RwLock::new(HashMap::new())),
            transitions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get number of settlements (for testing)
    pub fn len(&self) -> usize {
        self.settlements.read().unwrap().len()
    }

    /// Check if store is empty (for testing)
    pub fn is_empty(&self) -> bool {
        self.settlements.read().unwrap().is_empty()
    }

    /// Clear all data (for testing)
    pub fn clear(&self) {
        self.settlements.write().unwrap().clear();
        self.transitions.write().unwrap().clear();
    }
}

#[async_trait]
impl SettlementStore for InMemoryStore {
    async fn create(&self, settlement: &SettlementRecord) -> Result<(), StoreError> {
        let mut settlements = self.settlements.write().unwrap();
        if settlements.contains_key(&settlement.id) {
            return Err(StoreError::DuplicateId(settlement.id.clone()));
        }
        settlements.insert(settlement.id.clone(), settlement.clone());
        Ok(())
    }

    async fn update_status(
        &self,
        id: &str,
        status: SettlementStatus,
        details: Option<String>,
    ) -> Result<(), StoreError> {
        let mut settlements = self.settlements.write().unwrap();
        let settlement = settlements
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;

        let old_status = settlement.status.clone();
        settlement.status = status.clone();
        settlement.updated_at = chrono::Utc::now().timestamp() as u64;

        if let SettlementStatus::Failed { reason } = &status {
            settlement.error_message = Some(reason.clone());
        }

        if matches!(status, SettlementStatus::Complete) {
            settlement.completed_at = Some(settlement.updated_at);
        }

        // Record transition
        let transition = StateTransition {
            from_status: old_status,
            to_status: status,
            timestamp: settlement.updated_at,
            details,
            tx_hash: None,
        };

        self.transitions
            .write()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(Vec::new)
            .push(transition);

        Ok(())
    }

    async fn update(&self, settlement: &SettlementRecord) -> Result<(), StoreError> {
        let mut settlements = self.settlements.write().unwrap();
        if !settlements.contains_key(&settlement.id) {
            return Err(StoreError::NotFound(settlement.id.clone()));
        }
        settlements.insert(settlement.id.clone(), settlement.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<SettlementRecord>, StoreError> {
        Ok(self.settlements.read().unwrap().get(id).cloned())
    }

    async fn get_by_intent(&self, intent_id: &str) -> Result<Option<SettlementRecord>, StoreError> {
        Ok(self
            .settlements
            .read()
            .unwrap()
            .values()
            .find(|s| s.intent_id == intent_id)
            .cloned())
    }

    async fn list_by_status(
        &self,
        status: SettlementStatus,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError> {
        let settlements = self.settlements.read().unwrap();
        let mut results: Vec<_> = settlements
            .values()
            .filter(|s| {
                // Match the status, handling Failed variant specially
                match (&s.status, &status) {
                    (SettlementStatus::Failed { .. }, SettlementStatus::Failed { .. }) => true,
                    _ => s.status == status,
                }
            })
            .cloned()
            .collect();

        results.sort_by_key(|s| s.created_at);
        results.truncate(limit);
        Ok(results)
    }

    async fn list_stuck(
        &self,
        timeout_threshold: u64,
    ) -> Result<Vec<SettlementRecord>, StoreError> {
        let settlements = self.settlements.read().unwrap();
        Ok(settlements
            .values()
            .filter(|s| s.is_stuck(timeout_threshold))
            .cloned()
            .collect())
    }

    async fn list_by_solver(
        &self,
        solver_id: &str,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError> {
        let settlements = self.settlements.read().unwrap();
        let mut results: Vec<_> = settlements
            .values()
            .filter(|s| s.solver_id.as_deref() == Some(solver_id))
            .cloned()
            .collect();

        results.sort_by_key(|s| s.created_at);
        results.truncate(limit);
        Ok(results)
    }

    async fn record_transition(
        &self,
        id: &str,
        transition: StateTransition,
    ) -> Result<(), StoreError> {
        // Verify settlement exists
        if !self.settlements.read().unwrap().contains_key(id) {
            return Err(StoreError::NotFound(id.to_string()));
        }

        self.transitions
            .write()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(Vec::new)
            .push(transition);

        Ok(())
    }

    async fn get_history(&self, id: &str) -> Result<Vec<StateTransition>, StoreError> {
        Ok(self
            .transitions
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_settlement() -> SettlementRecord {
        SettlementRecord::new(
            "settlement-1".to_string(),
            "intent-1".to_string(),
            "user-address".to_string(),
            Asset::new("cosmoshub-4", "uatom", 1000000),
            Asset::new("osmosis-1", "uosmo", 5000000),
            1000,
            100,
        )
    }

    #[tokio::test]
    async fn test_create_settlement() {
        let store = InMemoryStore::new();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let retrieved = store.get("settlement-1").await.unwrap();
        assert_eq!(retrieved, Some(settlement));
    }

    #[tokio::test]
    async fn test_duplicate_id_error() {
        let store = InMemoryStore::new();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();
        let result = store.create(&settlement).await;

        assert!(matches!(result, Err(StoreError::DuplicateId(_))));
    }

    #[tokio::test]
    async fn test_update_status() {
        let store = InMemoryStore::new();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();
        store
            .update_status(
                "settlement-1",
                SettlementStatus::UserLocked,
                Some("Locked funds".to_string()),
            )
            .await
            .unwrap();

        let updated = store.get("settlement-1").await.unwrap().unwrap();
        assert_eq!(updated.status, SettlementStatus::UserLocked);

        let history = store.get_history("settlement-1").await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].from_status, SettlementStatus::Pending);
        assert_eq!(history[0].to_status, SettlementStatus::UserLocked);
    }

    #[tokio::test]
    async fn test_get_by_intent() {
        let store = InMemoryStore::new();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let retrieved = store.get_by_intent("intent-1").await.unwrap();
        assert_eq!(retrieved, Some(settlement));
    }

    #[tokio::test]
    async fn test_list_by_status() {
        let store = InMemoryStore::new();

        let mut s1 = create_test_settlement();
        s1.id = "settlement-1".to_string();
        s1.status = SettlementStatus::Pending;

        let mut s2 = create_test_settlement();
        s2.id = "settlement-2".to_string();
        s2.intent_id = "intent-2".to_string();
        s2.status = SettlementStatus::Complete;

        let mut s3 = create_test_settlement();
        s3.id = "settlement-3".to_string();
        s3.intent_id = "intent-3".to_string();
        s3.status = SettlementStatus::Pending;

        store.create(&s1).await.unwrap();
        store.create(&s2).await.unwrap();
        store.create(&s3).await.unwrap();

        let pending = store
            .list_by_status(SettlementStatus::Pending, 10)
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);

        let complete = store
            .list_by_status(SettlementStatus::Complete, 10)
            .await
            .unwrap();
        assert_eq!(complete.len(), 1);
    }

    #[tokio::test]
    async fn test_list_stuck() {
        let store = InMemoryStore::new();

        let mut s1 = create_test_settlement();
        s1.id = "settlement-1".to_string();
        s1.expires_at = 500;
        s1.status = SettlementStatus::Pending;

        let mut s2 = create_test_settlement();
        s2.id = "settlement-2".to_string();
        s2.intent_id = "intent-2".to_string();
        s2.expires_at = 1500;
        s2.status = SettlementStatus::Pending;

        store.create(&s1).await.unwrap();
        store.create(&s2).await.unwrap();

        let stuck = store.list_stuck(1000).await.unwrap();
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].id, "settlement-1");
    }

    #[tokio::test]
    async fn test_list_by_solver() {
        let store = InMemoryStore::new();

        let mut s1 = create_test_settlement();
        s1.id = "settlement-1".to_string();
        s1.solver_id = Some("solver-1".to_string());

        let mut s2 = create_test_settlement();
        s2.id = "settlement-2".to_string();
        s2.intent_id = "intent-2".to_string();
        s2.solver_id = Some("solver-2".to_string());

        let mut s3 = create_test_settlement();
        s3.id = "settlement-3".to_string();
        s3.intent_id = "intent-3".to_string();
        s3.solver_id = Some("solver-1".to_string());

        store.create(&s1).await.unwrap();
        store.create(&s2).await.unwrap();
        store.create(&s3).await.unwrap();

        let solver1 = store.list_by_solver("solver-1", 10).await.unwrap();
        assert_eq!(solver1.len(), 2);
    }

    #[tokio::test]
    async fn test_transition_history() {
        let store = InMemoryStore::new();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let t1 = StateTransition::new(SettlementStatus::Pending, SettlementStatus::UserLocked, 200);
        store.record_transition("settlement-1", t1).await.unwrap();

        let t2 = StateTransition::new(
            SettlementStatus::UserLocked,
            SettlementStatus::SolverLocked,
            300,
        )
        .with_details("Solver locked funds".to_string());
        store.record_transition("settlement-1", t2).await.unwrap();

        let history = store.get_history("settlement-1").await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].timestamp, 200);
        assert_eq!(history[1].timestamp, 300);
        assert_eq!(history[1].details, Some("Solver locked funds".to_string()));
    }

    #[tokio::test]
    async fn test_is_stuck() {
        let mut settlement = create_test_settlement();
        settlement.expires_at = 1000;
        settlement.status = SettlementStatus::Pending;

        assert!(!settlement.is_stuck(500));
        assert!(settlement.is_stuck(1500));

        settlement.status = SettlementStatus::Complete;
        assert!(!settlement.is_stuck(1500));
    }
}
