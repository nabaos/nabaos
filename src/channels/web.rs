//! Axum web server for the Nyaya dashboard REST API.
//!
//! Provides authenticated REST endpoints for querying the orchestrator,
//! managing workflows and schedules, viewing costs, and security scanning.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;

use crate::agent_os::confirmation::{ConfirmationRequest, ConfirmationResponse};

/// A pending confirmation waiting for user response via the web UI.
pub(crate) struct PendingWebConfirmation {
    #[allow(dead_code)]
    request: ConfirmationRequest,
    responder: std::sync::mpsc::Sender<ConfirmationResponse>,
}

/// Thread-safe map of pending confirmations keyed by request ID.
pub(crate) type PendingConfirmations = Arc<Mutex<HashMap<u64, PendingWebConfirmation>>>;

#[derive(rust_embed::Embed)]
#[folder = "nabaos-web/dist/"]
struct WebAssets;

use crate::agent_os::triggers::TriggerEngine;
use crate::chain::workflow_engine::WorkflowEngine;
use crate::core::config::NyayaConfig;
use crate::core::error::Result;
use crate::core::orchestrator::Orchestrator;
use crate::security::two_factor::TwoFactorAuth;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A web dashboard session.
#[derive(Debug, Clone)]
pub struct WebSession {
    pub created_at: u64,
}

/// Shared application state for all handlers.
#[derive(Clone)]
pub struct AppState {
    pub orch: Arc<Mutex<Orchestrator>>,
    pub two_fa: Arc<TwoFactorAuth>,
    pub sessions: Arc<Mutex<HashMap<String, WebSession>>>,
    /// If None, auth is disabled (all requests allowed).
    pub password_hash: Option<String>,
    /// Session TTL in seconds (default 24h).
    pub session_ttl_secs: u64,
    /// Configuration (needed for constitution path).
    pub config: NyayaConfig,
    /// Telegram bot token for Mini App auth validation.
    pub bot_token: Option<String>,
    /// Workflow engine for workflow API endpoints.
    pub workflow_engine: Option<Arc<Mutex<WorkflowEngine>>>,
    /// Optional privilege guard for tiered 2FA enforcement.
    pub privilege_guard: Option<Arc<crate::security::privilege::PrivilegeGuard>>,
    /// Optional trigger engine for webhook dispatch.
    pub trigger_engine: Option<Arc<Mutex<TriggerEngine>>>,
    /// In-memory rate limiter: IP/key -> (window_start, request_count).
    pub rate_limits: Arc<Mutex<HashMap<String, (std::time::Instant, u32)>>>,
    /// Pending OAuth flows keyed by state parameter.
    pub(crate) oauth_pending: Arc<Mutex<HashMap<String, PendingOAuthFlow>>>,
    /// Pending confirmation requests waiting for user response.
    pub(crate) pending_confirmations: PendingConfirmations,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OAuthCallbackParams {
    pub code: String,
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// Tracks a pending OAuth flow awaiting callback.
#[derive(Debug, Clone)]
pub(crate) struct PendingOAuthFlow {
    provider: crate::modules::oauth::token_manager::OAuthProvider,
    code_verifier: String,
    created_at: u64,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub auth_required: bool,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
}

/// Request body for `POST /api/v1/confirm/{id}`.
#[derive(Deserialize)]
struct ConfirmRequest {
    /// One of: "allow_once", "allow_session", "allow_always", "deny"
    response: String,
}

/// Response for `POST /api/v1/confirm/{id}`.
#[derive(Serialize)]
struct PermissionConfirmResponse {
    ok: bool,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub tier: String,
    pub intent_key: String,
    pub confidence: f64,
    pub allowed: bool,
    pub latency_ms: f64,
    pub description: String,
    pub response_text: Option<String>,
    pub nyaya_mode: Option<String>,
    pub security: SecurityInfo,
}

#[derive(Serialize)]
pub struct SecurityInfo {
    pub credentials_found: usize,
    pub injection_detected: bool,
    pub injection_confidence: f32,
    pub was_redacted: bool,
}

#[derive(Serialize)]
pub struct DashboardResponse {
    pub total_workflows: usize,
    pub total_scheduled_jobs: usize,
    pub total_abilities: usize,
    pub costs: CostInfo,
}

#[derive(Serialize)]
pub struct CostInfo {
    pub total_spent_usd: f64,
    pub total_saved_usd: f64,
    pub savings_percent: f64,
    pub total_llm_calls: u64,
    pub total_cache_hits: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

#[derive(Serialize)]
pub struct ChainInfo {
    #[serde(rename = "workflow_id")]
    pub chain_id: String,
    pub name: String,
    pub description: String,
    pub trust_level: u32,
    pub hit_count: u64,
    pub success_count: u64,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ScheduledJobInfo {
    pub id: String,
    pub chain_id: String,
    pub interval_secs: u64,
    pub enabled: bool,
    pub last_run_at: Option<i64>,
    pub last_output: Option<String>,
    pub run_count: u64,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct ScheduleRequest {
    pub chain_id: String,
    pub interval: String,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct ScheduleResponse {
    pub job_id: String,
}

#[derive(Deserialize)]
pub struct CostQuery {
    pub since: Option<i64>,
}

#[derive(Deserialize)]
pub struct ScanRequest {
    pub text: String,
}

#[derive(Serialize)]
pub struct ScanResponse {
    pub credential_count: usize,
    pub pii_count: usize,
    pub types_found: Vec<String>,
    pub injection_detected: bool,
    pub injection_match_count: usize,
    pub injection_max_confidence: f32,
    pub injection_category: Option<String>,
    pub redacted: String,
}

#[derive(Serialize)]
pub struct AbilityInfo {
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct ConstitutionInfo {
    pub name: String,
    pub rules: Vec<RuleInfo>,
}

#[derive(Serialize)]
pub struct RuleInfo {
    pub name: String,
    pub description: Option<String>,
    pub enforcement: String,
    pub trigger_actions: Vec<String>,
    pub trigger_targets: Vec<String>,
    pub trigger_keywords: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ConfirmResponse {
    pub confirmed: bool,
}

#[derive(Deserialize)]
pub struct StartWorkflowRequest {
    pub workflow_id: String,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct StartWorkflowResponse {
    pub instance_id: String,
}

#[derive(Serialize)]
pub struct WorkflowDefInfo {
    pub id: String,
    pub name: String,
}

/// Combined response for `/api/v1/workflows` — merges chain listing with
/// workflow definitions into a single JSON payload.
#[derive(Serialize)]
pub struct CombinedWorkflowsResponse {
    /// Compiled workflow definitions (legacy "chains").
    pub compiled: Vec<ChainInfo>,
    /// Workflow definitions from the workflow engine.
    pub workflows: Vec<WorkflowDefInfo>,
}

#[derive(Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub payload: String,
}

#[derive(Deserialize)]
pub struct MetaSuggestRequest {
    pub requirement: String,
}

#[derive(Serialize)]
pub struct MetaSuggestResponse {
    pub workflow: serde_json::Value,
    pub yaml: String,
}

#[derive(Deserialize)]
pub struct MetaCreateRequest {
    pub requirement: String,
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct MetaCreateResponse {
    pub workflow_id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct MetaTemplateInfo {
    pub id: String,
    pub name: String,
    pub category: String,
}

#[derive(Deserialize)]
pub struct SkillForgeRequest {
    pub tier: String,
    pub requirement: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct SkillForgeResponse {
    pub name: String,
    pub tier: String,
    pub workflow_id: Option<String>,
    pub wasm_path: Option<String>,
    pub script_path: Option<String>,
    pub script_content: Option<String>,
}

#[derive(Serialize)]
pub struct SkillListItem {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct SetStyleRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct TelegramAuthRequest {
    pub init_data: String,
}

#[derive(Serialize)]
pub struct TelegramAuthResponse {
    pub token: String,
    pub user_id: i64,
}

#[derive(Deserialize)]
pub struct ElevateRequest {
    pub session_token: String,
    pub totp_code: String,
    pub password: Option<String>,
}

#[derive(Serialize)]
pub struct ElevateResponse {
    pub success: bool,
    pub level: String,
    pub ttl_secs: u64,
}

#[derive(Deserialize)]
pub struct SetPermissionRequest {
    pub channel: String,
    pub access: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub domains: Option<Vec<String>>,
    pub send_domains: Option<Vec<String>>,
    pub servers: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct PermissionListResponse {
    pub permissions: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Channel permission override store (SQLite)
// ---------------------------------------------------------------------------

/// Ensure the channel_permission_overrides table exists.
fn ensure_permission_overrides_table(db_path: &std::path::Path) {
    if let Ok(conn) = rusqlite::Connection::open(db_path) {
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_permission_overrides (
                channel TEXT PRIMARY KEY,
                access_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        );
    }
}

/// Load all runtime permission overrides from SQLite.
fn load_permission_overrides(
    db_path: &std::path::Path,
) -> HashMap<String, crate::security::channel_permissions::ChannelAccess> {
    let mut overrides = HashMap::new();
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return overrides,
    };
    let mut stmt =
        match conn.prepare("SELECT channel, access_json FROM channel_permission_overrides") {
            Ok(s) => s,
            Err(_) => return overrides,
        };
    let rows = stmt.query_map([], |row| {
        let channel: String = row.get(0)?;
        let json: String = row.get(1)?;
        Ok((channel, json))
    });
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            if let Ok(access) =
                serde_json::from_str::<crate::security::channel_permissions::ChannelAccess>(&row.1)
            {
                overrides.insert(row.0, access);
            }
        }
    }
    overrides
}

/// Save a runtime permission override for a channel.
fn save_permission_override(
    db_path: &std::path::Path,
    channel: &str,
    access: &crate::security::channel_permissions::ChannelAccess,
) -> std::result::Result<(), String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| format!("DB open: {}", e))?;
    ensure_permission_overrides_table(db_path);
    let json = serde_json::to_string(access).map_err(|e| format!("serialize: {}", e))?;
    let now = now_secs() as i64;
    conn.execute(
        "INSERT OR REPLACE INTO channel_permission_overrides (channel, access_json, updated_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![channel, json, now],
    ).map_err(|e| format!("DB insert: {}", e))?;
    Ok(())
}

/// Delete a runtime permission override for a channel.
fn delete_permission_override(
    db_path: &std::path::Path,
    channel: &str,
) -> std::result::Result<bool, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| format!("DB open: {}", e))?;
    ensure_permission_overrides_table(db_path);
    let count = conn
        .execute(
            "DELETE FROM channel_permission_overrides WHERE channel = ?1",
            rusqlite::params![channel],
        )
        .map_err(|e| format!("DB delete: {}", e))?;
    Ok(count > 0)
}

/// Get the DB path for permission overrides.
fn permissions_db_path(config: &NyayaConfig) -> std::path::PathBuf {
    config.data_dir.join("permissions_overrides.db")
}

/// Build effective permissions by merging constitution + runtime overrides.
fn build_effective_permissions(
    config: &NyayaConfig,
    constitution_perms: Option<&crate::security::channel_permissions::ChannelPermissions>,
) -> crate::security::channel_permissions::ChannelPermissions {
    let db_path = permissions_db_path(config);
    ensure_permission_overrides_table(&db_path);
    let overrides = load_permission_overrides(&db_path);

    let mut effective = constitution_perms.cloned().unwrap_or_default();

    // Merge overrides on top of constitution, but cap at constitution ceiling.
    // The constitution ceiling can never be exceeded by runtime overrides.
    for (channel, mut override_access) in overrides {
        if let Some(constitution) = constitution_perms {
            if let Some(ceiling) = constitution.channels.get(&channel) {
                // Cap access level: override cannot exceed constitution ceiling
                if override_access.access.rank() > ceiling.access.rank() {
                    override_access.access = ceiling.access.clone();
                }
                // Preserve exclude entries from constitution — override can't remove them
                for entry in &ceiling.contacts {
                    if entry.excluded
                        && !override_access
                            .contacts
                            .iter()
                            .any(|e| e.excluded && e.pattern == entry.pattern)
                    {
                        override_access.contacts.push(entry.clone());
                    }
                }
                for entry in &ceiling.groups {
                    if entry.excluded
                        && !override_access
                            .groups
                            .iter()
                            .any(|e| e.excluded && e.pattern == entry.pattern)
                    {
                        override_access.groups.push(entry.clone());
                    }
                }
                for entry in &ceiling.domains {
                    if entry.excluded
                        && !override_access
                            .domains
                            .iter()
                            .any(|e| e.excluded && e.pattern == entry.pattern)
                    {
                        override_access.domains.push(entry.clone());
                    }
                }
            } else {
                // Constitution has no entry for this channel — check default_access as ceiling
                if override_access.access.rank() > constitution.default_access.rank() {
                    override_access.access = constitution.default_access.clone();
                }
            }
        }
        effective.channels.insert(channel, override_access);
    }

    effective
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[allow(clippy::result_large_err)]
fn lock_or_500<T>(m: &Mutex<T>) -> std::result::Result<std::sync::MutexGuard<'_, T>, Response> {
    m.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal lock error".to_string(),
            }),
        )
            .into_response()
    })
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorResponse { error: msg.into() })).into_response()
}

