use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::upgrade::{DrainError, DrainMode, DrainModeManager, DrainStatus, InflightIntent, InflightPhase, InflightTracker};

/// HTTP server for admin upgrade and inflight controls.
pub struct AdminServer {
    addr: String,
    drain_manager: Arc<DrainModeManager>,
    inflight_tracker: Arc<InflightTracker>,
    auth_token: String,
}

impl AdminServer {
    pub fn new(
        drain_manager: Arc<DrainModeManager>,
        inflight_tracker: Arc<InflightTracker>,
        addr: String,
        auth_token: String,
    ) -> Self {
        assert!(
            !auth_token.trim().is_empty(),
            "admin auth token must be non-empty"
        );
        Self {
            addr,
            drain_manager,
            inflight_tracker,
            auth_token,
        }
    }

    pub async fn serve(self) -> Result<(), AdminServerError> {
        let state = Arc::new(AdminState {
            drain_manager: self.drain_manager,
            inflight_tracker: self.inflight_tracker,
            auth_token: self.auth_token,
        });

        let app = Router::new()
            .route("/admin/drain/start", post(drain_start))
            .route("/admin/drain/cancel", post(drain_cancel))
            .route("/admin/drain/force", post(drain_force))
            .route("/admin/drain/resume", post(drain_resume))
            .route("/admin/drain/status", get(drain_status))
            .route("/admin/inflight", get(inflight_list))
            .route("/admin/inflight/:id", get(inflight_get))
            .route("/admin/upgrade/start", post(upgrade_start))
            .route("/admin/upgrade/status", get(upgrade_status))
            .route("/admin/upgrade/abort", post(upgrade_abort))
            .layer(from_fn_with_state(state.clone(), require_admin_auth))
            .with_state(state);

        let listener = TcpListener::bind(&self.addr)
            .await
            .map_err(|e| AdminServerError::BindError(e.to_string()))?;

        tracing::info!("Admin server listening on {}", self.addr);

        axum::serve(listener, app)
            .await
            .map_err(|e| AdminServerError::ServerError(e.to_string()))?;

        Ok(())
    }
}

#[derive(Clone)]
struct AdminState {
    drain_manager: Arc<DrainModeManager>,
    inflight_tracker: Arc<InflightTracker>,
    auth_token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AdminServerError {
    #[error("failed to bind to address: {0}")]
    BindError(String),
    #[error("server error: {0}")]
    ServerError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum AdminApiError {
    #[error("drain error: {0}")]
    Drain(#[from] DrainError),
    #[error("inflight intent not found")]
    InflightNotFound,
}

impl IntoResponse for AdminApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AdminApiError::Drain(err) => match err {
                DrainError::AlreadyDraining
                | DrainError::DrainInProgress
                | DrainError::UpgradeInProgress
                | DrainError::InvalidState => (StatusCode::CONFLICT, err.to_string()),
            },
            AdminApiError::InflightNotFound => (StatusCode::NOT_FOUND, self.to_string()),
        };

        (status, message).into_response()
    }
}

