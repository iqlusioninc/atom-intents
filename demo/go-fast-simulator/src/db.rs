//! Database persistence layer for production deployment
//!
//! Provides PostgreSQL storage for intents, auctions, and settlements.
//! Falls back to in-memory storage when the `postgres` feature is not enabled.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::*;

/// Result type for database operations
pub type DbResult<T> = Result<T, DbError>;

/// Database errors
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Duplicate key: {0}")]
    DuplicateKey(String),

    #[cfg(feature = "postgres")]
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Storage trait for abstracting between in-memory and PostgreSQL storage
#[async_trait]
pub trait Storage: Send + Sync {
    // Intents
    async fn insert_intent(&self, intent: &Intent) -> DbResult<()>;
    async fn get_intent(&self, id: &str) -> DbResult<Option<Intent>>;
    async fn list_intents(&self, limit: usize, offset: usize) -> DbResult<Vec<Intent>>;
    async fn get_pending_intents(&self) -> DbResult<Vec<Intent>>;
    async fn update_intent_status(&self, id: &str, status: IntentStatus) -> DbResult<()>;

    // Auctions
    async fn insert_auction(&self, auction: &Auction) -> DbResult<()>;
    async fn get_auction(&self, id: &str) -> DbResult<Option<Auction>>;
    async fn get_current_auction(&self) -> DbResult<Option<Auction>>;
    async fn update_auction(&self, auction: &Auction) -> DbResult<()>;
    async fn list_auctions(&self, limit: usize) -> DbResult<Vec<Auction>>;

    // Settlements
    async fn insert_settlement(&self, settlement: &Settlement) -> DbResult<()>;
    async fn get_settlement(&self, id: &str) -> DbResult<Option<Settlement>>;
    async fn update_settlement(&self, settlement: &Settlement) -> DbResult<()>;
    async fn list_settlements(&self, limit: usize) -> DbResult<Vec<Settlement>>;

    // Solvers
    async fn get_solvers(&self) -> DbResult<Vec<Solver>>;
    async fn update_solver(&self, solver: &Solver) -> DbResult<()>;

    // Stats
    async fn get_stats(&self) -> DbResult<SystemStats>;
    async fn update_stats(&self, stats: &SystemStats) -> DbResult<()>;
}