/// Extract Bearer token from Authorization header.
fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// Check rate limit for a given client key. Returns `true` if the request is allowed.
fn check_rate_limit(
    state: &AppState,
    client_ip: &str,
    max_requests: u32,
    window_secs: u64,
) -> bool {
    let mut limits = state.rate_limits.lock().unwrap_or_else(|p| p.into_inner());
    let now = std::time::Instant::now();
    let entry = limits.entry(client_ip.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0).as_secs() > window_secs {
        *entry = (now, 1);
        true
    } else {
        entry.1 += 1;
        entry.1 <= max_requests
    }
}

/// Require authentication. Returns Ok(()) if auth is disabled or session is valid.
#[allow(clippy::result_large_err)]
fn require_auth(state: &AppState, headers: &HeaderMap) -> std::result::Result<(), Response> {
    // If no password hash configured, auth is disabled
    if state.password_hash.is_none() {
        return Ok(());
    }

    let token = extract_bearer(headers)
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

    let sessions = lock_or_500(&state.sessions)?;
    let session = sessions
        .get(&token)
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Invalid or expired session token"))?;

    let elapsed = now_secs().saturating_sub(session.created_at);
    if elapsed >= state.session_ttl_secs {
        drop(sessions);
        // Clean up expired session
        if let Ok(mut s) = state.sessions.lock() {
            s.remove(&token);
        }
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "Session expired, please login again",
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
    if !check_rate_limit(&state, "login", 10, 60) {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Try again later.",
        );
    }

    let Some(ref hash) = state.password_hash else {
        // Auth disabled — return a dummy token
        let token = uuid::Uuid::new_v4().to_string();
        return (StatusCode::OK, Json(LoginResponse { token })).into_response();
    };

    // Verify password against stored argon2 hash
    use argon2::PasswordVerifier;
    let parsed_hash = match argon2::PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid password hash configuration",
            );
        }
    };
    let argon2 = argon2::Argon2::default();
    if argon2
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return json_error(StatusCode::UNAUTHORIZED, "Invalid password");
    }

    let token = uuid::Uuid::new_v4().to_string();
    match state.sessions.lock() {
        Ok(mut sessions) => {
            sessions.insert(
                token.clone(),
                WebSession {
                    created_at: now_secs(),
                },
            );
        }
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Session store error");
        }
    }

    (StatusCode::OK, Json(LoginResponse { token })).into_response()
}

async fn handle_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = extract_bearer(&headers) {
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.remove(&token);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn handle_auth_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth_required = state.password_hash.is_some();
    if !auth_required {
        return (
            StatusCode::OK,
            Json(AuthStatusResponse {
                authenticated: true,
                auth_required: false,
            }),
        )
            .into_response();
    }

    let authenticated = if let Some(token) = extract_bearer(&headers) {
        if let Ok(sessions) = state.sessions.lock() {
            sessions
                .get(&token)
                .map(|s| {
                    let elapsed = now_secs().saturating_sub(s.created_at);
                    elapsed < state.session_ttl_secs
                })
                .unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };

    (
        StatusCode::OK,
        Json(AuthStatusResponse {
            authenticated,
            auth_required,
        }),
    )
        .into_response()
}

async fn handle_dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    let chains_count = orch.chain_store().list(1000).map(|c| c.len()).unwrap_or(0);
    let jobs_count = orch.scheduler().list().map(|j| j.len()).unwrap_or(0);
    let abilities_count = orch.ability_registry().list_all_abilities().len();
    let cost_summary = orch.cost_summary(None);

    let costs = match cost_summary {
        Ok(s) => CostInfo {
            total_spent_usd: s.total_spent_usd,
            total_saved_usd: s.total_saved_usd,
            savings_percent: s.savings_percent,
            total_llm_calls: s.total_llm_calls,
            total_cache_hits: s.total_cache_hits,
            total_input_tokens: s.total_input_tokens,
            total_output_tokens: s.total_output_tokens,
        },
        Err(_) => CostInfo {
            total_spent_usd: 0.0,
            total_saved_usd: 0.0,
            savings_percent: 0.0,
            total_llm_calls: 0,
            total_cache_hits: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        },
    };

    (
        StatusCode::OK,
        Json(DashboardResponse {
            total_workflows: chains_count,
            total_scheduled_jobs: jobs_count,
            total_abilities: abilities_count,
            costs,
        }),
    )
        .into_response()
}

async fn handle_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<QueryRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let mut orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    // Parse @agent-name prefix for inline agent routing
    let (target_agent, clean_query) = parse_agent_mention(&body.query);
    let saved_agent = if let Some(ref agent) = target_agent {
        let prev = orch.active_agent().to_string();
        orch.set_active_agent(agent);
        Some(prev)
    } else {
        None
    };

    let ctx = crate::core::orchestrator::ChannelContext {
        channel: "web".into(),
        user_id: headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
        ..Default::default()
    };
    let query_result = orch.process_query(&clean_query, Some(&ctx));

    // Restore previous agent
    if let Some(prev) = saved_agent {
        orch.set_active_agent(&prev);
    }

    match query_result {
        Ok(result) => {
            let resp = QueryResponse {
                tier: format!("{}", result.tier),
                intent_key: result.intent_key,
                confidence: result.confidence,
                allowed: result.allowed,
                latency_ms: result.latency_ms,
                description: result.description,
                response_text: result.response_text,
                nyaya_mode: result.nyaya_mode,
                security: SecurityInfo {
                    credentials_found: result.security.credentials_found,
                    injection_detected: result.security.injection_detected,
                    injection_confidence: result.security.injection_confidence,
                    was_redacted: result.security.was_redacted,
                },
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

/// SSE streaming endpoint for `/api/v1/ask/stream`.
///
/// Streams LLM responses as Server-Sent Events:
/// - `tier`  — tier name and confidence after classification
/// - `delta` — response text chunk(s)
/// - `done`  — full metadata JSON (latency, confidence, security, etc.)
/// - `confirm_required` — interactive confirmation needed (user must POST /api/v1/confirm/{id})
/// - `error` — on failure
async fn handle_query_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<QueryRequest>,
) -> std::result::Result<
    Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>,
    Response,
> {
    require_auth(&state, &headers)?;

    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<Event, Infallible>>(32);

    let orch = state.orch.clone();
    let query = body.query.clone();
    let user_id = headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Shared pending confirmations map for the confirm_fn closure
    let pending = state.pending_confirmations.clone();

    // SSE sender for confirm_required events (needs to be cloned into closure)
    let confirm_tx = tx.clone();

    tokio::spawn(async move {
        // Run the blocking orchestrator call on a blocking thread to avoid
        // holding MutexGuard across await points.
        let result = tokio::task::spawn_blocking(move || {
            let mut guard = match orch.lock() {
                Ok(g) => g,
                Err(_) => return Err("Internal lock error".to_string()),
            };

            // Parse @agent-name prefix for inline agent routing
            let (target_agent, clean_query) = parse_agent_mention(&query);
            let saved_agent = if let Some(ref agent) = target_agent {
                let prev = guard.active_agent().to_string();
                guard.set_active_agent(agent);
                Some(prev)
            } else {
                None
            };

            // Build a web-compatible ConfirmFn that sends SSE events and waits
            // for the user to POST /api/v1/confirm/{id}.
            let confirm_fn: crate::agent_os::confirmation::ConfirmFn = {
                let pending = pending.clone();
                let confirm_tx = confirm_tx.clone();
                Box::new(move |request: ConfirmationRequest| {
                    let req_id = request.id;
                    let (resp_tx, resp_rx) = std::sync::mpsc::channel::<ConfirmationResponse>();

                    // Send confirm_required SSE event to the browser
                    let confirm_json = serde_json::json!({
                        "id": req_id,
                        "agent_id": request.agent_id,
                        "ability": request.ability,
                        "reason": request.reason,
                        "options": [
                            {"value": "allow_once", "label": "Allow once"},
                            {"value": "allow_session", "label": "Allow for this session"},
                            {"value": "allow_always", "label": "Always allow for this agent"},
                            {"value": "deny", "label": "Deny"},
                        ],
                    });
                    let event = Event::default()
                        .event("confirm_required")
                        .data(confirm_json.to_string());
                    // Use a blocking send on the tokio channel
                    let _ = confirm_tx.blocking_send(Ok(event));

                    // Store in pending map so POST /api/v1/confirm/{id} can resolve it
                    if let Ok(mut map) = pending.lock() {
                        map.insert(req_id, PendingWebConfirmation {
                            request,
                            responder: resp_tx,
                        });
                    }

                    // Block until the user responds (up to 120s timeout)
                    let result = resp_rx
                        .recv_timeout(std::time::Duration::from_secs(120))
                        .ok();

                    // Clean up pending map
                    if let Ok(mut map) = pending.lock() {
                        map.remove(&req_id);
                    }

                    result
                })
            };

            // Set the confirm_fn on the orchestrator for this request
            guard.confirm_fn = Some(confirm_fn);

            let ctx = crate::core::orchestrator::ChannelContext {
                channel: "web".into(),
                user_id,
                ..Default::default()
            };
            let result = guard
                .process_query(&clean_query, Some(&ctx))
                .map_err(|e| format!("{}", e));

            // Clear confirm_fn and restore agent
            guard.confirm_fn = None;
            if let Some(prev) = saved_agent {
                guard.set_active_agent(&prev);
            }

            result
        })
        .await;

        // Flatten the JoinError + inner Result
        let result = match result {
            Ok(inner) => inner,
            Err(e) => Err(format!("Task join error: {}", e)),
        };

        match result {
            Ok(r) => {
                // Send tier event
                let tier_json = serde_json::json!({
                    "tier": format!("{}", r.tier),
                    "confidence": r.confidence,
                });
                let _ = tx
                    .send(Ok(Event::default()
                        .event("tier")
                        .data(tier_json.to_string())))
                    .await;

                // Send delta event with the response text
                let text = r
                    .response_text
                    .clone()
                    .unwrap_or_else(|| r.description.clone());
                let _ = tx
                    .send(Ok(Event::default().event("delta").data(text)))
                    .await;

                // Send done event with full metadata
                let done_json = serde_json::json!({
                    "tier": format!("{}", r.tier),
                    "intent_key": r.intent_key,
                    "confidence": r.confidence,
                    "allowed": r.allowed,
                    "latency_ms": r.latency_ms,
                    "description": r.description,
                    "response_text": r.response_text,
                    "nyaya_mode": r.nyaya_mode,
                    "security": {
                        "credentials_found": r.security.credentials_found,
                        "injection_detected": r.security.injection_detected,
                        "injection_confidence": r.security.injection_confidence,
                        "was_redacted": r.security.was_redacted,
                    }
                });
                let _ = tx
                    .send(Ok(Event::default()
                        .event("done")
                        .data(done_json.to_string())))
                    .await;
            }
            Err(e) => {
                let _ = tx.send(Ok(Event::default().event("error").data(&e))).await;
            }
        }
    });

    let stream = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
    ))
}