/// SECURITY FIX (O-C3): Constant-time comparison to prevent timing attacks
/// on admin token authentication.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn require_admin_auth(
    State(state): State<Arc<AdminState>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("x-admin-token")
        .and_then(|value| value.to_str().ok());

    let matches = token
        .map(|t| constant_time_eq(t.as_bytes(), state.auth_token.as_bytes()))
        .unwrap_or(false);

    if !matches {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

#[derive(Debug, Deserialize)]
struct DrainStartRequest {
    reason: Option<String>,
    deadline_secs: Option<u64>,
    drain_timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct UpgradeStartRequest {
    reason: Option<String>,
    drain_timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DrainModeResponse {
    state: String,
    started_at: Option<u64>,
    deadline: Option<u64>,
    reason: Option<String>,
    completed_at: Option<u64>,
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct DrainStatusResponse {
    mode: DrainModeResponse,
    inflight_count: u64,
    oldest_inflight_age_secs: Option<u64>,
    completed_since_drain: u64,
}

impl DrainStatusResponse {
    fn from_status(status: DrainStatus) -> Self {
        let mode = match status.mode {
            DrainMode::Active => DrainModeResponse {
                state: "Active".to_string(),
                started_at: None,
                deadline: None,
                reason: None,
                completed_at: None,
                version: None,
            },
            DrainMode::Draining {
                started_at,
                deadline,
                reason,
            } => DrainModeResponse {
                state: "Draining".to_string(),
                started_at: Some(started_at),
                deadline: Some(deadline),
                reason: Some(reason),
                completed_at: None,
                version: None,
            },
            DrainMode::Drained { completed_at } => DrainModeResponse {
                state: "Drained".to_string(),
                started_at: None,
                deadline: None,
                reason: None,
                completed_at: Some(completed_at),
                version: None,
            },
            DrainMode::Upgrading { version, started_at } => DrainModeResponse {
                state: "Upgrading".to_string(),
                started_at: Some(started_at),
                deadline: None,
                reason: None,
                completed_at: None,
                version: Some(version),
            },
        };

        Self {
            mode,
            inflight_count: status.inflight_count,
            oldest_inflight_age_secs: status.oldest_inflight_age_secs,
            completed_since_drain: status.completed_since_drain,
        }
    }
}

#[derive(Debug, Serialize)]
struct InflightIntentResponse {
    id: String,
    intent_id: String,
    created_at: u64,
    phase: String,
    user_funds_locked: bool,
    solver_funds_locked: bool,
    ibc_in_flight: bool,
    user_amount: String,
    solver_id: Option<String>,
}

impl From<InflightIntent> for InflightIntentResponse {
    fn from(intent: InflightIntent) -> Self {
        Self {
            id: intent.id,
            intent_id: intent.intent_id,
            created_at: intent.created_at,
            phase: phase_to_string(&intent.phase),
            user_funds_locked: intent.user_funds_locked,
            solver_funds_locked: intent.solver_funds_locked,
            ibc_in_flight: intent.ibc_in_flight,
            user_amount: intent.user_amount.to_string(),
            solver_id: intent.solver_id,
        }
    }
}

#[derive(Debug, Serialize)]
struct InflightListResponse {
    count: usize,
    intents: Vec<InflightIntentResponse>,
}

fn phase_to_string(phase: &InflightPhase) -> String {
    match phase {
        InflightPhase::Validating => "Validating",
        InflightPhase::Matching => "Matching",
        InflightPhase::QuotingWithSolvers => "QuotingWithSolvers",
        InflightPhase::SelectingRoute => "SelectingRoute",
        InflightPhase::LockingUserFunds => "LockingUserFunds",
        InflightPhase::LockingSolverBond => "LockingSolverBond",
        InflightPhase::ExecutingIbc => "ExecutingIbc",
        InflightPhase::WaitingForAck => "WaitingForAck",
        InflightPhase::Completing => "Completing",
    }
    .to_string()
}

async fn drain_start(
    State(state): State<Arc<AdminState>>,
    Json(request): Json<DrainStartRequest>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    let reason = request
        .reason
        .unwrap_or_else(|| "maintenance".to_string());
    let deadline_secs = request
        .deadline_secs
        .or(request.drain_timeout_secs)
        .unwrap_or(1800);

    tracing::info!(
        action = "drain_start",
        reason = %reason,
        deadline_secs,
        "admin action"
    );

    state
        .drain_manager
        .start_drain(reason, deadline_secs)
        .await?;

    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn drain_cancel(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    tracing::info!(action = "drain_cancel", "admin action");
    state.drain_manager.cancel_drain().await?;
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn drain_force(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    tracing::info!(action = "drain_force", "admin action");
    state.drain_manager.force_drain().await?;
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn drain_resume(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    tracing::info!(action = "drain_resume", "admin action");
    state.drain_manager.resume().await?;
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn drain_status(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn inflight_list(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<InflightListResponse>, AdminApiError> {
    let intents = state
        .inflight_tracker
        .get_all_inflight()
        .into_iter()
        .map(InflightIntentResponse::from)
        .collect::<Vec<_>>();

    Ok(Json(InflightListResponse {
        count: intents.len(),
        intents,
    }))
}

async fn inflight_get(
    State(state): State<Arc<AdminState>>,
    Path(id): Path<String>,
) -> Result<Json<InflightIntentResponse>, AdminApiError> {
    match state.inflight_tracker.get(&id).await {
        Some(intent) => Ok(Json(InflightIntentResponse::from(intent))),
        None => Err(AdminApiError::InflightNotFound),
    }
}

async fn upgrade_start(
    State(state): State<Arc<AdminState>>,
    Json(request): Json<UpgradeStartRequest>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    let reason = request
        .reason
        .unwrap_or_else(|| "upgrade".to_string());
    let deadline_secs = request.drain_timeout_secs.unwrap_or(1800);

    tracing::info!(
        action = "upgrade_start",
        reason = %reason,
        deadline_secs,
        "admin action"
    );

    state
        .drain_manager
        .start_drain(reason, deadline_secs)
        .await?;

    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn upgrade_status(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}

async fn upgrade_abort(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<DrainStatusResponse>, AdminApiError> {
    tracing::info!(action = "upgrade_abort", "admin action");
    state.drain_manager.cancel_drain().await?;
    let status = state.drain_manager.get_status().await;
    Ok(Json(DrainStatusResponse::from_status(status)))
}