/// In-memory storage implementation (for local development)
pub struct MemoryStorage {
    intents: RwLock<HashMap<String, Intent>>,
    auctions: RwLock<HashMap<String, Auction>>,
    settlements: RwLock<HashMap<String, Settlement>>,
    solvers: RwLock<HashMap<String, Solver>>,
    stats: RwLock<SystemStats>,
    current_auction_id: RwLock<Option<String>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            intents: RwLock::new(HashMap::new()),
            auctions: RwLock::new(HashMap::new()),
            settlements: RwLock::new(HashMap::new()),
            solvers: RwLock::new(HashMap::new()),
            stats: RwLock::new(SystemStats::default()),
            current_auction_id: RwLock::new(None),
        }
    }

    pub async fn set_current_auction(&self, id: Option<String>) {
        let mut current = self.current_auction_id.write().await;
        *current = id;
    }

    pub async fn init_solvers(&self, solvers: Vec<Solver>) {
        let mut store = self.solvers.write().await;
        for solver in solvers {
            store.insert(solver.id.clone(), solver);
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn insert_intent(&self, intent: &Intent) -> DbResult<()> {
        let mut intents = self.intents.write().await;
        if intents.contains_key(&intent.id) {
            return Err(DbError::DuplicateKey(intent.id.clone()));
        }
        intents.insert(intent.id.clone(), intent.clone());
        Ok(())
    }

    async fn get_intent(&self, id: &str) -> DbResult<Option<Intent>> {
        let intents = self.intents.read().await;
        Ok(intents.get(id).cloned())
    }

    async fn list_intents(&self, limit: usize, offset: usize) -> DbResult<Vec<Intent>> {
        let intents = self.intents.read().await;
        let mut result: Vec<_> = intents.values().cloned().collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result.into_iter().skip(offset).take(limit).collect())
    }

    async fn get_pending_intents(&self) -> DbResult<Vec<Intent>> {
        let intents = self.intents.read().await;
        Ok(intents
            .values()
            .filter(|i| i.status == IntentStatus::Pending && !i.is_expired())
            .cloned()
            .collect())
    }

    async fn update_intent_status(&self, id: &str, status: IntentStatus) -> DbResult<()> {
        let mut intents = self.intents.write().await;
        if let Some(intent) = intents.get_mut(id) {
            intent.status = status;
            Ok(())
        } else {
            Err(DbError::NotFound(id.to_string()))
        }
    }

    async fn insert_auction(&self, auction: &Auction) -> DbResult<()> {
        let mut auctions = self.auctions.write().await;
        auctions.insert(auction.id.clone(), auction.clone());
        if auction.status == AuctionStatus::Open {
            let mut current = self.current_auction_id.write().await;
            *current = Some(auction.id.clone());
        }
        Ok(())
    }

    async fn get_auction(&self, id: &str) -> DbResult<Option<Auction>> {
        let auctions = self.auctions.read().await;
        Ok(auctions.get(id).cloned())
    }

    async fn get_current_auction(&self) -> DbResult<Option<Auction>> {
        let current_id = self.current_auction_id.read().await;
        if let Some(id) = current_id.as_ref() {
            let auctions = self.auctions.read().await;
            Ok(auctions.get(id).cloned())
        } else {
            Ok(None)
        }
    }

    async fn update_auction(&self, auction: &Auction) -> DbResult<()> {
        let mut auctions = self.auctions.write().await;
        auctions.insert(auction.id.clone(), auction.clone());
        Ok(())
    }

    async fn list_auctions(&self, limit: usize) -> DbResult<Vec<Auction>> {
        let auctions = self.auctions.read().await;
        let mut result: Vec<_> = auctions.values().cloned().collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result.into_iter().take(limit).collect())
    }

    async fn insert_settlement(&self, settlement: &Settlement) -> DbResult<()> {
        let mut settlements = self.settlements.write().await;
        settlements.insert(settlement.id.clone(), settlement.clone());
        Ok(())
    }

    async fn get_settlement(&self, id: &str) -> DbResult<Option<Settlement>> {
        let settlements = self.settlements.read().await;
        Ok(settlements.get(id).cloned())
    }

    async fn update_settlement(&self, settlement: &Settlement) -> DbResult<()> {
        let mut settlements = self.settlements.write().await;
        settlements.insert(settlement.id.clone(), settlement.clone());
        Ok(())
    }

    async fn list_settlements(&self, limit: usize) -> DbResult<Vec<Settlement>> {
        let settlements = self.settlements.read().await;
        let mut result: Vec<_> = settlements.values().cloned().collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result.into_iter().take(limit).collect())
    }

    async fn get_solvers(&self) -> DbResult<Vec<Solver>> {
        let solvers = self.solvers.read().await;
        Ok(solvers.values().cloned().collect())
    }

    async fn update_solver(&self, solver: &Solver) -> DbResult<()> {
        let mut solvers = self.solvers.write().await;
        solvers.insert(solver.id.clone(), solver.clone());
        Ok(())
    }

    async fn get_stats(&self) -> DbResult<SystemStats> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }

    async fn update_stats(&self, new_stats: &SystemStats) -> DbResult<()> {
        let mut stats = self.stats.write().await;
        *stats = new_stats.clone();
        Ok(())
    }
}

/// PostgreSQL storage implementation (for production)
#[cfg(feature = "postgres")]
pub struct PostgresStorage {
    pool: sqlx::PgPool,
}