/// Parse `@agent-name query text` into (Some("agent-name"), "query text").
/// Returns (None, original) if no @-mention is found.
fn parse_agent_mention(query: &str) -> (Option<String>, String) {
    let trimmed = query.trim();
    if !trimmed.starts_with('@') {
        return (None, query.to_string());
    }
    if let Some(space_pos) = trimmed.find(|c: char| c.is_whitespace()) {
        let agent = trimmed[1..space_pos].to_string();
        let rest = trimmed[space_pos..].trim().to_string();
        if agent.is_empty() {
            (None, query.to_string())
        } else {
            (Some(agent), rest)
        }
    } else {
        // "@agent" with no query text
        let agent = trimmed[1..].to_string();
        if agent.is_empty() {
            (None, query.to_string())
        } else {
            (Some(agent), String::new())
        }
    }
}

/// Handle user confirmation response for a pending confirmation request.
///
/// The browser calls this after receiving a `confirm_required` SSE event.
async fn handle_confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(body): Json<ConfirmRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let response = match body.response.as_str() {
        "allow_once" => ConfirmationResponse::AllowOnce,
        "allow_session" => ConfirmationResponse::AllowSession,
        "allow_always" => ConfirmationResponse::AllowAlwaysAgent,
        "deny" => ConfirmationResponse::Deny,
        _ => return json_error(StatusCode::BAD_REQUEST, String::from("Invalid response value")),
    };

    let responder = {
        let mut map = match state.pending_confirmations.lock() {
            Ok(m) => m,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, String::from("Lock error")),
        };
        map.remove(&id).map(|p| p.responder)
    };

    match responder {
        Some(tx) => {
            let _ = tx.send(response);
            (StatusCode::OK, Json(PermissionConfirmResponse { ok: true })).into_response()
        }
        None => json_error(StatusCode::NOT_FOUND, format!("No pending confirmation with id {}", id)),
    }
}

#[allow(dead_code)] // Superseded by handle_list_workflows_combined
async fn handle_chains(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.chain_store().list(1000) {
        Ok(chains) => {
            let infos: Vec<ChainInfo> = chains
                .into_iter()
                .map(|c| ChainInfo {
                    chain_id: c.chain_id,
                    name: c.name,
                    description: c.description,
                    trust_level: c.trust_level,
                    hit_count: c.hit_count,
                    success_count: c.success_count,
                    created_at: c.created_at,
                })
                .collect();
            (StatusCode::OK, Json(infos)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

/// Combined handler for `/api/v1/workflows` — returns both compiled chains
/// and workflow definitions in a single JSON response.
async fn handle_list_workflows_combined(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    // Fetch compiled chains
    let compiled = {
        let orch = match lock_or_500(&state.orch) {
            Ok(o) => o,
            Err(e) => return e,
        };
        match orch.chain_store().list(1000) {
            Ok(chains) => chains
                .into_iter()
                .map(|c| ChainInfo {
                    chain_id: c.chain_id,
                    name: c.name,
                    description: c.description,
                    trust_level: c.trust_level,
                    hit_count: c.hit_count,
                    success_count: c.success_count,
                    created_at: c.created_at,
                })
                .collect(),
            Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
        }
    };

    // Fetch workflow definitions (empty list if engine not available)
    let workflows = match &state.workflow_engine {
        Some(engine_arc) => {
            let engine = match lock_or_500(engine_arc) {
                Ok(e) => e,
                Err(e) => return e,
            };
            match engine.store().list_defs() {
                Ok(defs) => defs
                    .into_iter()
                    .map(|(id, name)| WorkflowDefInfo { id, name })
                    .collect(),
                Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
            }
        }
        None => Vec::new(),
    };

    (
        StatusCode::OK,
        Json(CombinedWorkflowsResponse {
            compiled,
            workflows,
        }),
    )
        .into_response()
}

async fn handle_list_schedule(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.scheduler().list() {
        Ok(jobs) => {
            let infos: Vec<ScheduledJobInfo> = jobs
                .into_iter()
                .map(|j| ScheduledJobInfo {
                    id: j.id,
                    chain_id: j.chain_id,
                    interval_secs: j.interval_secs,
                    enabled: j.enabled,
                    last_run_at: j.last_run_at,
                    last_output: j.last_output,
                    run_count: j.run_count,
                    created_at: j.created_at,
                })
                .collect();
            (StatusCode::OK, Json(infos)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_create_schedule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ScheduleRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let interval_secs = match crate::chain::scheduler::parse_interval(&body.interval) {
        Ok(s) => s,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, format!("{}", e)),
    };

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    let spec = crate::chain::scheduler::ScheduleSpec::Interval(interval_secs);
    match orch.schedule_chain(&body.chain_id, spec, &body.params) {
        Ok(job_id) => (StatusCode::CREATED, Json(ScheduleResponse { job_id })).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_disable_schedule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.scheduler().disable(&job_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_costs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CostQuery>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.cost_summary(q.since) {
        Ok(s) => {
            let info = CostInfo {
                total_spent_usd: s.total_spent_usd,
                total_saved_usd: s.total_saved_usd,
                savings_percent: s.savings_percent,
                total_llm_calls: s.total_llm_calls,
                total_cache_hits: s.total_cache_hits,
                total_input_tokens: s.total_input_tokens,
                total_output_tokens: s.total_output_tokens,
            };
            (StatusCode::OK, Json(info)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_costs_dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.cost_dashboard() {
        Ok(d) => {
            let json = serde_json::json!({
                "daily": {
                    "total_cost": d.daily.total_spent_usd,
                    "total_calls": d.daily.total_llm_calls,
                    "cache_hit_rate": d.daily_cache_hit_rate,
                    "cache_hits": d.daily.total_cache_hits,
                    "total_saved": d.daily.total_saved_usd,
                },
                "weekly": {
                    "total_cost": d.weekly.total_spent_usd,
                    "total_calls": d.weekly.total_llm_calls,
                    "cache_hits": d.weekly.total_cache_hits,
                    "total_saved": d.weekly.total_saved_usd,
                },
                "monthly": {
                    "total_cost": d.monthly.total_spent_usd,
                    "total_calls": d.monthly.total_llm_calls,
                    "cache_hits": d.monthly.total_cache_hits,
                    "total_saved": d.monthly.total_saved_usd,
                    "estimated_savings": d.monthly.total_saved_usd,
                },
            });
            (StatusCode::OK, Json(json)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_security_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ScanRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    use crate::security::{credential_scanner, pattern_matcher};

    let cred_summary = credential_scanner::scan_summary(&body.text);
    let injection = pattern_matcher::assess(&body.text);
    let redacted = credential_scanner::redact_all(&body.text);

    let resp = ScanResponse {
        credential_count: cred_summary.credential_count,
        pii_count: cred_summary.pii_count,
        types_found: cred_summary.types_found,
        injection_detected: injection.likely_injection,
        injection_match_count: injection.match_count,
        injection_max_confidence: injection.max_confidence,
        injection_category: injection.top_category.map(|c| c.to_string()),
        redacted: redacted.redacted,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

async fn handle_abilities(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    let all = orch.ability_registry().list_all_abilities();
    let infos: Vec<AbilityInfo> = all
        .into_iter()
        .map(|(name, desc, source)| AbilityInfo {
            name,
            description: desc,
            source: format!("{}", source),
        })
        .collect();
    (StatusCode::OK, Json(infos)).into_response()
}

async fn handle_constitution(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    // Load constitution info from config
    let enforcer_name = {
        let enforcer = if let Some(ref path) = state.config.constitution_path {
            match crate::security::constitution::ConstitutionEnforcer::load(path) {
                Ok(e) => e,
                Err(err) => {
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load constitution: {}", err),
                    );
                }
            }
        } else {
            crate::security::constitution::ConstitutionEnforcer::from_constitution(
                crate::security::constitution::default_constitution(),
            )
        };

        let rules: Vec<RuleInfo> = enforcer
            .rules()
            .iter()
            .map(|r| RuleInfo {
                name: r.name.clone(),
                description: r.description.clone(),
                enforcement: format!("{:?}", r.enforcement),
                trigger_actions: r.trigger_actions.clone(),
                trigger_targets: r.trigger_targets.clone(),
                trigger_keywords: r.trigger_keywords.clone(),
                reason: r.reason.clone(),
            })
            .collect();

        ConstitutionInfo {
            name: enforcer.name().to_string(),
            rules,
        }
    };

    let templates = [
        "default",
        "solopreneur",
        "freelancer",
        "digital-marketer",
        "student",
        "sales",
        "customer-support",
        "legal",
        "ecommerce",
        "hr",
        "finance",
        "healthcare",
        "engineering",
        "media",
        "government",
        "ngo",
        "logistics",
        "research",
        "consulting",
        "creative",
        "agriculture",
    ];
    let template_list: Vec<serde_json::Value> = templates
        .iter()
        .filter_map(|name| {
            let c = crate::security::constitution::get_constitution_template(name)?;
            Some(serde_json::json!({
                "name": name,
                "description": c.description,
                "rules_count": c.rules.len(),
            }))
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "name": enforcer_name.name,
            "rules": enforcer_name.rules,
            "templates": template_list,
        })),
    )
        .into_response()
}

async fn handle_confirm_weblink(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Response {
    let confirmed = state.two_fa.confirm_weblink(&token);
    (StatusCode::OK, Json(ConfirmResponse { confirmed })).into_response()
}

/// Elevate a session's privilege level via TOTP (and optionally password).
///
/// - TOTP only: elevates to `Elevated` (Level 1)
/// - TOTP + password: elevates to `Admin` (Level 2)
async fn handle_auth_elevate(
    State(state): State<AppState>,
    Json(body): Json<ElevateRequest>,
) -> Response {
    use crate::security::privilege::PrivilegeLevel;

    let guard = match &state.privilege_guard {
        Some(g) => g,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Privilege guard not configured",
            );
        }
    };

    // Verify the session token exists
    {
        let sessions = match lock_or_500(&state.sessions) {
            Ok(s) => s,
            Err(r) => return r,
        };
        if !sessions.contains_key(&body.session_token) {
            return json_error(StatusCode::UNAUTHORIZED, "Invalid session token");
        }
    }

    // Verify TOTP code (use session hash as chat_id since we only need code verification)
    let totp_chat_id = body.session_token.len() as i64;
    if !state.two_fa.verify_totp(totp_chat_id, &body.totp_code) {
        return json_error(StatusCode::UNAUTHORIZED, "Invalid TOTP code");
    }

    // Determine level based on what was provided
    let (level, ttl) = if body.password.is_some() {
        (PrivilegeLevel::Admin, 900u64) // 15 minutes
    } else {
        (PrivilegeLevel::Elevated, 3600u64) // 1 hour
    };

    // Elevate the session
    guard.elevate(&body.session_token, level);

    (
        StatusCode::OK,
        Json(ElevateResponse {
            success: true,
            level: level.to_string(),
            ttl_secs: ttl,
        }),
    )
        .into_response()
}

/// Hex-encode a byte slice.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Authenticate a Telegram Mini App user via init_data HMAC validation.
async fn handle_telegram_auth(
    State(state): State<AppState>,
    Json(body): Json<TelegramAuthRequest>,
) -> Response {
    let bot_token = match &state.bot_token {
        Some(t) => t,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Telegram integration not configured",
            );
        }
    };

    // Parse init_data (URL-encoded key=value pairs)
    let params: Vec<(String, String)> = url::form_urlencoded::parse(body.init_data.as_bytes())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let hash = params
        .iter()
        .find(|(k, _)| k == "hash")
        .map(|(_, v)| v.clone());
    let hash = match hash {
        Some(h) => h,
        None => return json_error(StatusCode::BAD_REQUEST, "Missing hash"),
    };

    // Build data check string: all params except hash, sorted by key, joined by \n
    let mut check_params: Vec<&(String, String)> =
        params.iter().filter(|(k, _)| k != "hash").collect();
    check_params.sort_by(|a, b| a.0.cmp(&b.0));
    let data_check_string: String = check_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    // HMAC validation per Telegram spec:
    // secret_key = HMAC-SHA256("WebAppData", bot_token)
    // expected   = HMAC-SHA256(secret_key, data_check_string)
    use ring::hmac;
    let secret_key = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, b"WebAppData"),
        bot_token.as_bytes(),
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_ref());
    let expected = hmac::sign(&key, data_check_string.as_bytes());
    let expected_hex = to_hex(expected.as_ref());

    if expected_hex != hash {
        return json_error(StatusCode::UNAUTHORIZED, "Invalid Telegram auth");
    }

    // Extract user ID from the "user" JSON field
    let user_json = params
        .iter()
        .find(|(k, _)| k == "user")
        .map(|(_, v)| v.clone());
    let user_id: i64 = match user_json {
        Some(json) => match serde_json::from_str::<serde_json::Value>(&json) {
            Ok(v) => v["id"].as_i64().unwrap_or(0),
            Err(_) => return json_error(StatusCode::BAD_REQUEST, "Invalid user data"),
        },
        None => return json_error(StatusCode::BAD_REQUEST, "Missing user data"),
    };

    // Create a session
    let token = uuid::Uuid::new_v4().to_string();
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(
            token.clone(),
            WebSession {
                created_at: now_secs(),
            },
        );
    }

    (
        StatusCode::OK,
        Json(TelegramAuthResponse { token, user_id }),
    )
        .into_response()
}

// --- Workflow API ---

#[allow(dead_code)] // Superseded by handle_list_workflows_combined
async fn handle_list_workflows(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match engine.store().list_defs() {
        Ok(defs) => {
            let infos: Vec<WorkflowDefInfo> = defs
                .into_iter()
                .map(|(id, name)| WorkflowDefInfo { id, name })
                .collect();
            (StatusCode::OK, Json(infos)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_start_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StartWorkflowRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match engine.start(&body.workflow_id, body.params) {
        Ok(instance_id) => (
            StatusCode::CREATED,
            Json(StartWorkflowResponse { instance_id }),
        )
            .into_response(),
        Err(e) => json_error(StatusCode::BAD_REQUEST, e),
    }
}

async fn handle_workflow_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match engine.status(&instance_id) {
        Ok(Some(inst)) => (StatusCode::OK, Json(inst)).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "Instance not found"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn handle_cancel_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match engine.cancel(&instance_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "cancelled": true })),
        )
            .into_response(),
        Err(e) => json_error(StatusCode::BAD_REQUEST, e),
    }
}

async fn handle_hook_incoming(
    State(state): State<AppState>,
    Path((workflow_id, correlation_value)): Path<(String, String)>,
    Json(body): Json<HookPayload>,
) -> Response {
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };

    // Get ability registry and manifest from the orchestrator
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let ability_registry = orch.ability_registry();
    let manifest = crate::runtime::manifest::AgentManifest::workflow_manifest();

    match engine.deliver_event(
        &workflow_id,
        &correlation_value,
        &body.event_type,
        &body.payload,
        ability_registry,
        &manifest,
        None,
        None,
    ) {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "delivered": true,
                "status": format!("{}", status),
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "delivered": false,
                "reason": "No matching waiting instance found",
            })),
        )
            .into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn handle_workflow_visualize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workflow_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let engine_arc = match &state.workflow_engine {
        Some(e) => e,
        None => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Workflow engine not available",
            )
        }
    };
    let engine = match lock_or_500(engine_arc) {
        Ok(e) => e,
        Err(e) => return e,
    };

    // Look up the workflow definition
    let def = match engine.store().get_def(&workflow_id) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return json_error(
                StatusCode::NOT_FOUND,
                format!("Workflow '{}' not found", workflow_id),
            )
        }
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    };

    // Optionally look up an instance for status coloring
    let instance_id = params.get("instance");
    let inst = if let Some(iid) = instance_id {
        match engine.status(iid) {
            Ok(i) => i,
            Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    } else {
        None
    };

    // Generate diagram based on ?format= query param (default: mermaid)
    let format = params
        .get("format")
        .map(|s| s.as_str())
        .unwrap_or("mermaid");
    let diagram = match format {
        "dot" => crate::viz::dot::workflow_to_dot(&def, inst.as_ref()),
        _ => crate::viz::mermaid::workflow_to_mermaid(&def, inst.as_ref()),
    };

    let mut response = serde_json::json!({
        "workflow_id": workflow_id,
        "format": format,
        "diagram": diagram,
    });

    if let Some(inst) = &inst {
        response["instance_status"] = serde_json::json!({
            "instance_id": inst.instance_id,
            "status": format!("{}", inst.status),
            "cursor_node": inst.cursor.node_index,
        });
    }

    (StatusCode::OK, Json(response)).into_response()
}

