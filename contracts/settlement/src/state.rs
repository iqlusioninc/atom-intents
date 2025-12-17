use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    /// Admin address
    pub admin: Addr,
    /// Escrow contract address
    pub escrow_contract: Addr,
    /// Minimum solver bond amount
    pub min_solver_bond: Uint128,
    /// Base slash percentage (e.g., 200 = 2%)
    pub base_slash_bps: u64,
}

#[cw_serde]
pub struct RegisteredSolver {
    pub id: String,
    pub operator: Addr,
    pub bond_amount: Uint128,
    pub active: bool,
    pub total_settlements: u64,
    pub failed_settlements: u64,
    pub registered_at: u64,
}

#[cw_serde]
pub struct Settlement {
    pub id: String,
    pub intent_id: String,
    pub solver_id: String,
    pub user: Addr,
    pub user_input_amount: Uint128,
    pub user_input_denom: String,
    pub solver_output_amount: Uint128,
    pub solver_output_denom: String,
    pub status: SettlementStatus,
    pub created_at: u64,
    pub expires_at: u64,
    pub escrow_id: Option<String>,
}

#[cw_serde]
pub enum SettlementStatus {
    Pending,
    UserLocked,
    SolverLocked,
    Executing,
    Completed,
    Failed { reason: String },
    Slashed { amount: Uint128 },
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const SOLVERS: Map<&str, RegisteredSolver> = Map::new("solvers");
pub const SETTLEMENTS: Map<&str, Settlement> = Map::new("settlements");
pub const INTENT_SETTLEMENTS: Map<&str, String> = Map::new("intent_settlements");