#[cfg(feature = "postgres")]
impl PostgresStorage {
    pub async fn new(database_url: &str) -> DbResult<Self> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> DbResult<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl Storage for PostgresStorage {
    async fn insert_intent(&self, intent: &Intent) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO intents (id, user_address, input_denom, input_amount, output_denom,
                                min_output_amount, max_slippage_bps, status, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(&intent.id)
        .bind(&intent.user_address)
        .bind(&intent.input_denom)
        .bind(intent.input_amount as i64)
        .bind(&intent.output_denom)
        .bind(intent.min_output_amount as i64)
        .bind(intent.max_slippage_bps as i32)
        .bind(serde_json::to_string(&intent.status)?)
        .bind(intent.created_at)
        .bind(intent.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_intent(&self, id: &str) -> DbResult<Option<Intent>> {
        let row = sqlx::query_as::<_, IntentRow>(
            "SELECT * FROM intents WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(r.into_intent()?)),
            None => Ok(None),
        }
    }

    async fn list_intents(&self, limit: usize, offset: usize) -> DbResult<Vec<Intent>> {
        let rows = sqlx::query_as::<_, IntentRow>(
            "SELECT * FROM intents ORDER BY created_at DESC LIMIT $1 OFFSET $2"
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.into_intent()).collect()
    }

    async fn get_pending_intents(&self) -> DbResult<Vec<Intent>> {
        let rows = sqlx::query_as::<_, IntentRow>(
            r#"SELECT * FROM intents
               WHERE status = '"Pending"' AND expires_at > NOW()
               ORDER BY created_at ASC"#
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.into_intent()).collect()
    }

    async fn update_intent_status(&self, id: &str, status: IntentStatus) -> DbResult<()> {
        sqlx::query("UPDATE intents SET status = $1 WHERE id = $2")
            .bind(serde_json::to_string(&status)?)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn insert_auction(&self, auction: &Auction) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO auctions (id, status, intent_ids, quotes, winning_quote_id,
                                 clearing_price, created_at, closed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(&auction.id)
        .bind(serde_json::to_string(&auction.status)?)
        .bind(serde_json::to_value(&auction.intent_ids)?)
        .bind(serde_json::to_value(&auction.quotes)?)
        .bind(&auction.winning_quote_id)
        .bind(auction.clearing_price)
        .bind(auction.created_at)
        .bind(auction.closed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_auction(&self, id: &str) -> DbResult<Option<Auction>> {
        let row = sqlx::query_as::<_, AuctionRow>(
            "SELECT * FROM auctions WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(r.into_auction()?)),
            None => Ok(None),
        }
    }

    async fn get_current_auction(&self) -> DbResult<Option<Auction>> {
        let row = sqlx::query_as::<_, AuctionRow>(
            r#"SELECT * FROM auctions WHERE status = '"Open"' ORDER BY created_at DESC LIMIT 1"#
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(r.into_auction()?)),
            None => Ok(None),
        }
    }

    async fn update_auction(&self, auction: &Auction) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE auctions
            SET status = $2, quotes = $3, winning_quote_id = $4, clearing_price = $5, closed_at = $6
            WHERE id = $1
            "#,
        )
        .bind(&auction.id)
        .bind(serde_json::to_string(&auction.status)?)
        .bind(serde_json::to_value(&auction.quotes)?)
        .bind(&auction.winning_quote_id)
        .bind(auction.clearing_price)
        .bind(auction.closed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_auctions(&self, limit: usize) -> DbResult<Vec<Auction>> {
        let rows = sqlx::query_as::<_, AuctionRow>(
            "SELECT * FROM auctions ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.into_auction()).collect()
    }

    async fn insert_settlement(&self, settlement: &Settlement) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO settlements (id, auction_id, intent_ids, solver_id, status, phases,
                                    created_at, completed_at, execution_time_ms)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&settlement.id)
        .bind(&settlement.auction_id)
        .bind(serde_json::to_value(&settlement.intent_ids)?)
        .bind(&settlement.solver_id)
        .bind(serde_json::to_string(&settlement.status)?)
        .bind(serde_json::to_value(&settlement.phases)?)
        .bind(settlement.created_at)
        .bind(settlement.completed_at)
        .bind(settlement.execution_time_ms.map(|v| v as i64))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_settlement(&self, id: &str) -> DbResult<Option<Settlement>> {
        let row = sqlx::query_as::<_, SettlementRow>(
            "SELECT * FROM settlements WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(r.into_settlement()?)),
            None => Ok(None),
        }
    }

    async fn update_settlement(&self, settlement: &Settlement) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE settlements
            SET status = $2, phases = $3, completed_at = $4, execution_time_ms = $5
            WHERE id = $1
            "#,
        )
        .bind(&settlement.id)
        .bind(serde_json::to_string(&settlement.status)?)
        .bind(serde_json::to_value(&settlement.phases)?)
        .bind(settlement.completed_at)
        .bind(settlement.execution_time_ms.map(|v| v as i64))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_settlements(&self, limit: usize) -> DbResult<Vec<Settlement>> {
        let rows = sqlx::query_as::<_, SettlementRow>(
            "SELECT * FROM settlements ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.into_settlement()).collect()
    }

    async fn get_solvers(&self) -> DbResult<Vec<Solver>> {
        let rows = sqlx::query_as::<_, SolverRow>(
            "SELECT * FROM solvers WHERE status = 'Active'"
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.into_solver()).collect()
    }

    async fn update_solver(&self, solver: &Solver) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO solvers (id, name, solver_type, status, reputation_score, total_volume,
                                success_rate, avg_execution_time_ms, supported_chains, supported_denoms)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO UPDATE SET
                status = $4, reputation_score = $5, total_volume = $6, success_rate = $7,
                avg_execution_time_ms = $8
            "#,
        )
        .bind(&solver.id)
        .bind(&solver.name)
        .bind(serde_json::to_string(&solver.solver_type)?)
        .bind(serde_json::to_string(&solver.status)?)
        .bind(solver.reputation_score)
        .bind(solver.total_volume as i64)
        .bind(solver.success_rate)
        .bind(solver.avg_execution_time_ms as i64)
        .bind(serde_json::to_value(&solver.supported_chains)?)
        .bind(serde_json::to_value(&solver.supported_denoms)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_stats(&self) -> DbResult<SystemStats> {
        let row = sqlx::query_as::<_, StatsRow>(
            "SELECT * FROM system_stats ORDER BY updated_at DESC LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(r.into_stats()),
            None => Ok(SystemStats::default()),
        }
    }

    async fn update_stats(&self, stats: &SystemStats) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO system_stats (id, total_intents, pending_intents, active_auctions,
                                     completed_settlements, total_volume_usd, active_solvers,
                                     avg_execution_time_ms, updated_at)
            VALUES (1, $1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT (id) DO UPDATE SET
                total_intents = $1, pending_intents = $2, active_auctions = $3,
                completed_settlements = $4, total_volume_usd = $5, active_solvers = $6,
                avg_execution_time_ms = $7, updated_at = NOW()
            "#,
        )
        .bind(stats.total_intents as i64)
        .bind(stats.pending_intents as i64)
        .bind(stats.active_auctions as i64)
        .bind(stats.completed_settlements as i64)
        .bind(stats.total_volume_usd)
        .bind(stats.active_solvers as i64)
        .bind(stats.avg_execution_time_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// Row types for PostgreSQL
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct IntentRow {
    id: String,
    user_address: String,
    input_denom: String,
    input_amount: i64,
    output_denom: String,
    min_output_amount: i64,
    max_slippage_bps: i32,
    status: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[cfg(feature = "postgres")]
impl IntentRow {
    fn into_intent(self) -> DbResult<Intent> {
        Ok(Intent {
            id: self.id,
            user_address: self.user_address,
            input_denom: self.input_denom,
            input_amount: self.input_amount as u128,
            output_denom: self.output_denom,
            min_output_amount: self.min_output_amount as u128,
            max_slippage_bps: self.max_slippage_bps as u16,
            status: serde_json::from_str(&self.status)?,
            created_at: self.created_at,
            expires_at: self.expires_at,
            source_chain: "cosmoshub-4".to_string(),
            destination_chain: None,
            metadata: None,
        })
    }
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct AuctionRow {
    id: String,
    status: String,
    intent_ids: serde_json::Value,
    quotes: serde_json::Value,
    winning_quote_id: Option<String>,
    clearing_price: Option<f64>,
    created_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

#[cfg(feature = "postgres")]
impl AuctionRow {
    fn into_auction(self) -> DbResult<Auction> {
        Ok(Auction {
            id: self.id,
            status: serde_json::from_str(&self.status)?,
            intent_ids: serde_json::from_value(self.intent_ids)?,
            quotes: serde_json::from_value(self.quotes)?,
            winning_quote_id: self.winning_quote_id,
            clearing_price: self.clearing_price,
            created_at: self.created_at,
            closed_at: self.closed_at,
        })
    }
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct SettlementRow {
    id: String,
    auction_id: String,
    intent_ids: serde_json::Value,
    solver_id: String,
    status: String,
    phases: serde_json::Value,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    execution_time_ms: Option<i64>,
}

#[cfg(feature = "postgres")]
impl SettlementRow {
    fn into_settlement(self) -> DbResult<Settlement> {
        Ok(Settlement {
            id: self.id,
            auction_id: self.auction_id,
            intent_ids: serde_json::from_value(self.intent_ids)?,
            solver_id: self.solver_id,
            status: serde_json::from_str(&self.status)?,
            phases: serde_json::from_value(self.phases)?,
            created_at: self.created_at,
            completed_at: self.completed_at,
            execution_time_ms: self.execution_time_ms.map(|v| v as u64),
            tx_hashes: vec![],
            gas_used: None,
        })
    }
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct SolverRow {
    id: String,
    name: String,
    solver_type: String,
    status: String,
    reputation_score: f64,
    total_volume: i64,
    success_rate: f64,
    avg_execution_time_ms: i64,
    supported_chains: serde_json::Value,
    supported_denoms: serde_json::Value,
}

#[cfg(feature = "postgres")]
impl SolverRow {
    fn into_solver(self) -> DbResult<Solver> {
        Ok(Solver {
            id: self.id,
            name: self.name,
            solver_type: serde_json::from_str(&self.solver_type)?,
            status: serde_json::from_str(&self.status)?,
            reputation_score: self.reputation_score,
            total_volume: self.total_volume as u128,
            success_rate: self.success_rate,
            avg_execution_time_ms: self.avg_execution_time_ms as u64,
            supported_chains: serde_json::from_value(self.supported_chains)?,
            supported_denoms: serde_json::from_value(self.supported_denoms)?,
            connected_at: None,
        })
    }
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct StatsRow {
    total_intents: i64,
    pending_intents: i64,
    active_auctions: i64,
    completed_settlements: i64,
    total_volume_usd: f64,
    active_solvers: i64,
    avg_execution_time_ms: f64,
}

#[cfg(feature = "postgres")]
impl StatsRow {
    fn into_stats(self) -> SystemStats {
        SystemStats {
            total_intents: self.total_intents as u64,
            pending_intents: self.pending_intents as u64,
            active_auctions: self.active_auctions as u64,
            completed_settlements: self.completed_settlements as u64,
            total_volume_usd: self.total_volume_usd,
            active_solvers: self.active_solvers as u64,
            avg_execution_time_ms: self.avg_execution_time_ms,
        }
    }
}

/// Create storage based on configuration
pub async fn create_storage(database_url: Option<&str>) -> DbResult<Arc<dyn Storage>> {
    #[cfg(feature = "postgres")]
    if let Some(url) = database_url {
        let storage = PostgresStorage::new(url).await?;
        storage.run_migrations().await?;
        return Ok(Arc::new(storage));
    }

    Ok(Arc::new(MemoryStorage::new()))
}