// --- Meta-Agent API ---

async fn handle_meta_suggest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MetaSuggestRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    use crate::meta_agent::capability_index::CapabilityIndex;
    use crate::meta_agent::generator::WorkflowGenerator;
    use crate::meta_agent::template_library::TemplateLibrary;

    let ability_specs: Vec<_> = orch
        .ability_registry()
        .list_abilities()
        .into_iter()
        .cloned()
        .collect();
    let index = CapabilityIndex::build(&ability_specs, Vec::new(), &[]);
    let templates = TemplateLibrary::new();
    let generator = WorkflowGenerator::new(&index);

    drop(orch); // Release lock before potentially slow LLM call

    match generator.generate(&body.requirement, &templates) {
        Ok(def) => {
            let yaml = serde_yaml::to_string(&def).unwrap_or_default();
            let workflow_json = serde_json::to_value(&def).unwrap_or(serde_json::json!({}));
            (
                StatusCode::OK,
                Json(MetaSuggestResponse {
                    workflow: workflow_json,
                    yaml,
                }),
            )
                .into_response()
        }
        Err(e) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to generate workflow: {}", e),
        ),
    }
}

async fn handle_meta_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MetaCreateRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    use crate::meta_agent::capability_index::CapabilityIndex;
    use crate::meta_agent::generator::WorkflowGenerator;
    use crate::meta_agent::template_library::TemplateLibrary;

    let ability_specs: Vec<_> = orch
        .ability_registry()
        .list_abilities()
        .into_iter()
        .cloned()
        .collect();
    let index = CapabilityIndex::build(&ability_specs, Vec::new(), &[]);
    let templates = TemplateLibrary::new();
    let generator = WorkflowGenerator::new(&index);

    drop(orch); // Release lock before potentially slow LLM call

    match generator.generate(&body.requirement, &templates) {
        Ok(mut def) => {
            if let Some(ref name) = body.name {
                def.name = name.clone();
            }

            // Store in workflow engine if available
            let engine_arc = match &state.workflow_engine {
                Some(e) => e,
                None => {
                    return json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Workflow engine not available",
                    )
                }
            };
            let engine = match lock_or_500(engine_arc) {
                Ok(e) => e,
                Err(e) => return e,
            };

            match engine.store().store_def(&def) {
                Ok(()) => (
                    StatusCode::CREATED,
                    Json(MetaCreateResponse {
                        workflow_id: def.id.clone(),
                        name: def.name.clone(),
                    }),
                )
                    .into_response(),
                Err(e) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to store workflow: {}", e),
                ),
            }
        }
        Err(e) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to generate workflow: {}", e),
        ),
    }
}

async fn handle_meta_templates(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    use crate::meta_agent::template_library::TemplateLibrary;

    let templates = TemplateLibrary::new();
    let infos: Vec<MetaTemplateInfo> = templates
        .list()
        .iter()
        .map(|t| MetaTemplateInfo {
            id: t.def.id.clone(),
            name: t.def.name.clone(),
            category: t.category.clone(),
        })
        .collect();

    (StatusCode::OK, Json(infos)).into_response()
}

// --- Skill Forge API ---

async fn handle_skill_forge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SkillForgeRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    use crate::runtime::skill_forge::SkillForge;

    match body.tier.to_lowercase().as_str() {
        "chain" | "workflow" => {
            let orch = match lock_or_500(&state.orch) {
                Ok(o) => o,
                Err(e) => return e,
            };

            use crate::meta_agent::capability_index::CapabilityIndex;
            use crate::meta_agent::generator::WorkflowGenerator;
            use crate::meta_agent::template_library::TemplateLibrary;

            let ability_specs: Vec<_> = orch
                .ability_registry()
                .list_abilities()
                .into_iter()
                .cloned()
                .collect();
            let index = CapabilityIndex::build(&ability_specs, Vec::new(), &[]);
            let templates = TemplateLibrary::new();
            let generator = WorkflowGenerator::new(&index);

            drop(orch); // Release lock before workflow generation

            // Open workflow store
            let db_path = state.config.data_dir.join("workflows.db");
            let store = match crate::chain::workflow_store::WorkflowStore::open(&db_path) {
                Ok(s) => s,
                Err(e) => {
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to open workflow store: {}", e),
                    )
                }
            };

            match SkillForge::forge_chain(
                &body.requirement,
                &body.name,
                &generator,
                &templates,
                &store,
            ) {
                Ok(forged) => (
                    StatusCode::CREATED,
                    Json(SkillForgeResponse {
                        name: forged.name,
                        tier: format!("{}", forged.tier),
                        workflow_id: forged.workflow_id,
                        wasm_path: forged.wasm_path,
                        script_path: forged.script_path,
                        script_content: None,
                    }),
                )
                    .into_response(),
                Err(e) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to forge workflow skill: {}", e),
                ),
            }
        }
        "wasm" => {
            let skills_dir = state.config.data_dir.join("skills");
            match SkillForge::forge_wasm(&body.requirement, &body.name, &skills_dir) {
                Ok(forged) => (
                    StatusCode::CREATED,
                    Json(SkillForgeResponse {
                        name: forged.name,
                        tier: format!("{}", forged.tier),
                        workflow_id: forged.workflow_id,
                        wasm_path: forged.wasm_path,
                        script_path: forged.script_path,
                        script_content: None,
                    }),
                )
                    .into_response(),
                Err(e) => json_error(StatusCode::BAD_REQUEST, e),
            }
        }
        "shell" => {
            let scripts_dir = state.config.data_dir.join("scripts");
            match SkillForge::forge_shell(&body.requirement, &body.name, &scripts_dir) {
                Ok((forged, script_content)) => (
                    StatusCode::CREATED,
                    Json(SkillForgeResponse {
                        name: forged.name,
                        tier: format!("{}", forged.tier),
                        workflow_id: forged.workflow_id,
                        wasm_path: forged.wasm_path,
                        script_path: forged.script_path,
                        script_content: Some(script_content),
                    }),
                )
                    .into_response(),
                Err(e) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to forge shell skill: {}", e),
                ),
            }
        }
        other => json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Unknown tier: '{}'. Use 'workflow', 'wasm', or 'shell'.",
                other
            ),
        ),
    }
}

