use async_trait::async_trait;
use atom_intents_types::{Asset, SettlementStatus};
use cosmwasm_std::Uint128;
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

use crate::store::{SettlementRecord, SettlementStore, StateTransition, StoreError};

// ═══════════════════════════════════════════════════════════════════════════
// SQLITE STORE IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════════════

pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Create a new SQLite store with the given database path
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, StoreError> {
        let url = format!("sqlite:{}", db_path.as_ref().display());
        let pool = SqlitePool::connect(&url)
            .await
            .map_err(|e| StoreError::ConnectionError(e.to_string()))?;

        let store = Self { pool };
        store.run_migrations().await?;

        Ok(store)
    }

    /// Create an in-memory SQLite database (for testing)
    pub async fn in_memory() -> Result<Self, StoreError> {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .map_err(|e| StoreError::ConnectionError(e.to_string()))?;

        let store = Self { pool };
        store.run_migrations().await?;

        Ok(store)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), StoreError> {
        // Create settlements table
        sqlx::query(include_str!("../migrations/001_create_settlements.sql"))
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        // Create transitions table
        sqlx::query(include_str!("../migrations/002_create_transitions.sql"))
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Convert database row to SettlementRecord
    fn row_to_settlement(row: &sqlx::sqlite::SqliteRow) -> Result<SettlementRecord, StoreError> {
        let status_str: String = row.get("status");
        let status = parse_settlement_status(&status_str)?;

        let input_asset = Asset {
            chain_id: row.get("input_chain_id"),
            denom: row.get("input_denom"),
            amount: Uint128::from_str(row.get::<String, _>("input_amount").as_str())
                .map_err(|e| StoreError::SerializationError(e.to_string()))?,
        };

        let output_asset = Asset {
            chain_id: row.get("output_chain_id"),
            denom: row.get("output_denom"),
            amount: Uint128::from_str(row.get::<String, _>("output_amount").as_str())
                .map_err(|e| StoreError::SerializationError(e.to_string()))?,
        };

        Ok(SettlementRecord {
            id: row.get("id"),
            intent_id: row.get("intent_id"),
            solver_id: row.get("solver_id"),
            user_address: row.get("user_address"),
            input_asset,
            output_asset,
            status,
            escrow_id: row.get("escrow_id"),
            solver_bond_id: row.get("solver_bond_id"),
            ibc_packet_sequence: row.get::<Option<i64>, _>("ibc_packet_sequence").map(|v| v as u64),
            created_at: row.get::<i64, _>("created_at") as u64,
            updated_at: row.get::<i64, _>("updated_at") as u64,
            expires_at: row.get::<i64, _>("expires_at") as u64,
            completed_at: row.get::<Option<i64>, _>("completed_at").map(|v| v as u64),
            error_message: row.get("error_message"),
        })
    }
}