async fn handle_skills_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let db_path = state.config.data_dir.join("workflows.db");
    let store = match crate::chain::workflow_store::WorkflowStore::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to open workflow store: {}", e),
            )
        }
    };

    match store.list_defs() {
        Ok(defs) => {
            let items: Vec<SkillListItem> = defs
                .into_iter()
                .map(|(id, name)| SkillListItem { id, name })
                .collect();
            (StatusCode::OK, Json(items)).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

// --- Style API ---

/// POST /api/style -- set active style
async fn handle_set_style(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetStyleRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let mut orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match orch.set_style(&req.name) {
        Ok(()) => {
            let style_name = orch.active_style_name().map(|s| s.to_string());
            Json(serde_json::json!({
                "active_style": style_name,
                "message": format!("Style set to: {}", req.name)
            }))
            .into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

/// GET /api/style -- get current active style
async fn handle_get_style(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let style_name = orch.active_style_name().map(|s| s.to_string());
    Json(serde_json::json!({
        "active_style": style_name
    }))
    .into_response()
}

/// DELETE /api/style -- clear active style
async fn handle_clear_style(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let mut orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    orch.clear_style();
    Json(serde_json::json!({
        "active_style": null,
        "message": "Style cleared"
    }))
    .into_response()
}

// --- Permissions API ---

/// GET /api/v1/permissions -- get effective permissions (constitution + runtime overrides)
async fn handle_get_permissions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let constitution_perms = orch.constitution_channel_permissions();
    let effective = build_effective_permissions(&state.config, constitution_perms);
    let value = match serde_json::to_value(&effective) {
        Ok(v) => v,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize: {}", e),
            );
        }
    };
    Json(PermissionListResponse { permissions: value }).into_response()
}

/// POST /api/v1/permissions -- set runtime override for a channel
async fn handle_set_permissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetPermissionRequest>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    use crate::security::channel_permissions::{AccessLevel, ChannelAccess, PermissionEntry};

    let access_level = match req.access.as_deref() {
        Some("full") => AccessLevel::Full,
        Some("restricted") => AccessLevel::Restricted,
        Some("none") | None => AccessLevel::None,
        Some(other) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid access level: {}", other),
            );
        }
    };

    let parse_entries = |items: Option<Vec<String>>| -> Vec<PermissionEntry> {
        items
            .unwrap_or_default()
            .iter()
            .map(|s| PermissionEntry::parse(s))
            .collect()
    };

    let channel_access = ChannelAccess {
        access: access_level,
        contacts: parse_entries(req.contacts),
        groups: parse_entries(req.groups),
        domains: parse_entries(req.domains),
        send_domains: parse_entries(req.send_domains),
        servers: parse_entries(req.servers),
    };

    let db_path = permissions_db_path(&state.config);
    if let Err(e) = save_permission_override(&db_path, &req.channel, &channel_access) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }

    Json(serde_json::json!({
        "ok": true,
        "channel": req.channel,
        "message": format!("Permission override set for channel: {}", req.channel)
    }))
    .into_response()
}

/// DELETE /api/v1/permissions/{channel} -- remove runtime override for a channel
async fn handle_delete_permissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel): Path<String>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let db_path = permissions_db_path(&state.config);
    match delete_permission_override(&db_path, &channel) {
        Ok(true) => Json(serde_json::json!({
            "ok": true,
            "channel": channel,
            "message": format!("Permission override removed for channel: {}", channel)
        }))
        .into_response(),
        Ok(false) => json_error(
            StatusCode::NOT_FOUND,
            format!("No override found for channel: {}", channel),
        ),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// --- MCP API ---

async fn handle_mcp_list_servers(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let manager = orch.mcp_manager();
    let allowed = manager.allowed_servers();
    let mut servers = Vec::new();
    for server_id in &allowed {
        if let Some(config) = manager.server_config(server_id) {
            let tool_count =
                crate::mcp::discovery::load_tools_cache(manager.cache_dir(), server_id)
                    .ok()
                    .flatten()
                    .map(|t| t.len())
                    .unwrap_or(0);
            servers.push(serde_json::json!({
                "id": server_id,
                "trust_level": format!("{:?}", config.trust_level).to_lowercase(),
                "tool_count": tool_count,
                "status": "stopped",
            }));
        }
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "servers": servers })),
    )
        .into_response()
}

async fn handle_mcp_list_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let manager = orch.mcp_manager();
    match crate::mcp::discovery::load_tools_cache(manager.cache_dir(), &server_id) {
        Ok(Some(tools)) => {
            let tool_list: Vec<_> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "tools": tool_list })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No cached tools. Run discovery first."
            })),
        )
            .into_response(),
        Err(e) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load tools: {}", e),
        ),
    }
}

async fn handle_mcp_store_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let secret_name = body
        .get("secret_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let secret_value = body
        .get("secret_value")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if secret_name.is_empty() || secret_value.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "secret_name and secret_value required",
        );
    }
    // Validate mcp_ prefix
    if !secret_name.starts_with("mcp_") {
        return json_error(
            StatusCode::BAD_REQUEST,
            "MCP secrets must start with 'mcp_' prefix",
        );
    }
    match crate::providers::credentials::EncryptedFileStore::new(&state.config.data_dir) {
        Ok(store) => match store.set(secret_name, secret_value) {
            Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "stored": true }))).into_response(),
            Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
        },
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_mcp_discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    let server_id = body.get("server_id").and_then(|v| v.as_str()).unwrap_or("");
    if server_id.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "server_id required");
    }
    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let manager = orch.mcp_manager();
    match crate::mcp::discovery::load_tools_cache(manager.cache_dir(), server_id) {
        Ok(Some(tools)) => {
            let tool_list: Vec<_> = tools.iter().map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
            })).collect();
            (StatusCode::OK, Json(serde_json::json!({ "discovered": true, "tools": tool_list }))).into_response()
        }
        _ => {
            (StatusCode::OK, Json(serde_json::json!({
                "discovered": false,
                "message": "No cached tools. Live discovery requires starting the server (future feature)."
            }))).into_response()
        }
    }
}

// --- Agents API ---

async fn handle_list_agents(State(state): State<AppState>) -> impl IntoResponse {
    let orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    let agents = orch.list_agents();
    let active = orch.active_agent().to_string();
    Json(serde_json::json!({
        "agents": agents,
        "active": active,
    }))
}

#[derive(Deserialize)]
struct SetAgentRequest {
    agent_id: String,
}

async fn handle_set_agent(
    State(state): State<AppState>,
    Json(body): Json<SetAgentRequest>,
) -> impl IntoResponse {
    let mut orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    orch.set_active_agent(&body.agent_id);
    Json(serde_json::json!({ "active": body.agent_id }))
}

// --- Providers API ---

async fn handle_list_providers(State(state): State<AppState>) -> impl IntoResponse {
    let orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    let registry = orch.provider_registry();
    let mut providers: Vec<serde_json::Value> = registry
        .list_all()
        .into_iter()
        .filter_map(|id| {
            let p = registry.get(id)?;
            let configured = registry.get_api_key(&p.id).is_some();
            Some(serde_json::json!({
                "id": p.id,
                "display_name": p.display_name,
                "configured": configured,
                "model_count": p.models.len(),
                "supports_tools": p.supports_tools,
                "supports_vision": p.supports_vision,
            }))
        })
        .collect();
    providers.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Json(serde_json::json!({ "providers": providers }))
}

#[derive(Deserialize)]
struct SetProviderKeyRequest {
    provider_id: String,
    api_key: String,
}

async fn handle_set_provider_key(
    State(state): State<AppState>,
    Json(body): Json<SetProviderKeyRequest>,
) -> impl IntoResponse {
    // Store in encrypted file
    if let Ok(cred_store) =
        crate::providers::credentials::EncryptedFileStore::new(&state.config.data_dir)
    {
        if let Err(e) = cred_store.set(&body.provider_id, &body.api_key) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    }
    // Also set in registry for immediate use
    let mut orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    orch.provider_registry_mut()
        .set_api_key(&body.provider_id, body.api_key);
    Json(serde_json::json!({ "ok": true })).into_response()
}

// ---------------------------------------------------------------------------
// Webhook incoming handler (unauthenticated — external services POST here)
// ---------------------------------------------------------------------------

/// Handle Slack Events API — verifies signature, responds to url_verification
/// challenges, and acknowledges event callbacks.
async fn handle_slack_events(
    State(_state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Verify Slack request signature if signing secret is configured
    let slack = crate::channels::slack::SlackChannel::new();
    if slack.has_signing_secret() {
        let timestamp = headers
            .get("X-Slack-Request-Timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        let signature = headers
            .get("X-Slack-Signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !slack.verify_signature(timestamp, &body, signature) {
            return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
        }
    }

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        if json.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
            let challenge = json
                .get("challenge")
                .and_then(|c| c.as_str())
                .unwrap_or_default();
            return Json(serde_json::json!({"challenge": challenge})).into_response();
        }
    }
    (StatusCode::OK, "ok").into_response()
}

// ---------------------------------------------------------------------------

/// Handle incoming POST to a registered webhook endpoint.
async fn handle_webhook_incoming(
    Path(webhook_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    use crate::runtime::host_functions::webhook_store;

    let store = match webhook_store() {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let guard = match store.lock() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("lock: {}", e) })),
            )
                .into_response();
        }
    };

    // Serialize headers to JSON
    let mut headers_map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        if let Ok(val_str) = value.to_str() {
            headers_map.insert(
                name.as_str().to_string(),
                serde_json::Value::String(val_str.to_string()),
            );
        }
    }
    let headers_json = serde_json::Value::Object(headers_map).to_string();

    // Body as UTF-8 (lossy)
    let body_str = String::from_utf8_lossy(&body).to_string();

    // Extract signature from X-Webhook-Signature header
    let signature = headers
        .get("x-webhook-signature")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    match guard.validate_and_store(&webhook_id, &headers_json, &body_str, signature.as_deref()) {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "webhook_id": webhook_id,
        }))
        .into_response(),
        Err(e) if e.contains("not found or expired") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
        Err(e) if e.contains("HMAC") || e.contains("signature") => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

// --- Resource API ---

async fn handle_list_resources(State(state): State<AppState>) -> impl IntoResponse {
    let orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    match orch.resource_registry().list_resources() {
        Ok(resources) => Json(serde_json::json!({ "resources": resources })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_resource_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    match orch.resource_registry().get_resource(&id) {
        Ok(Some(r)) => Json(serde_json::json!({ "resource": r })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_list_leases(State(state): State<AppState>) -> impl IntoResponse {
    let orch = state.orch.lock().unwrap_or_else(|p| p.into_inner());
    match orch.resource_registry().list_active_leases() {
        Ok(leases) => Json(serde_json::json!({ "leases": leases })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Build-not-found fallback when nabaos-web/dist does not exist.
async fn fallback_html() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>NabaOS Dashboard</title>
<style>body{font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#1a1a2e;color:#e0e0e0}
.box{text-align:center;padding:2rem;border:1px solid #333;border-radius:8px;background:#16213e}
h1{margin-top:0}code{background:#0f3460;padding:2px 6px;border-radius:4px}</style></head>
<body><div class="box">
<h1>NabaOS Dashboard</h1>
<p>Frontend build not found.</p>
<p>Run <code>cd nabaos-web &amp;&amp; npm run build</code> to build the SPA.</p>
<p>The REST API is available at <code>/api/v1/*</code>.</p>
</div></body></html>"#,
    )
}

// ---------------------------------------------------------------------------
// Memory handlers
// ---------------------------------------------------------------------------

async fn handle_get_memory(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.conversation_history(50) {
        Ok(turns) => {
            let json_turns: Vec<serde_json::Value> = turns
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "role": t.role.as_str(),
                        "content": t.content,
                        "token_estimate": t.token_estimate,
                        "created_at": t.created_at,
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "turns": json_turns })),
            )
                .into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

async fn handle_delete_memory(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }

    let orch = match lock_or_500(&state.orch) {
        Ok(o) => o,
        Err(e) => return e,
    };

    match orch.memory_store().delete_session("default") {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "deleted": count })),
        )
            .into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)),
    }
}

// ---------------------------------------------------------------------------
// Webhook trigger dispatch handler
// ---------------------------------------------------------------------------

/// Handle incoming POST to `/triggers/{agent_id}/{*path}` — resolves via TriggerEngine
/// and starts the associated workflow chain.
async fn handle_webhook(
    Path((agent_id, path)): Path<(String, String)>,
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    // Check if trigger engine is available
    let te = match state.trigger_engine.as_ref() {
        Some(te) => te,
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                "Webhooks not enabled".to_string(),
            );
        }
    };

    let full_path = format!("{}/{}", agent_id, path);
    match te.lock() {
        Ok(engine) => match engine.resolve_webhook(&full_path) {
            Some(route) => {
                if let Some(ref we) = state.workflow_engine {
                    match we.lock() {
                        Ok(we) => {
                            let mut params = route.params.clone();
                            params.insert("webhook_body".to_string(), body);
                            match we.start(&route.chain, params) {
                                Ok(id) => {
                                    (StatusCode::OK, format!("{{\"workflow_id\":\"{}\"}}", id))
                                }
                                Err(e) => (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("{{\"error\":\"{}\"}}", e),
                                ),
                            }
                        }
                        Err(_) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Engine lock failed".to_string(),
                        ),
                    }
                } else {
                    (
                        StatusCode::NOT_IMPLEMENTED,
                        "Workflow engine not available".to_string(),
                    )
                }
            }
            None => (
                StatusCode::NOT_FOUND,
                format!("No webhook for path: {}", full_path),
            ),
        },
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Trigger engine lock failed".to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Router & Server
// ---------------------------------------------------------------------------

/// Create the axum router with all API routes and optional static file serving.
async fn handle_health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION")
        })),
    )
}

async fn oauth_start(State(state): State<AppState>) -> Response {
    use crate::modules::oauth::token_manager::{OAuthProvider, TokenManager};

    let port = std::env::var("NABA_WEB_PORT").unwrap_or_else(|_| "3000".to_string());

    let provider = OAuthProvider {
        name: "google".to_string(),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        client_id: std::env::var("NABA_GOOGLE_CLIENT_ID").unwrap_or_default(),
        client_secret: std::env::var("NABA_GOOGLE_CLIENT_SECRET").ok(),
        scopes: vec!["https://www.googleapis.com/auth/calendar".to_string()],
        redirect_uri: format!("http://localhost:{}/oauth/callback", port),
    };

    if provider.client_id.is_empty() {
        return Html(
            "<html><body><h2>Error</h2><p>NABA_GOOGLE_CLIENT_ID not set.</p></body></html>",
        )
        .into_response();
    }

    let (verifier, challenge) = TokenManager::generate_pkce();
    let state_param = format!("nyaya_{}", uuid::Uuid::new_v4());
    let auth_url = TokenManager::auth_url(&provider, &state_param, &challenge);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    state.oauth_pending.lock().unwrap().insert(
        state_param,
        PendingOAuthFlow {
            provider,
            code_verifier: verifier,
            created_at: now,
        },
    );

    axum::response::Redirect::temporary(&auth_url).into_response()
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Response {
    use crate::modules::oauth::token_manager::TokenManager;

    if let Some(ref err) = params.error {
        let desc = params
            .error_description
            .as_deref()
            .unwrap_or("Unknown error");
        return Html(format!(
            "<html><body><h2>OAuth Error</h2><p>{}: {}</p><p>You can close this window.</p></body></html>",
            err, desc
        )).into_response();
    }

    let state_param = match &params.state {
        Some(s) => s.clone(),
        None => {
            return Html("<html><body><h2>Error</h2><p>Missing state parameter.</p></body></html>")
                .into_response();
        }
    };

    // Look up the pending flow
    let flow = state.oauth_pending.lock().unwrap().remove(&state_param);
    let flow = match flow {
        Some(f) => f,
        None => {
            return Html(
                "<html><body><h2>Error</h2><p>Unknown or expired OAuth flow.</p></body></html>",
            )
            .into_response();
        }
    };

    // Exchange authorization code for tokens
    match TokenManager::exchange_code(&flow.provider, &params.code, &flow.code_verifier).await {
        Ok(token_pair) => {
            // Store the token
            let db_path = state.config.data_dir.join("oauth_tokens.db");
            match TokenManager::open(&db_path) {
                Ok(mgr) => {
                    if let Err(e) = mgr.store(&flow.provider.name, &token_pair) {
                        tracing::error!("Failed to store OAuth token: {}", e);
                        return Html(format!(
                            "<html><body><h2>Token Storage Error</h2><p>{}</p></body></html>",
                            e
                        ))
                        .into_response();
                    }
                    tracing::info!(provider = %flow.provider.name, "OAuth token exchanged and stored");
                }
                Err(e) => {
                    tracing::error!("Failed to open token database: {}", e);
                    return Html(format!(
                        "<html><body><h2>Database Error</h2><p>{}</p></body></html>",
                        e
                    ))
                    .into_response();
                }
            }

            Html(
                "<html><body><h2>Authorization Successful</h2>\
                 <p>Token stored. You can close this window and return to the agent.</p>\
                 <script>window.close();</script></body></html>",
            )
            .into_response()
        }
        Err(e) => {
            tracing::error!("OAuth token exchange failed: {}", e);
            Html(format!(
                "<html><body><h2>Token Exchange Failed</h2><p>{}</p>\
                 <p>You can close this window.</p></body></html>",
                e
            ))
            .into_response()
        }
    }
}

/// Background task: removes expired OAuth flows every 60 seconds.
fn spawn_oauth_cleanup(pending: Arc<Mutex<HashMap<String, PendingOAuthFlow>>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let mut flows = pending.lock().unwrap();
            let before = flows.len();
            flows.retain(|_, flow| now.saturating_sub(flow.created_at) < 600);
            let removed = before - flows.len();
            if removed > 0 {
                tracing::debug!(removed, "Cleaned up expired OAuth flows");
            }
        }
    });
}

pub fn create_router(state: AppState) -> Router {
    let api = Router::new()
        // Auth (/api/v1/auth/*)
        .route("/api/v1/auth/login", post(handle_login))
        .route("/api/v1/auth/logout", post(handle_logout))
        .route("/api/v1/auth/status", get(handle_auth_status))
        .route("/api/v1/auth/confirm/{token}", post(handle_confirm_weblink))
        .route("/api/v1/auth/elevate", post(handle_auth_elevate))
        .route("/api/v1/auth/telegram", post(handle_telegram_auth))
        // Dashboard
        .route("/api/v1/dashboard", get(handle_dashboard))
        // Query
        .route("/api/v1/ask", post(handle_query))
        .route("/api/v1/ask/stream", post(handle_query_stream))
        .route("/api/v1/confirm/{id}", post(handle_confirm))
        // Workflows (/api/v1/workflows/*) — combined chains + workflows
        .route("/api/v1/workflows", get(handle_list_workflows_combined))
        .route("/api/v1/workflows/schedule", get(handle_list_schedule))
        .route("/api/v1/workflows/schedule", post(handle_create_schedule))
        .route(
            "/api/v1/workflows/schedule/{id}",
            delete(handle_disable_schedule),
        )
        .route("/api/v1/workflows/start", post(handle_start_workflow))
        .route("/api/v1/workflows/{id}/status", get(handle_workflow_status))
        .route(
            "/api/v1/workflows/{id}/cancel",
            post(handle_cancel_workflow),
        )
        .route(
            "/api/v1/workflows/{id}/visualize",
            get(handle_workflow_visualize),
        )
        // Status (/api/v1/status/*)
        .route("/api/v1/status", get(handle_costs))
        .route("/api/v1/status/abilities", get(handle_abilities))
        // Cost dashboard (/api/v1/costs/dashboard)
        .route("/api/v1/costs/dashboard", get(handle_costs_dashboard))
        // Security (/api/v1/security/*)
        .route("/api/v1/security/scan", post(handle_security_scan))
        // Rules (constitution)
        .route("/api/v1/rules", get(handle_constitution))
        // Personas (/api/v1/personas/*)
        .route("/api/v1/personas", get(handle_list_agents))
        .route("/api/v1/personas/active", post(handle_set_agent))
        // Vault (/api/v1/vault/*)
        .route("/api/v1/vault", get(handle_list_providers))
        .route("/api/v1/vault/store", post(handle_set_provider_key))
        // Tools (/api/v1/tools/*) — formerly MCP
        .route("/api/v1/tools/servers", get(handle_mcp_list_servers))
        .route("/api/v1/tools/{server_id}", get(handle_mcp_list_tools))
        .route("/api/v1/tools/secret", post(handle_mcp_store_secret))
        .route("/api/v1/tools/discover", post(handle_mcp_discover))
        // Style
        .route("/api/v1/style", post(handle_set_style))
        .route("/api/v1/style", get(handle_get_style))
        .route("/api/v1/style", delete(handle_clear_style))
        // Permissions (/api/v1/permissions/*)
        .route("/api/v1/permissions", get(handle_get_permissions))
        .route("/api/v1/permissions", post(handle_set_permissions))
        .route(
            "/api/v1/permissions/{channel}",
            delete(handle_delete_permissions),
        )
        // Skills (/api/v1/skills/*)
        .route("/api/v1/skills/forge", post(handle_skill_forge))
        .route("/api/v1/skills", get(handle_skills_list))
        // Admin (/api/v1/admin/*) — formerly meta-agent
        .route("/api/v1/admin/suggest", post(handle_meta_suggest))
        .route("/api/v1/admin/create", post(handle_meta_create))
        .route("/api/v1/admin/templates", get(handle_meta_templates))
        // Workflow hooks (unauthenticated — external services POST here)
        .route(
            "/hooks/{workflow_id}/{correlation}",
            post(handle_hook_incoming),
        )
        // Agent trigger webhooks (unauthenticated — dispatches via TriggerEngine)
        .route("/triggers/{agent_id}/{*path}", post(handle_webhook))
        // Slack events (unauthenticated — Slack sends challenge + events)
        .route("/api/v1/slack/events", post(handle_slack_events))
        // Webhooks (unauthenticated — external services POST here)
        .route("/api/webhooks/{id}", post(handle_webhook_incoming))
        // Resources (/api/v1/resources/*)
        .route("/api/v1/resources", get(handle_list_resources))
        .route("/api/v1/resources/leases", get(handle_list_leases))
        .route("/api/v1/resources/{id}", get(handle_resource_status))
        // Browser management (/api/v1/browser/*)
        .route(
            "/api/v1/browser/sessions",
            get(browser_sessions_handler).delete(browser_clear_sessions_handler),
        )
        .route(
            "/api/v1/browser/captcha-config",
            get(browser_captcha_config_handler),
        )
        // Memory (/api/v1/memory)
        .route(
            "/api/v1/memory",
            get(handle_get_memory).delete(handle_delete_memory),
        )
        // Health (unauthenticated — load balancer readiness)
        .route("/health", get(handle_health))
        // OAuth (unauthenticated — browser redirect from OAuth provider)
        .route("/oauth/start", get(oauth_start))
        .route("/oauth/callback", get(oauth_callback))
        .with_state(state);

    // Serve embedded static files (compiled into the binary via rust-embed),
    // with SPA fallback to index.html for client-side routing.
    if WebAssets::get("index.html").is_some() {
        api.fallback(|uri: axum::http::Uri| async move {
            let path = uri.path().trim_start_matches('/');
            if let Some(file) = WebAssets::get(path) {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                (
                    [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                    file.data.to_vec(),
                )
                    .into_response()
            } else if let Some(index) = WebAssets::get("index.html") {
                // SPA fallback — serve index.html for client-side routing
                (
                    [(axum::http::header::CONTENT_TYPE, "text/html".to_string())],
                    index.data.to_vec(),
                )
                    .into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        })
    } else {
        api.fallback(fallback_html)
    }
}

/// Start the web server.
pub async fn run_server(
    config: NyayaConfig,
    orch: Orchestrator,
    two_fa: TwoFactorAuth,
    bind_addr: &str,
) -> Result<()> {
    run_server_with_engine(config, orch, two_fa, bind_addr, None, None).await
}

/// Start the web server with an optional workflow engine.
pub async fn run_server_with_engine(
    config: NyayaConfig,
    orch: Orchestrator,
    two_fa: TwoFactorAuth,
    bind_addr: &str,
    workflow_engine: Option<Arc<Mutex<WorkflowEngine>>>,
    shutdown_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
    // Hash the password from env if set
    let password_hash = std::env::var("NABA_WEB_PASSWORD")
        .ok()
        .map(|pw| TwoFactorAuth::hash_password(&pw));

    let state = AppState {
        orch: Arc::new(Mutex::new(orch)),
        two_fa: Arc::new(two_fa),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        password_hash,
        session_ttl_secs: std::env::var("NABA_WEB_SESSION_TTL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24 * 60 * 60),
        config,
        bot_token: std::env::var("NABA_TELEGRAM_BOT_TOKEN").ok(),
        workflow_engine,
        privilege_guard: Some(Arc::new(crate::security::privilege::PrivilegeGuard::new())),
        trigger_engine: None,
        rate_limits: Arc::new(Mutex::new(HashMap::new())),
        oauth_pending: Arc::new(Mutex::new(HashMap::new())),
        pending_confirmations: Arc::new(Mutex::new(HashMap::new())),
    };

    spawn_oauth_cleanup(state.oauth_pending.clone());

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| {
            crate::core::error::NyayaError::Config(format!(
                "Failed to bind to {}: {}",
                bind_addr, e
            ))
        })?;

    tracing::info!("Nyaya web dashboard listening on http://{}", bind_addr);

    if let Some(mut rx) = shutdown_rx {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.wait_for(|&v| v).await;
                tracing::info!("Web server shutting down gracefully...");
            })
            .await
            .map_err(|e| {
                crate::core::error::NyayaError::Config(format!("Web server error: {}", e))
            })?;
    } else {
        axum::serve(listener, app)
            .await
            .map_err(|e| {
                crate::core::error::NyayaError::Config(format!("Web server error: {}", e))
            })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Browser management handlers
// ---------------------------------------------------------------------------

async fn browser_sessions_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "sessions": [],
        "message": "No saved sessions"
    }))
}

async fn browser_clear_sessions_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "cleared": true,
        "message": "All sessions cleared"
    }))
}

async fn browser_captcha_config_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "enabled": false,
        "vlm_enabled": false,
        "capsolver_configured": false,
        "message": "Advanced CAPTCHA solving not configured"
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a real AppState backed by a temp directory so that
    /// `create_router` can be called.
    fn test_state() -> AppState {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = NyayaConfig {
            data_dir: tmp.path().to_path_buf(),
            model_path: tmp.path().join("models"),
            constitution_path: None,
            llm_api_key: None,
            llm_provider: None,
            llm_base_url: None,
            llm_model: None,
            daily_budget_usd: None,
            per_task_budget_usd: None,
            plugin_dir: tmp.path().join("plugins"),
            subprocess_config: None,
            constitution_template: None,
            profile: crate::modules::profile::ModuleProfile::default(),
        };
        let orch = Orchestrator::new(config.clone()).expect("orchestrator");
        let two_fa = TwoFactorAuth::new(crate::security::two_factor::TwoFactorMethod::None);
        AppState {
            orch: Arc::new(Mutex::new(orch)),
            two_fa: Arc::new(two_fa),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            password_hash: None,
            session_ttl_secs: 3600,
            config: NyayaConfig {
                data_dir: tmp.path().to_path_buf(),
                model_path: tmp.path().join("models"),
                constitution_path: None,
                llm_api_key: None,
                llm_provider: None,
                llm_base_url: None,
                llm_model: None,
                daily_budget_usd: None,
                per_task_budget_usd: None,
                plugin_dir: tmp.path().join("plugins"),
                subprocess_config: None,
                constitution_template: None,
                profile: crate::modules::profile::ModuleProfile::default(),
            },
            bot_token: None,
            workflow_engine: None,
            privilege_guard: None,
            trigger_engine: None,
            rate_limits: Arc::new(Mutex::new(HashMap::new())),
            oauth_pending: Arc::new(Mutex::new(HashMap::new())),
            pending_confirmations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[test]
    fn test_api_v1_routes_exist() {
        // Building the router exercises all .route() calls — if any handler
        // signature is wrong or a path is malformed, this will panic.
        let state = test_state();
        let _router = create_router(state);
    }

    #[test]
    fn test_old_routes_removed() {
        // Read the source of create_router and verify that the old route
        // prefixes are no longer present. We check the literal string
        // patterns that were renamed.
        let source = include_str!("web.rs");

        // Extract only the create_router function body for precision
        let router_start = source
            .find("pub fn create_router")
            .expect("create_router not found");
        let router_section = &source[router_start..];
        // Find the closing of with_state(state); which ends the route block
        let router_end = router_section
            .find(".with_state(state);")
            .expect("with_state not found")
            + ".with_state(state);".len();
        let router_body = &router_section[..router_end];

        // These old route paths must NOT appear in the router definition.
        // Each pattern is crafted so it does NOT match the new /api/v1/ paths.
        let removed_routes = [
            "\"/api/query\"",
            "\"/api/chains\"",
            "\"/api/chains/schedule\"",
            "\"/api/costs\"",
            "\"/api/abilities\"",
            "\"/api/constitution\"",
            "\"/api/telegram-auth\"",
            "\"/api/agents\"",
            "\"/api/agents/active\"",
            "\"/api/providers\"",
            "\"/api/providers/key\"",
            "\"/api/mcp/",
            "\"/api/meta/",
            "\"/api/dashboard\"",
        ];

        for old in &removed_routes {
            assert!(
                !router_body.contains(old),
                "Old route {} still present in create_router",
                old
            );
        }

        // For routes where old path is a substring of new (e.g., /api/style
        // inside /api/v1/style), count occurrences — there should be ZERO
        // occurrences of the old prefix without /v1/.
        // We do this by confirming every "/api/" in the route block has "/api/v1/"
        // (except webhook routes which are intentionally kept).
        let webhook_exceptions = ["/api/webhooks/"];
        for line in router_body.lines() {
            if let Some(pos) = line.find("\"/api/") {
                let route_str = &line[pos + 1..]; // skip opening quote
                if webhook_exceptions
                    .iter()
                    .any(|ex| route_str.starts_with(ex))
                {
                    continue;
                }
                assert!(
                    route_str.starts_with("/api/v1/"),
                    "Route line does not use /api/v1/ prefix: {}",
                    line.trim()
                );
            }
        }

        // Verify new v1 routes ARE present
        let required_v1_routes = [
            "/api/v1/auth/login",
            "/api/v1/auth/logout",
            "/api/v1/auth/status",
            "/api/v1/auth/confirm/",
            "/api/v1/auth/elevate",
            "/api/v1/auth/telegram",
            "/api/v1/dashboard",
            "/api/v1/ask",
            "/api/v1/workflows",
            "/api/v1/status",
            "/api/v1/status/abilities",
            "/api/v1/security/scan",
            "/api/v1/rules",
            "/api/v1/personas",
            "/api/v1/personas/active",
            "/api/v1/vault",
            "/api/v1/vault/store",
            "/api/v1/tools/servers",
            "/api/v1/style",
            "/api/v1/skills",
            "/api/v1/admin/suggest",
            "/api/v1/resources",
        ];

        for route in &required_v1_routes {
            assert!(
                router_body.contains(route),
                "Expected v1 route {} not found in create_router",
                route
            );
        }

        // Webhook routes must remain unchanged (external integrations)
        assert!(
            router_body.contains("/hooks/{workflow_id}/{correlation}"),
            "Webhook hook route must be preserved"
        );
        assert!(
            router_body.contains("/api/webhooks/{id}"),
            "Webhook incoming route must be preserved"
        );
    }

    #[test]
    fn test_sse_endpoint_returns_event_stream() {
        // Verify the SSE handler compiles and the router builds with it.
        // The handler returns Sse<impl Stream>, so if this compiles the
        // return type is correct.
        let state = test_state();
        let _router = create_router(state);
        // If we got here, the SSE handler compiled and was registered.
    }

    #[test]
    fn test_sse_route_registered() {
        // Verify /api/v1/ask/stream is present in the router definition.
        let source = include_str!("web.rs");
        let router_start = source
            .find("pub fn create_router")
            .expect("create_router not found");
        let router_section = &source[router_start..];
        let router_end = router_section
            .find(".with_state(state);")
            .expect("with_state not found")
            + ".with_state(state);".len();
        let router_body = &router_section[..router_end];

        assert!(
            router_body.contains("/api/v1/ask/stream"),
            "SSE route /api/v1/ask/stream not found in create_router"
        );
    }

    #[tokio::test]
    async fn test_rate_limit_check() {
        let state = test_state();
        // First 10 requests should pass
        for _ in 0..10 {
            assert!(check_rate_limit(&state, "test_ip", 10, 60));
        }
        // 11th should fail
        assert!(!check_rate_limit(&state, "test_ip", 10, 60));
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        use tower::util::ServiceExt;

        let state = test_state();
        let app: Router = create_router(state);
        let req = axum::http::Request::builder()
            .uri("/health")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["version"].is_string());
    }

    #[test]
    fn test_oauth_callback_params_deserialize() {
        let json = r#"{"code":"abc123","state":"test","error":null}"#;
        let params: OAuthCallbackParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.code, "abc123");
        assert_eq!(params.state.as_deref(), Some("test"));
        assert!(params.error.is_none());
    }

    #[test]
    fn test_oauth_callback_error_params() {
        let json = r#"{"code":"","state":"test","error":"access_denied","error_description":"User denied"}"#;
        let params: OAuthCallbackParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.error.as_deref(), Some("access_denied"));
    }

    // --- Task 14 tests: cost dashboard API ---

    #[tokio::test]
    async fn test_costs_api_response() {
        use tower::util::ServiceExt;

        let state = test_state();
        let app: Router = create_router(state);
        // No auth required (password_hash is None in test_state)
        let req = axum::http::Request::builder()
            .uri("/api/v1/costs/dashboard")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify JSON structure has daily/weekly/monthly keys
        assert!(
            json["daily"].is_object(),
            "Response should have daily object: {}",
            json
        );
        assert!(
            json["weekly"].is_object(),
            "Response should have weekly object: {}",
            json
        );
        assert!(
            json["monthly"].is_object(),
            "Response should have monthly object: {}",
            json
        );

        // Verify daily has expected fields
        assert!(
            json["daily"]["total_cost"].is_number(),
            "daily should have total_cost"
        );
        assert!(
            json["daily"]["total_calls"].is_number(),
            "daily should have total_calls"
        );
        assert!(
            json["daily"]["cache_hit_rate"].is_number(),
            "daily should have cache_hit_rate"
        );

        // Verify monthly has estimated_savings
        assert!(
            json["monthly"]["estimated_savings"].is_number(),
            "monthly should have estimated_savings"
        );
    }

    /// Helper: build AppState with a temp directory that persists (for DB tests).
    fn test_state_persistent() -> AppState {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _ = std::mem::ManuallyDrop::new(tmp.path().to_path_buf());
        let config = NyayaConfig {
            data_dir: tmp.path().to_path_buf(),
            model_path: tmp.path().join("models"),
            constitution_path: None,
            llm_api_key: None,
            llm_provider: None,
            llm_base_url: None,
            llm_model: None,
            daily_budget_usd: None,
            per_task_budget_usd: None,
            plugin_dir: tmp.path().join("plugins"),
            subprocess_config: None,
            constitution_template: None,
            profile: crate::modules::profile::ModuleProfile::default(),
        };
        let orch = Orchestrator::new(config.clone()).expect("orchestrator");
        let two_fa = TwoFactorAuth::new(crate::security::two_factor::TwoFactorMethod::None);
        // Leak the tempdir so it survives the test
        let _leaked = std::mem::ManuallyDrop::new(tmp);
        AppState {
            orch: Arc::new(Mutex::new(orch)),
            two_fa: Arc::new(two_fa),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            password_hash: None,
            session_ttl_secs: 3600,
            config,
            bot_token: None,
            workflow_engine: None,
            privilege_guard: None,
            trigger_engine: None,
            rate_limits: Arc::new(Mutex::new(HashMap::new())),
            oauth_pending: Arc::new(Mutex::new(HashMap::new())),
            pending_confirmations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[tokio::test]
    async fn test_set_and_get_permissions() {
        use tower::util::ServiceExt;

        let state = test_state_persistent();
        let app = create_router(state);

        // POST: set permission override for "whatsapp"
        let body = serde_json::json!({
            "channel": "whatsapp",
            "access": "restricted",
            "contacts": ["+91XXX", "-+91YYY"],
            "groups": [],
            "domains": [],
            "send_domains": [],
            "servers": []
        });
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions")
            .method("POST")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(json["ok"], true);

        // GET: retrieve permissions and verify the override is present
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let perms = &json["permissions"];
        assert!(
            perms["channels"]["whatsapp"].is_object(),
            "whatsapp channel should exist in permissions"
        );
        assert_eq!(perms["channels"]["whatsapp"]["access"], "restricted");
        // Check contacts include +91XXX and -+91YYY
        let contacts = perms["channels"]["whatsapp"]["contacts"]
            .as_array()
            .unwrap();
        assert!(contacts.contains(&serde_json::json!("+91XXX")));
        assert!(contacts.contains(&serde_json::json!("-+91YYY")));
    }

    #[tokio::test]
    async fn test_delete_permissions() {
        use tower::util::ServiceExt;

        let state = test_state_persistent();
        let app = create_router(state);

        // First, set a permission
        let body = serde_json::json!({
            "channel": "telegram",
            "access": "full"
        });
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions")
            .method("POST")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // DELETE the permission
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions/telegram")
            .method("DELETE")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(json["ok"], true);

        // Verify it's gone from the GET response
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        // telegram should not appear in channels (no constitution config, override deleted)
        assert!(
            json["permissions"]["channels"].get("telegram").is_none(),
            "telegram channel should have been removed"
        );
    }

    #[tokio::test]
    async fn test_override_merges_with_constitution() {
        use tower::util::ServiceExt;

        // Create a state where the orchestrator has constitution-level channel permissions.
        // Since test_state_persistent() uses no constitution, we set overrides via POST and verify
        // the GET returns the merged result (default + override).
        let state = test_state_persistent();
        let app = create_router(state);

        // Set overrides for two channels
        for (ch, access) in [("email", "full"), ("discord", "none")] {
            let body = serde_json::json!({
                "channel": ch,
                "access": access
            });
            let req = axum::http::Request::builder()
                .uri("/api/v1/permissions")
                .method("POST")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // GET: verify both overrides appear in the merged result
        let req = axum::http::Request::builder()
            .uri("/api/v1/permissions")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let channels = &json["permissions"]["channels"];
        assert_eq!(channels["email"]["access"], "full");
        assert_eq!(channels["discord"]["access"], "none");
    }

    #[test]
    fn test_build_effective_permissions_caps_at_constitution_ceiling() {
        use crate::security::channel_permissions::*;

        // Constitution says: telegram=Restricted, email=None (no access)
        let mut constitution_channels = HashMap::new();
        constitution_channels.insert(
            "telegram".to_string(),
            ChannelAccess {
                access: AccessLevel::Restricted,
                contacts: vec![PermissionEntry {
                    pattern: "baduser".into(),
                    excluded: true,
                }],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        constitution_channels.insert(
            "email".to_string(),
            ChannelAccess {
                access: AccessLevel::None,
                contacts: vec![],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        );
        let constitution_perms = ChannelPermissions {
            default_access: AccessLevel::None,
            channels: constitution_channels,
        };

        // Create a temp config with a DB that has overrides trying to exceed the ceiling
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = NyayaConfig {
            data_dir: tmp.path().to_path_buf(),
            model_path: tmp.path().join("models"),
            constitution_path: None,
            llm_api_key: None,
            llm_provider: None,
            llm_base_url: None,
            llm_model: None,
            daily_budget_usd: None,
            per_task_budget_usd: None,
            plugin_dir: tmp.path().join("plugins"),
            subprocess_config: None,
            constitution_template: None,
            profile: crate::modules::profile::ModuleProfile::default(),
        };

        // Save overrides that try to escalate beyond constitution ceiling
        let db_path = permissions_db_path(&config);
        ensure_permission_overrides_table(&db_path);

        // Override tries to set telegram to Full (constitution says Restricted)
        save_permission_override(
            &db_path,
            "telegram",
            &ChannelAccess {
                access: AccessLevel::Full,
                contacts: vec![], // tries to remove the exclude for "baduser"
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        )
        .unwrap();

        // Override tries to set email to Full (constitution says None)
        save_permission_override(
            &db_path,
            "email",
            &ChannelAccess {
                access: AccessLevel::Full,
                contacts: vec![],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        )
        .unwrap();

        // Override tries to set discord to Full (constitution has no entry, default_access=None)
        save_permission_override(
            &db_path,
            "discord",
            &ChannelAccess {
                access: AccessLevel::Full,
                contacts: vec![],
                groups: vec![],
                domains: vec![],
                send_domains: vec![],
                servers: vec![],
            },
        )
        .unwrap();

        let effective = build_effective_permissions(&config, Some(&constitution_perms));

        // Telegram: override tried Full, but constitution ceiling is Restricted
        let telegram = effective
            .channels
            .get("telegram")
            .expect("telegram present");
        assert_eq!(
            telegram.access,
            AccessLevel::Restricted,
            "telegram must be capped at Restricted"
        );

        // Telegram: constitution exclude for "baduser" must be preserved
        assert!(
            telegram
                .contacts
                .iter()
                .any(|e| e.excluded && e.pattern == "baduser"),
            "constitution exclude for baduser must be preserved"
        );

        // Email: override tried Full, but constitution ceiling is None
        let email = effective.channels.get("email").expect("email present");
        assert_eq!(
            email.access,
            AccessLevel::None,
            "email must be capped at None"
        );

        // Discord: override tried Full, but constitution default_access is None
        let discord = effective.channels.get("discord").expect("discord present");
        assert_eq!(
            discord.access,
            AccessLevel::None,
            "discord must be capped at default None"
        );
    }

    #[tokio::test]
    async fn test_browser_sessions_api() {
        use tower::util::ServiceExt;

        let state = test_state();
        let app: Router = create_router(state);
        let req = axum::http::Request::builder()
            .uri("/api/v1/browser/sessions")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["sessions"].is_array(),
            "Response should have sessions array"
        );
        assert_eq!(json["sessions"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_browser_captcha_config_api() {
        use tower::util::ServiceExt;

        let state = test_state();
        let app: Router = create_router(state);
        let req = axum::http::Request::builder()
            .uri("/api/v1/browser/captcha-config")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["enabled"], false);
    }

    #[test]
    fn test_pending_oauth_flow_insert_and_retrieve() {
        let mut flows: HashMap<String, PendingOAuthFlow> = HashMap::new();
        let flow = PendingOAuthFlow {
            provider: crate::modules::oauth::token_manager::OAuthProvider {
                name: "google".into(),
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                client_id: "test-client".into(),
                client_secret: None,
                scopes: vec!["calendar".into()],
                redirect_uri: "http://localhost:3000/oauth/callback".into(),
            },
            code_verifier: "test-verifier".into(),
            created_at: 1000,
        };
        flows.insert("state_abc".into(), flow);
        assert!(flows.contains_key("state_abc"));
        let retrieved = flows.remove("state_abc").unwrap();
        assert_eq!(retrieved.provider.name, "google");
        assert_eq!(retrieved.code_verifier, "test-verifier");
        assert!(!flows.contains_key("state_abc"));
    }

    #[test]
    fn test_pending_oauth_flow_unknown_state() {
        let flows: HashMap<String, PendingOAuthFlow> = HashMap::new();
        assert!(flows.get("unknown_state").is_none());
    }

    #[test]
    fn test_oauth_callback_params_deserialize_with_code() {
        let json = r#"{"code":"auth_code_123","state":"state_abc"}"#;
        let params: OAuthCallbackParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.code, "auth_code_123");
        assert_eq!(params.state, Some("state_abc".into()));
        assert!(params.error.is_none());
    }

    #[test]
    fn test_oauth_cleanup_preserves_fresh_flows() {
        let pending: Arc<Mutex<HashMap<String, PendingOAuthFlow>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        pending.lock().unwrap().insert(
            "fresh".to_string(),
            PendingOAuthFlow {
                provider: crate::modules::oauth::token_manager::OAuthProvider {
                    name: "test".to_string(),
                    auth_url: String::new(),
                    token_url: String::new(),
                    client_id: String::new(),
                    client_secret: None,
                    scopes: vec![],
                    redirect_uri: String::new(),
                },
                code_verifier: "v".to_string(),
                created_at: now,
            },
        );
        let flows = pending.lock().unwrap();
        let fresh_count = flows
            .values()
            .filter(|f| now.saturating_sub(f.created_at) < 600)
            .count();
        assert_eq!(fresh_count, 1);
    }

    #[test]
    fn test_oauth_cleanup_removes_expired_flows() {
        let pending: Arc<Mutex<HashMap<String, PendingOAuthFlow>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        pending.lock().unwrap().insert(
            "expired".to_string(),
            PendingOAuthFlow {
                provider: crate::modules::oauth::token_manager::OAuthProvider {
                    name: "test".to_string(),
                    auth_url: String::new(),
                    token_url: String::new(),
                    client_id: String::new(),
                    client_secret: None,
                    scopes: vec![],
                    redirect_uri: String::new(),
                },
                code_verifier: "v".to_string(),
                created_at: now - 700,
            },
        );
        let mut flows = pending.lock().unwrap();
        flows.retain(|_, flow| now.saturating_sub(flow.created_at) < 600);
        assert!(flows.is_empty());
    }

    #[test]
    fn test_oauth_cleanup_mixed_flows() {
        let pending: Arc<Mutex<HashMap<String, PendingOAuthFlow>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let provider = crate::modules::oauth::token_manager::OAuthProvider {
            name: "test".to_string(),
            auth_url: String::new(),
            token_url: String::new(),
            client_id: String::new(),
            client_secret: None,
            scopes: vec![],
            redirect_uri: String::new(),
        };
        {
            let mut flows = pending.lock().unwrap();
            flows.insert(
                "fresh".to_string(),
                PendingOAuthFlow {
                    provider: provider.clone(),
                    code_verifier: "v1".to_string(),
                    created_at: now,
                },
            );
            flows.insert(
                "expired".to_string(),
                PendingOAuthFlow {
                    provider: provider.clone(),
                    code_verifier: "v2".to_string(),
                    created_at: now - 700,
                },
            );
        }
        let mut flows = pending.lock().unwrap();
        flows.retain(|_, flow| now.saturating_sub(flow.created_at) < 600);
        assert_eq!(flows.len(), 1);
        assert!(flows.contains_key("fresh"));
    }

    #[test]
    fn test_parse_agent_mention_with_query() {
        let (agent, query) = parse_agent_mention("@morning-briefing check my schedule");
        assert_eq!(agent.as_deref(), Some("morning-briefing"));
        assert_eq!(query, "check my schedule");
    }

    #[test]
    fn test_parse_agent_mention_no_mention() {
        let (agent, query) = parse_agent_mention("hello world");
        assert!(agent.is_none());
        assert_eq!(query, "hello world");
    }

    #[test]
    fn test_parse_agent_mention_only_agent() {
        let (agent, query) = parse_agent_mention("@my-agent");
        assert_eq!(agent.as_deref(), Some("my-agent"));
        assert_eq!(query, "");
    }

    #[test]
    fn test_parse_agent_mention_bare_at() {
        let (agent, query) = parse_agent_mention("@ something");
        assert!(agent.is_none());
        assert_eq!(query, "@ something");
    }

    #[test]
    fn test_confirmation_channel_roundtrip() {
        let pending: PendingConfirmations = Arc::new(Mutex::new(HashMap::new()));
        let (resp_tx, resp_rx) = std::sync::mpsc::channel::<ConfirmationResponse>();

        let req = ConfirmationRequest::new(
            "test-agent",
            "email.send:bob@example.com",
            "Test confirmation",
            crate::agent_os::confirmation::ConfirmationSource::Constitution {
                rule_name: "test_rule".into(),
            },
        );
        let req_id = req.id;

        pending.lock().unwrap().insert(req_id, PendingWebConfirmation {
            request: req,
            responder: resp_tx,
        });

        // Simulate browser POST /api/v1/confirm/{id}
        let responder = pending.lock().unwrap().remove(&req_id).map(|p| p.responder);
        assert!(responder.is_some());
        responder.unwrap().send(ConfirmationResponse::AllowOnce).unwrap();

        let response = resp_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(response, ConfirmationResponse::AllowOnce);
    }
}