#[async_trait]
impl SettlementStore for SqliteStore {
    async fn create(&self, settlement: &SettlementRecord) -> Result<(), StoreError> {
        let status_str = settlement_status_to_string(&settlement.status);
        let input_amount = settlement.input_asset.amount.to_string();
        let output_amount = settlement.output_asset.amount.to_string();

        let result = sqlx::query(
            r#"
            INSERT INTO settlements (
                id, intent_id, solver_id, user_address,
                input_chain_id, input_denom, input_amount,
                output_chain_id, output_denom, output_amount,
                status, escrow_id, solver_bond_id, ibc_packet_sequence,
                created_at, updated_at, expires_at, completed_at, error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&settlement.id)
        .bind(&settlement.intent_id)
        .bind(&settlement.solver_id)
        .bind(&settlement.user_address)
        .bind(&settlement.input_asset.chain_id)
        .bind(&settlement.input_asset.denom)
        .bind(&input_amount)
        .bind(&settlement.output_asset.chain_id)
        .bind(&settlement.output_asset.denom)
        .bind(&output_amount)
        .bind(&status_str)
        .bind(&settlement.escrow_id)
        .bind(&settlement.solver_bond_id)
        .bind(settlement.ibc_packet_sequence.map(|v| v as i64))
        .bind(settlement.created_at as i64)
        .bind(settlement.updated_at as i64)
        .bind(settlement.expires_at as i64)
        .bind(settlement.completed_at.map(|v| v as i64))
        .bind(&settlement.error_message)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Err(StoreError::DuplicateId(settlement.id.clone()))
            }
            Err(e) => Err(StoreError::DatabaseError(e.to_string())),
        }
    }

    async fn update_status(
        &self,
        id: &str,
        status: SettlementStatus,
        details: Option<String>,
    ) -> Result<(), StoreError> {
        let status_str = settlement_status_to_string(&status);
        let now = chrono::Utc::now().timestamp();

        // Get the current status for transition recording
        let old_status = match self.get(id).await? {
            Some(s) => s.status,
            None => return Err(StoreError::NotFound(id.to_string())),
        };

        // Update the settlement
        let error_msg = if let SettlementStatus::Failed { reason } = &status {
            Some(reason.clone())
        } else {
            None
        };

        let completed_at = if matches!(status, SettlementStatus::Complete) {
            Some(now)
        } else {
            None
        };

        sqlx::query(
            r#"
            UPDATE settlements
            SET status = ?, updated_at = ?, error_message = ?, completed_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&status_str)
        .bind(now)
        .bind(&error_msg)
        .bind(completed_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        // Record the transition
        let transition = StateTransition {
            from_status: old_status,
            to_status: status,
            timestamp: now as u64,
            details,
            tx_hash: None,
        };
        self.record_transition(id, transition).await?;

        Ok(())
    }

    async fn update(&self, settlement: &SettlementRecord) -> Result<(), StoreError> {
        let status_str = settlement_status_to_string(&settlement.status);
        let input_amount = settlement.input_asset.amount.to_string();
        let output_amount = settlement.output_asset.amount.to_string();

        sqlx::query(
            r#"
            UPDATE settlements
            SET intent_id = ?, solver_id = ?, user_address = ?,
                input_chain_id = ?, input_denom = ?, input_amount = ?,
                output_chain_id = ?, output_denom = ?, output_amount = ?,
                status = ?, escrow_id = ?, solver_bond_id = ?, ibc_packet_sequence = ?,
                updated_at = ?, expires_at = ?, completed_at = ?, error_message = ?
            WHERE id = ?
            "#,
        )
        .bind(&settlement.intent_id)
        .bind(&settlement.solver_id)
        .bind(&settlement.user_address)
        .bind(&settlement.input_asset.chain_id)
        .bind(&settlement.input_asset.denom)
        .bind(&input_amount)
        .bind(&settlement.output_asset.chain_id)
        .bind(&settlement.output_asset.denom)
        .bind(&output_amount)
        .bind(&status_str)
        .bind(&settlement.escrow_id)
        .bind(&settlement.solver_bond_id)
        .bind(settlement.ibc_packet_sequence.map(|v| v as i64))
        .bind(settlement.updated_at as i64)
        .bind(settlement.expires_at as i64)
        .bind(settlement.completed_at.map(|v| v as i64))
        .bind(&settlement.error_message)
        .bind(&settlement.id)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<SettlementRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM settlements WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_settlement(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_by_intent(&self, intent_id: &str) -> Result<Option<SettlementRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM settlements WHERE intent_id = ? LIMIT 1")
            .bind(intent_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_settlement(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_by_status(
        &self,
        status: SettlementStatus,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError> {
        let status_str = settlement_status_to_string(&status);
        let status_prefix = match status {
            SettlementStatus::Failed { .. } => "Failed",
            _ => &status_str,
        };

        let rows = sqlx::query(
            "SELECT * FROM settlements WHERE status LIKE ? ORDER BY created_at ASC LIMIT ?",
        )
        .bind(format!("{}%", status_prefix))
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        rows.iter()
            .map(|row| Self::row_to_settlement(row))
            .collect()
    }

    async fn list_stuck(&self, timeout_threshold: u64) -> Result<Vec<SettlementRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM settlements
            WHERE expires_at < ?
            AND status NOT IN ('Complete', 'TimedOut')
            AND status NOT LIKE 'Failed%'
            "#,
        )
        .bind(timeout_threshold as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        rows.iter()
            .map(|row| Self::row_to_settlement(row))
            .collect()
    }

    async fn list_by_solver(
        &self,
        solver_id: &str,
        limit: usize,
    ) -> Result<Vec<SettlementRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM settlements WHERE solver_id = ? ORDER BY created_at ASC LIMIT ?",
        )
        .bind(solver_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        rows.iter()
            .map(|row| Self::row_to_settlement(row))
            .collect()
    }

    async fn record_transition(&self, id: &str, transition: StateTransition) -> Result<(), StoreError> {
        let from_status = settlement_status_to_string(&transition.from_status);
        let to_status = settlement_status_to_string(&transition.to_status);

        sqlx::query(
            r#"
            INSERT INTO settlement_transitions (
                settlement_id, from_status, to_status, timestamp, details, tx_hash
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(&from_status)
        .bind(&to_status)
        .bind(transition.timestamp as i64)
        .bind(&transition.details)
        .bind(&transition.tx_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    async fn get_history(&self, id: &str) -> Result<Vec<StateTransition>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM settlement_transitions WHERE settlement_id = ? ORDER BY timestamp ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::DatabaseError(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let from_status = parse_settlement_status(row.get("from_status"))?;
                let to_status = parse_settlement_status(row.get("to_status"))?;

                Ok(StateTransition {
                    from_status,
                    to_status,
                    timestamp: row.get::<i64, _>("timestamp") as u64,
                    details: row.get("details"),
                    tx_hash: row.get("tx_hash"),
                })
            })
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

fn settlement_status_to_string(status: &SettlementStatus) -> String {
    match status {
        SettlementStatus::Pending => "Pending".to_string(),
        SettlementStatus::UserLocked => "UserLocked".to_string(),
        SettlementStatus::SolverLocked => "SolverLocked".to_string(),
        SettlementStatus::Executing => "Executing".to_string(),
        SettlementStatus::Complete => "Complete".to_string(),
        SettlementStatus::Failed { reason } => format!("Failed: {}", reason),
        SettlementStatus::TimedOut => "TimedOut".to_string(),
    }
}

fn parse_settlement_status(s: &str) -> Result<SettlementStatus, StoreError> {
    if s.starts_with("Failed: ") {
        let reason = s.strip_prefix("Failed: ").unwrap_or("Unknown").to_string();
        Ok(SettlementStatus::Failed { reason })
    } else {
        match s {
            "Pending" => Ok(SettlementStatus::Pending),
            "UserLocked" => Ok(SettlementStatus::UserLocked),
            "SolverLocked" => Ok(SettlementStatus::SolverLocked),
            "Executing" => Ok(SettlementStatus::Executing),
            "Complete" => Ok(SettlementStatus::Complete),
            "TimedOut" => Ok(SettlementStatus::TimedOut),
            _ => Err(StoreError::SerializationError(format!(
                "Unknown status: {}",
                s
            ))),
        }
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
    async fn test_sqlite_create_settlement() {
        let store = SqliteStore::in_memory().await.unwrap();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let retrieved = store.get("settlement-1").await.unwrap();
        assert_eq!(retrieved, Some(settlement));
    }

    #[tokio::test]
    async fn test_sqlite_duplicate_id_error() {
        let store = SqliteStore::in_memory().await.unwrap();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();
        let result = store.create(&settlement).await;

        assert!(matches!(result, Err(StoreError::DuplicateId(_))));
    }

    #[tokio::test]
    async fn test_sqlite_update_status() {
        let store = SqliteStore::in_memory().await.unwrap();
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
    async fn test_sqlite_get_by_intent() {
        let store = SqliteStore::in_memory().await.unwrap();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let retrieved = store.get_by_intent("intent-1").await.unwrap();
        assert_eq!(retrieved, Some(settlement));
    }

    #[tokio::test]
    async fn test_sqlite_list_by_status() {
        let store = SqliteStore::in_memory().await.unwrap();

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
    async fn test_sqlite_list_stuck() {
        let store = SqliteStore::in_memory().await.unwrap();

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
    async fn test_sqlite_list_by_solver() {
        let store = SqliteStore::in_memory().await.unwrap();

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
    async fn test_sqlite_transition_history() {
        let store = SqliteStore::in_memory().await.unwrap();
        let settlement = create_test_settlement();

        store.create(&settlement).await.unwrap();

        let t1 = StateTransition::new(
            SettlementStatus::Pending,
            SettlementStatus::UserLocked,
            200,
        );
        store
            .record_transition("settlement-1", t1)
            .await
            .unwrap();

        let t2 = StateTransition::new(
            SettlementStatus::UserLocked,
            SettlementStatus::SolverLocked,
            300,
        )
        .with_details("Solver locked funds".to_string());
        store
            .record_transition("settlement-1", t2)
            .await
            .unwrap();

        let history = store.get_history("settlement-1").await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].timestamp, 200);
        assert_eq!(history[1].timestamp, 300);
        assert_eq!(
            history[1].details,
            Some("Solver locked funds".to_string())
        );
    }

    #[tokio::test]
    async fn test_sqlite_failed_status_with_reason() {
        let store = SqliteStore::in_memory().await.unwrap();
        let mut settlement = create_test_settlement();
        settlement.status = SettlementStatus::Failed {
            reason: "Test error".to_string(),
        };

        store.create(&settlement).await.unwrap();

        let retrieved = store.get("settlement-1").await.unwrap().unwrap();
        match retrieved.status {
            SettlementStatus::Failed { reason } => {
                assert_eq!(reason, "Test error");
            }
            _ => panic!("Expected Failed status"),
        }
    }
}
