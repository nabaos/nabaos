// src/llm_router/function_library.rs
// Living function call library — SQLite-backed registry of all available functions
// with full JSON-schema-style definitions, lifecycle tracking, and proposal evaluation.
//
// This is a parallel metadata layer: FunctionRegistry provides rich schemas,
// while AbilityRegistry still handles actual dispatch. No existing code breaks.

use crate::core::error::{NyayaError, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core enums
// ---------------------------------------------------------------------------

/// Security classification for functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityTier {
    /// Pure reads, no side effects
    ReadOnly,
    /// Writes to local storage only
    LocalWrite,
    /// Reads from external services
    ExternalRead,
    /// Writes to external services (email, notifications)
    ExternalWrite,
    /// Dangerous operations requiring explicit user approval
    Critical,
}

impl SecurityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityTier::ReadOnly => "read_only",
            SecurityTier::LocalWrite => "local_write",
            SecurityTier::ExternalRead => "ext_read",
            SecurityTier::ExternalWrite => "ext_write",
            SecurityTier::Critical => "critical",
        }
    }

    pub fn parse_from(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(SecurityTier::ReadOnly),
            "local_write" => Some(SecurityTier::LocalWrite),
            "ext_read" | "external_read" => Some(SecurityTier::ExternalRead),
            "ext_write" | "external_write" => Some(SecurityTier::ExternalWrite),
            "critical" => Some(SecurityTier::Critical),
            _ => None,
        }
    }

    /// Whether this tier can be auto-accepted without user approval.
    pub fn auto_acceptable(&self) -> bool {
        matches!(
            self,
            SecurityTier::ReadOnly | SecurityTier::LocalWrite | SecurityTier::ExternalRead
        )
    }
}

/// Lifecycle state of a function definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionLifecycle {
    /// Proposed by LLM, not yet reviewed
    Proposed,
    /// Reviewed but not yet accepted
    Reviewed,
    /// Accepted and available for use
    Accepted,
    /// Proven reliable through usage (>50 calls, >95% success)
    Graduated,
    /// No longer active
    Deprecated,
}

impl FunctionLifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            FunctionLifecycle::Proposed => "proposed",
            FunctionLifecycle::Reviewed => "reviewed",
            FunctionLifecycle::Accepted => "accepted",
            FunctionLifecycle::Graduated => "graduated",
            FunctionLifecycle::Deprecated => "deprecated",
        }
    }

    pub fn parse_from(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(FunctionLifecycle::Proposed),
            "reviewed" => Some(FunctionLifecycle::Reviewed),
            "accepted" => Some(FunctionLifecycle::Accepted),
            "graduated" => Some(FunctionLifecycle::Graduated),
            "deprecated" => Some(FunctionLifecycle::Deprecated),
            _ => None,
        }
    }
}

/// Where this function definition originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionSource {
    /// Built-in core ability
    Core,
    /// Proposed by the LLM via MODE 6
    LlmProposed,
    /// Defined by the user
    UserDefined,
    /// From a skill package
    Skill,
}

impl FunctionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            FunctionSource::Core => "core",
            FunctionSource::LlmProposed => "llm_proposed",
            FunctionSource::UserDefined => "user_defined",
            FunctionSource::Skill => "skill",
        }
    }

    pub fn parse_from(s: &str) -> Option<Self> {
        match s {
            "core" => Some(FunctionSource::Core),
            "llm_proposed" => Some(FunctionSource::LlmProposed),
            "user_defined" => Some(FunctionSource::UserDefined),
            "skill" => Some(FunctionSource::Skill),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Schema types
// ---------------------------------------------------------------------------

/// Schema for a function parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSchema {
    pub name: String,
    pub description: String,
    /// "string", "number", "integer", "boolean"
    pub schema_type: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    /// For constrained values (e.g., priority: low|normal|high|urgent)
    pub enum_values: Vec<String>,
    /// Regex validation pattern
    pub pattern: Option<String>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

/// A single field in a return schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnField {
    pub name: String,
    pub description: String,
    pub schema_type: String,
}

/// Schema describing what a function returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnSchema {
    pub description: String,
    pub schema_type: String,
    pub fields: Vec<ReturnField>,
}

/// An example invocation of a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionExample {
    pub description: String,
    pub input: serde_json::Value,
    pub expected_output: serde_json::Value,
}

/// Complete function definition with schema, lifecycle, and usage stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub category: String,
    pub permission: String,
    pub version: u32,
    pub params: Vec<ParamSchema>,
    pub returns: ReturnSchema,
    pub examples: Vec<FunctionExample>,
    pub security_tier: SecurityTier,
    pub lifecycle: FunctionLifecycle,
    pub source: FunctionSource,
    pub proposed_by: String,
    pub call_count: u64,
    pub success_count: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl FunctionDef {
    pub fn success_rate(&self) -> f64 {
        if self.call_count == 0 {
            1.0
        } else {
            self.success_count as f64 / self.call_count as f64
        }
    }
}

// ---------------------------------------------------------------------------
// Proposal evaluation
// ---------------------------------------------------------------------------

/// Result of evaluating a proposed function.
#[derive(Debug)]
pub enum ProposalResult {
    /// Auto-accepted (low risk tier)
    AutoAccepted,
    /// Queued for user approval (high risk tier)
    QueuedForApproval { reason: String },
    /// Rejected outright
    Rejected { reason: String },
}

// ---------------------------------------------------------------------------
// FunctionRegistry — SQLite-backed
// ---------------------------------------------------------------------------

/// SQLite-backed registry of function definitions.
pub struct FunctionRegistry {
    conn: Connection,
}

impl FunctionRegistry {
    /// Open (or create) the function registry database.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("FunctionRegistry DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS functions (
                name TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                category TEXT NOT NULL,
                permission TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                params_json TEXT NOT NULL DEFAULT '[]',
                returns_json TEXT NOT NULL DEFAULT '{}',
                examples_json TEXT NOT NULL DEFAULT '[]',
                security_tier TEXT NOT NULL,
                lifecycle TEXT NOT NULL,
                source TEXT NOT NULL,
                proposed_by TEXT NOT NULL DEFAULT 'system',
                call_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("FunctionRegistry table creation failed: {}", e)))?;

        Ok(Self { conn })
    }

    /// Seed the 11 core ability function definitions.
    /// If a function already exists and the code version is newer, update metadata
    /// but preserve accumulated `call_count` and `success_count`.
    pub fn seed_core_functions(&self) -> Result<()> {
        let defs = core_function_defs();
        for def in defs {
            match self.lookup(&def.name)? {
                None => {
                    self.register(&def)?;
                }
                Some(existing) if def.version > existing.version => {
                    self.update_definition(&def, existing.call_count, existing.success_count)?;
                }
                _ => {
                    // Already exists at same or newer version — skip
                }
            }
        }
        Ok(())
    }

    /// Update a function definition while preserving usage stats.
    fn update_definition(
        &self,
        func: &FunctionDef,
        call_count: u64,
        success_count: u64,
    ) -> Result<()> {
        let params_json = serde_json::to_string(&func.params)
            .map_err(|e| NyayaError::Config(format!("Serialize params: {}", e)))?;
        let returns_json = serde_json::to_string(&func.returns)
            .map_err(|e| NyayaError::Config(format!("Serialize returns: {}", e)))?;
        let examples_json = serde_json::to_string(&func.examples)
            .map_err(|e| NyayaError::Config(format!("Serialize examples: {}", e)))?;
        let now = now_ms();

        self.conn
            .execute(
                "UPDATE functions SET description = ?1, category = ?2, permission = ?3,
             version = ?4, params_json = ?5, returns_json = ?6, examples_json = ?7,
             security_tier = ?8, lifecycle = ?9, source = ?10, proposed_by = ?11,
             call_count = ?12, success_count = ?13, updated_at = ?14
             WHERE name = ?15",
                params![
                    func.description,
                    func.category,
                    func.permission,
                    func.version,
                    params_json,
                    returns_json,
                    examples_json,
                    func.security_tier.as_str(),
                    func.lifecycle.as_str(),
                    func.source.as_str(),
                    func.proposed_by,
                    call_count as i64,
                    success_count as i64,
                    now,
                    func.name,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("Update definition '{}': {}", func.name, e)))?;

        Ok(())
    }

    /// Register a new function definition.
    /// SECURITY: Refuses to overwrite core functions. LLM-proposed functions
    /// cannot replace built-in abilities.
    pub fn register(&self, func: &FunctionDef) -> Result<()> {
        // H25: Prevent overwriting core built-in functions
        if func.source != FunctionSource::Core {
            if let Some(existing) = self.lookup(&func.name)? {
                if existing.source == FunctionSource::Core {
                    return Err(NyayaError::PermissionDenied(format!(
                        "Cannot overwrite core function '{}' with {} source",
                        func.name,
                        func.source.as_str()
                    )));
                }
            }
        }
        let params_json = serde_json::to_string(&func.params)
            .map_err(|e| NyayaError::Config(format!("Serialize params: {}", e)))?;
        let returns_json = serde_json::to_string(&func.returns)
            .map_err(|e| NyayaError::Config(format!("Serialize returns: {}", e)))?;
        let examples_json = serde_json::to_string(&func.examples)
            .map_err(|e| NyayaError::Config(format!("Serialize examples: {}", e)))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO functions
             (name, description, category, permission, version, params_json, returns_json,
              examples_json, security_tier, lifecycle, source, proposed_by,
              call_count, success_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    func.name,
                    func.description,
                    func.category,
                    func.permission,
                    func.version,
                    params_json,
                    returns_json,
                    examples_json,
                    func.security_tier.as_str(),
                    func.lifecycle.as_str(),
                    func.source.as_str(),
                    func.proposed_by,
                    func.call_count,
                    func.success_count,
                    func.created_at,
                    func.updated_at,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("Register function '{}': {}", func.name, e)))?;

        Ok(())
    }

    /// Look up a function by name.
    pub fn lookup(&self, name: &str) -> Result<Option<FunctionDef>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, category, permission, version, params_json, returns_json,
                    examples_json, security_tier, lifecycle, source, proposed_by,
                    call_count, success_count, created_at, updated_at
             FROM functions WHERE name = ?1"
        ).map_err(|e| NyayaError::Cache(format!("Prepare lookup: {}", e)))?;

        let result = stmt.query_row(params![name], |row| Ok(Self::row_to_func(row)));

        match result {
            Ok(func) => Ok(Some(func?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(NyayaError::Cache(format!("Lookup '{}': {}", name, e))),
        }
    }

    /// Check if a function exists.
    pub fn exists(&self, name: &str) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM functions WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Exists check: {}", e)))?;
        Ok(count > 0)
    }

    /// List all available functions (Accepted + Graduated).
    pub fn list_available(&self) -> Result<Vec<FunctionDef>> {
        self.list_where("lifecycle IN ('accepted', 'graduated')")
    }

    /// List core available functions — source = 'core' AND lifecycle in (accepted, graduated).
    /// These are functions with actual dispatch in AbilityRegistry.
    pub fn list_core_available(&self) -> Result<Vec<FunctionDef>> {
        self.list_where("source = 'core' AND lifecycle IN ('accepted', 'graduated')")
    }

    /// List proposed/non-core available functions — not core, but accepted or graduated.
    fn list_proposed_available(&self) -> Result<Vec<FunctionDef>> {
        self.list_where("source != 'core' AND lifecycle IN ('accepted', 'graduated')")
    }

    /// List functions by category.
    pub fn list_by_category(&self, category: &str) -> Result<Vec<FunctionDef>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, category, permission, version, params_json, returns_json,
                    examples_json, security_tier, lifecycle, source, proposed_by,
                    call_count, success_count, created_at, updated_at
             FROM functions WHERE category = ?1 ORDER BY name"
        ).map_err(|e| NyayaError::Cache(format!("Prepare list_by_category: {}", e)))?;

        let rows = stmt
            .query_map(params![category], |row| Ok(Self::row_to_func(row)))
            .map_err(|e| NyayaError::Cache(format!("list_by_category: {}", e)))?;

        let mut funcs = Vec::new();
        for row in rows {
            let func = row.map_err(|e| NyayaError::Cache(format!("Row read: {}", e)))??;
            funcs.push(func);
        }
        Ok(funcs)
    }

    /// List functions by lifecycle state.
    pub fn list_by_lifecycle(&self, state: FunctionLifecycle) -> Result<Vec<FunctionDef>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, category, permission, version, params_json, returns_json,
                    examples_json, security_tier, lifecycle, source, proposed_by,
                    call_count, success_count, created_at, updated_at
             FROM functions WHERE lifecycle = ?1 ORDER BY name"
        ).map_err(|e| NyayaError::Cache(format!("Prepare list_by_lifecycle: {}", e)))?;

        let rows = stmt
            .query_map(params![state.as_str()], |row| Ok(Self::row_to_func(row)))
            .map_err(|e| NyayaError::Cache(format!("list_by_lifecycle: {}", e)))?;

        let mut funcs = Vec::new();
        for row in rows {
            let func = row.map_err(|e| NyayaError::Cache(format!("Row read: {}", e)))??;
            funcs.push(func);
        }
        Ok(funcs)
    }

    /// Transition a function to a new lifecycle state.
    pub fn transition(&self, name: &str, new_state: FunctionLifecycle) -> Result<()> {
        let now = now_ms();
        let updated = self
            .conn
            .execute(
                "UPDATE functions SET lifecycle = ?1, updated_at = ?2 WHERE name = ?3",
                params![new_state.as_str(), now, name],
            )
            .map_err(|e| NyayaError::Cache(format!("Transition '{}': {}", name, e)))?;

        if updated == 0 {
            return Err(NyayaError::Config(format!(
                "Function '{}' not found for transition",
                name
            )));
        }
        Ok(())
    }

    /// Delete a function from the registry.
    /// Core functions cannot be deleted — use `transition()` to deprecate them instead.
    pub fn delete(&self, name: &str) -> Result<()> {
        // Guard: prevent deletion of core functions
        if let Some(func) = self.lookup(name)? {
            if func.source == FunctionSource::Core {
                return Err(NyayaError::Config(format!(
                    "Cannot delete core function '{}'. Use transition() to deprecate instead.",
                    name
                )));
            }
        }

        let deleted = self
            .conn
            .execute("DELETE FROM functions WHERE name = ?1", params![name])
            .map_err(|e| NyayaError::Cache(format!("Delete '{}': {}", name, e)))?;

        if deleted == 0 {
            return Err(NyayaError::Config(format!(
                "Function '{}' not found for deletion",
                name
            )));
        }
        Ok(())
    }

    /// Record a function call (increment call_count, optionally success_count).
    pub fn record_call(&self, name: &str, success: bool) -> Result<()> {
        let now = now_ms();
        if success {
            self.conn.execute(
                "UPDATE functions SET call_count = call_count + 1, success_count = success_count + 1, updated_at = ?1 WHERE name = ?2",
                params![now, name],
            )
        } else {
            self.conn.execute(
                "UPDATE functions SET call_count = call_count + 1, updated_at = ?1 WHERE name = ?2",
                params![now, name],
            )
        }.map_err(|e| NyayaError::Cache(format!("Record call '{}': {}", name, e)))?;
        Ok(())
    }

    /// Find functions eligible for graduation: >50 calls, >95% success, currently Accepted.
    pub fn graduation_candidates(&self) -> Result<Vec<FunctionDef>> {
        self.list_where(
            "lifecycle = 'accepted' AND call_count > 50 AND \
             CAST(success_count AS REAL) / CAST(call_count AS REAL) > 0.95",
        )
    }

    /// Generate compact prompt text for LLM injection.
    /// Format: ~40 tokens per function, designed for minimal token usage.
    /// Section 1: AVAILABLE FUNCTIONS (core, dispatchable)
    /// Section 2: PROPOSED FUNCTIONS (non-core, metadata only)
    pub fn to_prompt_text(&self) -> Result<String> {
        let core_funcs = self.list_core_available()?;
        let proposed_funcs = self.list_proposed_available()?;

        if core_funcs.is_empty() && proposed_funcs.is_empty() {
            return Ok(String::new());
        }

        let mut text = String::new();

        if !core_funcs.is_empty() {
            text.push_str("\nAVAILABLE FUNCTIONS:\n");
            for f in &core_funcs {
                text.push_str(&Self::format_func_line(f));
            }
        }

        if !proposed_funcs.is_empty() {
            text.push_str("\nPROPOSED FUNCTIONS (not yet callable):\n");
            for f in &proposed_funcs {
                text.push_str(&Self::format_func_line(f));
            }
        }

        text.push_str(
            "\nTo propose a new function, use MODE 6: PROPOSE_FUNC (see format above).\n",
        );
        Ok(text)
    }

    /// Format a single function line for prompt text.
    fn format_func_line(f: &FunctionDef) -> String {
        let params_str: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                let mut s = format!("{}:{}", p.name, p.schema_type);
                if p.required {
                    s.push('!');
                } else if let Some(ref def) = p.default {
                    s.push_str(&format!("={}", format_default(def)));
                }
                s
            })
            .collect();

        let return_fields: Vec<&str> = f.returns.fields.iter().map(|rf| rf.name.as_str()).collect();

        format!(
            "  {}({}) -> {{{}}} [{}]\n",
            f.name,
            params_str.join(", "),
            return_fields.join(","),
            f.security_tier.as_str(),
        )
    }

    /// Convert core available functions to OpenAI-compatible tools JSON.
    /// Only includes core functions with actual dispatch.
    pub fn to_openai_tools(&self) -> Result<Vec<serde_json::Value>> {
        let funcs = self.list_core_available()?;
        let mut tools = Vec::new();

        for f in &funcs {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for p in &f.params {
                let mut prop = serde_json::Map::new();
                prop.insert(
                    "type".into(),
                    serde_json::Value::String(openai_type(&p.schema_type).to_string()),
                );
                prop.insert(
                    "description".into(),
                    serde_json::Value::String(p.description.clone()),
                );
                if !p.enum_values.is_empty() {
                    prop.insert(
                        "enum".into(),
                        serde_json::Value::Array(
                            p.enum_values
                                .iter()
                                .map(|v| serde_json::Value::String(v.clone()))
                                .collect(),
                        ),
                    );
                }
                if let Some(ref def) = p.default {
                    prop.insert("default".into(), def.clone());
                }
                properties.insert(p.name.clone(), serde_json::Value::Object(prop));
                if p.required {
                    required.push(serde_json::Value::String(p.name.clone()));
                }
            }

            let tool = serde_json::json!({
                "type": "function",
                "function": {
                    "name": f.name,
                    "description": f.description,
                    "parameters": {
                        "type": "object",
                        "properties": properties,
                        "required": required,
                    }
                }
            });
            tools.push(tool);
        }

        Ok(tools)
    }

    /// Convert core available functions to Claude-compatible tools JSON.
    /// Only includes core functions with actual dispatch.
    pub fn to_claude_tools(&self) -> Result<Vec<serde_json::Value>> {
        let funcs = self.list_core_available()?;
        let mut tools = Vec::new();

        for f in &funcs {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for p in &f.params {
                let mut prop = serde_json::Map::new();
                prop.insert(
                    "type".into(),
                    serde_json::Value::String(openai_type(&p.schema_type).to_string()),
                );
                prop.insert(
                    "description".into(),
                    serde_json::Value::String(p.description.clone()),
                );
                if !p.enum_values.is_empty() {
                    prop.insert(
                        "enum".into(),
                        serde_json::Value::Array(
                            p.enum_values
                                .iter()
                                .map(|v| serde_json::Value::String(v.clone()))
                                .collect(),
                        ),
                    );
                }
                properties.insert(p.name.clone(), serde_json::Value::Object(prop));
                if p.required {
                    required.push(serde_json::Value::String(p.name.clone()));
                }
            }

            let tool = serde_json::json!({
                "name": f.name,
                "description": f.description,
                "input_schema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                }
            });
            tools.push(tool);
        }

        Ok(tools)
    }

    /// Evaluate a proposed function definition.
    pub fn evaluate_proposal(&self, func: &FunctionDef) -> Result<ProposalResult> {
        // Check name format: must be category.name
        if !func.name.contains('.') {
            return Ok(ProposalResult::Rejected {
                reason: format!("Name '{}' must follow category.name convention", func.name),
            });
        }

        // Check for duplicate
        if self.exists(&func.name)? {
            return Ok(ProposalResult::Rejected {
                reason: format!("Function '{}' already exists", func.name),
            });
        }

        // Validate schema
        if func.description.is_empty() {
            return Ok(ProposalResult::Rejected {
                reason: "Description cannot be empty".into(),
            });
        }

        // Security tier determines auto-accept vs. queue
        if func.security_tier.auto_acceptable() {
            Ok(ProposalResult::AutoAccepted)
        } else {
            Ok(ProposalResult::QueuedForApproval {
                reason: format!(
                    "Security tier '{}' requires user approval",
                    func.security_tier.as_str()
                ),
            })
        }
    }

    // --- Internal helpers ---

    /// SAFETY: `where_clause` is interpolated directly into SQL.
    /// This method is private and all callers use compile-time string literals.
    /// NEVER pass user-controlled input as `where_clause`.
    fn list_where(&self, where_clause: &str) -> Result<Vec<FunctionDef>> {
        // Guard: reject obvious injection patterns in debug builds
        debug_assert!(
            !where_clause.contains("--")
                && !where_clause.contains(';')
                && !where_clause.contains("DROP"),
            "list_where received suspicious clause: {}",
            where_clause
        );
        let sql = format!(
            "SELECT name, description, category, permission, version, params_json, returns_json,
                    examples_json, security_tier, lifecycle, source, proposed_by,
                    call_count, success_count, created_at, updated_at
             FROM functions WHERE {} ORDER BY name",
            where_clause
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| NyayaError::Cache(format!("Prepare list: {}", e)))?;

        let rows = stmt
            .query_map([], |row| Ok(Self::row_to_func(row)))
            .map_err(|e| NyayaError::Cache(format!("Query list: {}", e)))?;

        let mut funcs = Vec::new();
        for row in rows {
            let func = row.map_err(|e| NyayaError::Cache(format!("Row read: {}", e)))??;
            funcs.push(func);
        }
        Ok(funcs)
    }

    fn row_to_func(row: &rusqlite::Row) -> Result<FunctionDef> {
        let params_json: String = row
            .get(5)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;
        let returns_json: String = row
            .get(6)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;
        let examples_json: String = row
            .get(7)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;
        let tier_str: String = row
            .get(8)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;
        let lifecycle_str: String = row
            .get(9)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;
        let source_str: String = row
            .get(10)
            .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?;

        Ok(FunctionDef {
            name: row
                .get(0)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            description: row
                .get(1)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            category: row
                .get(2)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            permission: row
                .get(3)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            version: row
                .get::<_, u32>(4)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            params: serde_json::from_str(&params_json).unwrap_or_default(),
            returns: serde_json::from_str(&returns_json).unwrap_or(ReturnSchema {
                description: String::new(),
                schema_type: "object".into(),
                fields: vec![],
            }),
            examples: serde_json::from_str(&examples_json).unwrap_or_default(),
            security_tier: SecurityTier::parse_from(&tier_str).unwrap_or(SecurityTier::Critical),
            lifecycle: FunctionLifecycle::parse_from(&lifecycle_str)
                .unwrap_or(FunctionLifecycle::Proposed),
            source: FunctionSource::parse_from(&source_str).unwrap_or(FunctionSource::Core),
            proposed_by: row
                .get(11)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            call_count: row
                .get::<_, i64>(12)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?
                as u64,
            success_count: row
                .get::<_, i64>(13)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?
                as u64,
            created_at: row
                .get(14)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
            updated_at: row
                .get(15)
                .map_err(|e| NyayaError::Cache(format!("Row field: {}", e)))?,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn format_default(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        other => other.to_string(),
    }
}

fn openai_type(schema_type: &str) -> &str {
    match schema_type {
        "string" | "str" => "string",
        "integer" | "int" => "integer",
        "number" | "float" => "number",
        "boolean" | "bool" => "boolean",
        "array" => "array",
        "object" => "object",
        _ => "string",
    }
}

// ---------------------------------------------------------------------------
// Core function definitions (the 11 abilities)
// ---------------------------------------------------------------------------

/// Returns full FunctionDef schemas for all 11 core abilities.
pub fn core_function_defs() -> Vec<FunctionDef> {
    let now = now_ms();

    vec![
        FunctionDef {
            name: "storage.get".into(),
            description: "Read a value from the agent's scoped key-value store".into(),
            category: "storage".into(),
            permission: "storage.get".into(),
            version: 1,
            params: vec![ParamSchema {
                name: "key".into(),
                description: "The key to retrieve".into(),
                schema_type: "string".into(),
                required: true,
                default: None,
                enum_values: vec![],
                pattern: None,
                minimum: None,
                maximum: None,
            }],
            returns: ReturnSchema {
                description: "The stored value".into(),
                schema_type: "object".into(),
                fields: vec![ReturnField {
                    name: "value".into(),
                    description: "The stored value".into(),
                    schema_type: "string".into(),
                }],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "storage.set".into(),
            description: "Write a value to the agent's scoped key-value store".into(),
            category: "storage".into(),
            permission: "storage.set".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "key".into(),
                    description: "The key to store".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "value".into(),
                    description: "The value to store".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Storage operation result".into(),
                schema_type: "object".into(),
                fields: vec![ReturnField {
                    name: "status".into(),
                    description: "Operation status".into(),
                    schema_type: "string".into(),
                }],
            },
            examples: vec![],
            security_tier: SecurityTier::LocalWrite,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "data.fetch_url".into(),
            description: "Fetch data from a URL (HTTP GET/POST with SSRF protection)".into(),
            category: "data".into(),
            permission: "data.fetch_url".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "url".into(),
                    description: "URL to fetch (http/https only)".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: Some(r"^https?://".into()),
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "method".into(),
                    description: "HTTP method".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: Some(serde_json::Value::String("GET".into())),
                    enum_values: vec!["GET".into(), "POST".into()],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Fetch request status".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "status".into(),
                        description: "Request status".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "url".into(),
                        description: "Requested URL".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "host".into(),
                        description: "Resolved host".into(),
                        schema_type: "string".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "nlp.sentiment".into(),
            description: "Analyze sentiment of text (positive/negative/neutral)".into(),
            category: "nlp".into(),
            permission: "nlp.sentiment".into(),
            version: 1,
            params: vec![ParamSchema {
                name: "text".into(),
                description: "Text to analyze".into(),
                schema_type: "string".into(),
                required: true,
                default: None,
                enum_values: vec![],
                pattern: None,
                minimum: None,
                maximum: None,
            }],
            returns: ReturnSchema {
                description: "Sentiment analysis result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "sentiment".into(),
                        description: "Sentiment label".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "score".into(),
                        description: "Sentiment score (-1 to 1)".into(),
                        schema_type: "number".into(),
                    },
                    ReturnField {
                        name: "confidence".into(),
                        description: "Confidence (0 to 1)".into(),
                        schema_type: "number".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "nlp.summarize".into(),
            description: "Summarize text by extracting key sentences".into(),
            category: "nlp".into(),
            permission: "nlp.summarize".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "text".into(),
                    description: "Text to summarize".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "max_sentences".into(),
                    description: "Maximum sentences in summary".into(),
                    schema_type: "integer".into(),
                    required: false,
                    default: Some(serde_json::json!(3)),
                    enum_values: vec![],
                    pattern: None,
                    minimum: Some(1.0),
                    maximum: Some(20.0),
                },
            ],
            returns: ReturnSchema {
                description: "Summarization result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "summary".into(),
                        description: "Summarized text".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "original_sentence_count".into(),
                        description: "Sentences in original".into(),
                        schema_type: "integer".into(),
                    },
                    ReturnField {
                        name: "compression_ratio".into(),
                        description: "Compression ratio".into(),
                        schema_type: "string".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "notify.user".into(),
            description: "Send a notification to the user".into(),
            category: "notify".into(),
            permission: "notify.user".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "message".into(),
                    description: "Notification message".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "priority".into(),
                    description: "Notification priority".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: Some(serde_json::Value::String("normal".into())),
                    enum_values: vec![
                        "low".into(),
                        "normal".into(),
                        "high".into(),
                        "urgent".into(),
                    ],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "channel".into(),
                    description: "Notification channel".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: Some(serde_json::Value::String("default".into())),
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Notification result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "status".into(),
                        description: "Delivery status".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "timestamp".into(),
                        description: "Delivery timestamp".into(),
                        schema_type: "integer".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalWrite,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "flow.branch".into(),
            description: "Conditional branching — evaluate a condition and return the branch taken"
                .into(),
            category: "flow".into(),
            permission: "flow.branch".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "condition".into(),
                    description: "Condition type".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![
                        "equals".into(),
                        "not_equals".into(),
                        "contains".into(),
                        "is_empty".into(),
                        "is_not_empty".into(),
                        "gt".into(),
                        "lt".into(),
                        "gte".into(),
                    ],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "value".into(),
                    description: "Value to test".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "threshold".into(),
                    description: "Threshold to compare against".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Branch evaluation result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "branch".into(),
                        description: "Branch taken (true/false)".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "result".into(),
                        description: "Boolean result".into(),
                        schema_type: "boolean".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "flow.stop".into(),
            description: "Stop workflow execution".into(),
            category: "flow".into(),
            permission: "flow.stop".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Stop result".into(),
                schema_type: "object".into(),
                fields: vec![ReturnField {
                    name: "status".into(),
                    description: "Always 'stopped'".into(),
                    schema_type: "string".into(),
                }],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "schedule.delay".into(),
            description: "Delay execution for a specified duration".into(),
            category: "schedule".into(),
            permission: "schedule.delay".into(),
            version: 1,
            params: vec![ParamSchema {
                name: "duration".into(),
                description: "Delay duration (e.g., 5s, 1m, 1h)".into(),
                schema_type: "string".into(),
                required: true,
                default: None,
                enum_values: vec![],
                pattern: Some(r"^\d+(ms|s|m|h)$".into()),
                minimum: None,
                maximum: None,
            }],
            returns: ReturnSchema {
                description: "Delay result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "status".into(),
                        description: "completed or scheduled".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "delay_ms".into(),
                        description: "Delay in milliseconds".into(),
                        schema_type: "integer".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::LocalWrite,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "email.send".into(),
            description: "Send an email (validates address, queues for delivery)".into(),
            category: "email".into(),
            permission: "email.send".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "to".into(),
                    description: "Recipient email address".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: Some(r".+@.+\..+".into()),
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "subject".into(),
                    description: "Email subject line".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "body".into(),
                    description: "Email body text".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: None,
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Email send result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "status".into(),
                        description: "Queue status".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "to".into(),
                        description: "Recipient".into(),
                        schema_type: "string".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalWrite,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
        FunctionDef {
            name: "trading.get_price".into(),
            description: "Fetch current price of a trading instrument".into(),
            category: "trading".into(),
            permission: "trading.get_price".into(),
            version: 1,
            params: vec![
                ParamSchema {
                    name: "symbol".into(),
                    description: "Trading symbol (e.g., AAPL, BTC/USD)".into(),
                    schema_type: "string".into(),
                    required: true,
                    default: None,
                    enum_values: vec![],
                    pattern: Some(r"^[A-Za-z0-9/\-\.]{1,10}$".into()),
                    minimum: None,
                    maximum: None,
                },
                ParamSchema {
                    name: "exchange".into(),
                    description: "Exchange to query".into(),
                    schema_type: "string".into(),
                    required: false,
                    default: Some(serde_json::Value::String("auto".into())),
                    enum_values: vec![],
                    pattern: None,
                    minimum: None,
                    maximum: None,
                },
            ],
            returns: ReturnSchema {
                description: "Price query result".into(),
                schema_type: "object".into(),
                fields: vec![
                    ReturnField {
                        name: "status".into(),
                        description: "Query status".into(),
                        schema_type: "string".into(),
                    },
                    ReturnField {
                        name: "symbol".into(),
                        description: "Normalized symbol".into(),
                        schema_type: "string".into(),
                    },
                ],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::Core,
            proposed_by: "system".into(),
            call_count: 0,
            success_count: 0,
            created_at: now,
            updated_at: now,
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> FunctionRegistry {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("functions.db");
        let reg = FunctionRegistry::open(&db_path).unwrap();
        reg.seed_core_functions().unwrap();
        // Keep tempdir alive by leaking it (test only)
        std::mem::forget(dir);
        reg
    }

    #[test]
    fn test_seed_core_functions() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("functions.db");
        let reg = FunctionRegistry::open(&db_path).unwrap();
        reg.seed_core_functions().unwrap();

        // All 11 should exist
        assert!(reg.exists("storage.get").unwrap());
        assert!(reg.exists("storage.set").unwrap());
        assert!(reg.exists("data.fetch_url").unwrap());
        assert!(reg.exists("nlp.sentiment").unwrap());
        assert!(reg.exists("nlp.summarize").unwrap());
        assert!(reg.exists("notify.user").unwrap());
        assert!(reg.exists("flow.branch").unwrap());
        assert!(reg.exists("flow.stop").unwrap());
        assert!(reg.exists("schedule.delay").unwrap());
        assert!(reg.exists("email.send").unwrap());
        assert!(reg.exists("trading.get_price").unwrap());
    }

    #[test]
    fn test_seed_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("functions.db");
        let reg = FunctionRegistry::open(&db_path).unwrap();
        reg.seed_core_functions().unwrap();
        reg.seed_core_functions().unwrap(); // Should not fail or duplicate
        let funcs = reg.list_available().unwrap();
        assert_eq!(funcs.len(), 11);
    }

    #[test]
    fn test_lookup() {
        let reg = test_registry();
        let func = reg.lookup("nlp.sentiment").unwrap().unwrap();
        assert_eq!(func.name, "nlp.sentiment");
        assert_eq!(func.category, "nlp");
        assert_eq!(func.security_tier, SecurityTier::ReadOnly);
        assert_eq!(func.params.len(), 1);
        assert_eq!(func.params[0].name, "text");
        assert!(func.params[0].required);
    }

    #[test]
    fn test_lookup_nonexistent() {
        let reg = test_registry();
        assert!(reg.lookup("nonexistent.func").unwrap().is_none());
    }

    #[test]
    fn test_list_available() {
        let reg = test_registry();
        let funcs = reg.list_available().unwrap();
        assert_eq!(funcs.len(), 11);
    }

    #[test]
    fn test_list_by_category() {
        let reg = test_registry();
        let nlp = reg.list_by_category("nlp").unwrap();
        assert_eq!(nlp.len(), 2);
        let flow = reg.list_by_category("flow").unwrap();
        assert_eq!(flow.len(), 2);
        let storage = reg.list_by_category("storage").unwrap();
        assert_eq!(storage.len(), 2);
    }

    #[test]
    fn test_list_by_lifecycle() {
        let reg = test_registry();
        let accepted = reg.list_by_lifecycle(FunctionLifecycle::Accepted).unwrap();
        assert_eq!(accepted.len(), 11);
        let proposed = reg.list_by_lifecycle(FunctionLifecycle::Proposed).unwrap();
        assert_eq!(proposed.len(), 0);
    }

    #[test]
    fn test_lifecycle_transition() {
        let reg = test_registry();
        reg.transition("storage.get", FunctionLifecycle::Graduated)
            .unwrap();
        let func = reg.lookup("storage.get").unwrap().unwrap();
        assert_eq!(func.lifecycle, FunctionLifecycle::Graduated);
    }

    #[test]
    fn test_lifecycle_transition_nonexistent() {
        let reg = test_registry();
        let result = reg.transition("nonexistent.func", FunctionLifecycle::Deprecated);
        assert!(result.is_err());
    }

    #[test]
    fn test_record_call() {
        let reg = test_registry();
        reg.record_call("nlp.sentiment", true).unwrap();
        reg.record_call("nlp.sentiment", true).unwrap();
        reg.record_call("nlp.sentiment", false).unwrap();

        let func = reg.lookup("nlp.sentiment").unwrap().unwrap();
        assert_eq!(func.call_count, 3);
        assert_eq!(func.success_count, 2);
        assert!((func.success_rate() - 0.6667).abs() < 0.01);
    }

    #[test]
    fn test_graduation_candidates() {
        let reg = test_registry();

        // No candidates yet (0 calls)
        assert_eq!(reg.graduation_candidates().unwrap().len(), 0);

        // Simulate 60 calls, 58 successes for storage.get
        for _ in 0..58 {
            reg.record_call("storage.get", true).unwrap();
        }
        for _ in 0..2 {
            reg.record_call("storage.get", false).unwrap();
        }
        // 58/60 = 96.7% > 95%, > 50 calls
        let candidates = reg.graduation_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "storage.get");
    }

    #[test]
    fn test_prompt_text_generation() {
        let reg = test_registry();
        let text = reg.to_prompt_text().unwrap();

        assert!(text.contains("AVAILABLE FUNCTIONS:"));
        assert!(text.contains("storage.get("));
        assert!(text.contains("nlp.sentiment("));
        assert!(text.contains("[read_only]"));
        assert!(text.contains("[ext_read]"));
        assert!(text.contains("[ext_write]"));
        assert!(text.contains("PROPOSE_FUNC"));

        // Check token efficiency: should be under 500 tokens for 11 functions
        // Rough token estimate: ~4 chars per token
        let estimated_tokens = text.len() / 4;
        assert!(
            estimated_tokens < 500,
            "Prompt text too long: ~{} tokens",
            estimated_tokens
        );
    }

    #[test]
    fn test_proposal_evaluation_valid() {
        let reg = test_registry();
        let func = FunctionDef {
            name: "calendar.list_events".into(),
            description: "List calendar events".into(),
            category: "calendar".into(),
            permission: "calendar.list_events".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Events list".into(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Proposed,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };

        match reg.evaluate_proposal(&func).unwrap() {
            ProposalResult::AutoAccepted => {} // ExternalRead auto-accepts
            other => panic!("Expected AutoAccepted, got {:?}", other),
        }
    }

    #[test]
    fn test_proposal_evaluation_requires_approval() {
        let reg = test_registry();
        let func = FunctionDef {
            name: "notify.slack".into(),
            description: "Send a Slack message".into(),
            category: "notify".into(),
            permission: "notify.slack".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Result".into(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalWrite,
            lifecycle: FunctionLifecycle::Proposed,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };

        match reg.evaluate_proposal(&func).unwrap() {
            ProposalResult::QueuedForApproval { .. } => {}
            other => panic!("Expected QueuedForApproval, got {:?}", other),
        }
    }

    #[test]
    fn test_proposal_evaluation_duplicate() {
        let reg = test_registry();
        let func = FunctionDef {
            name: "storage.get".into(), // Already exists
            description: "Duplicate".into(),
            category: "storage".into(),
            permission: "storage.get".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: String::new(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Proposed,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };

        match reg.evaluate_proposal(&func).unwrap() {
            ProposalResult::Rejected { reason } => {
                assert!(reason.contains("already exists"));
            }
            other => panic!("Expected Rejected, got {:?}", other),
        }
    }

    #[test]
    fn test_proposal_evaluation_bad_name() {
        let reg = test_registry();
        let func = FunctionDef {
            name: "no_dot_name".into(), // Missing category.name convention
            description: "Bad name".into(),
            category: "misc".into(),
            permission: "misc".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: String::new(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ReadOnly,
            lifecycle: FunctionLifecycle::Proposed,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };

        match reg.evaluate_proposal(&func).unwrap() {
            ProposalResult::Rejected { reason } => {
                assert!(reason.contains("category.name"));
            }
            other => panic!("Expected Rejected, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_tools_format() {
        let reg = test_registry();
        let tools = reg.to_openai_tools().unwrap();
        assert_eq!(tools.len(), 11);

        // Check structure of first tool
        let tool = &tools[0];
        assert_eq!(tool["type"], "function");
        assert!(tool["function"]["name"].is_string());
        assert!(tool["function"]["parameters"]["type"] == "object");
    }

    #[test]
    fn test_claude_tools_format() {
        let reg = test_registry();
        let tools = reg.to_claude_tools().unwrap();
        assert_eq!(tools.len(), 11);

        let tool = &tools[0];
        assert!(tool["name"].is_string());
        assert!(tool["input_schema"]["type"] == "object");
    }

    #[test]
    fn test_security_tier_auto_acceptable() {
        assert!(SecurityTier::ReadOnly.auto_acceptable());
        assert!(SecurityTier::LocalWrite.auto_acceptable());
        assert!(SecurityTier::ExternalRead.auto_acceptable());
        assert!(!SecurityTier::ExternalWrite.auto_acceptable());
        assert!(!SecurityTier::Critical.auto_acceptable());
    }

    #[test]
    fn test_delete_core_function_blocked() {
        let reg = test_registry();
        assert!(reg.exists("storage.get").unwrap());
        let result = reg.delete("storage.get");
        assert!(result.is_err(), "Should not allow deleting core functions");
        assert!(result.unwrap_err().to_string().contains("core function"));
        // Function should still exist
        assert!(reg.exists("storage.get").unwrap());
    }

    #[test]
    fn test_delete_proposed_function_allowed() {
        let reg = test_registry();
        let proposed = FunctionDef {
            name: "calendar.list_events".into(),
            description: "List calendar events".into(),
            category: "calendar".into(),
            permission: "calendar.list_events".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Events".into(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };
        reg.register(&proposed).unwrap();
        assert!(reg.exists("calendar.list_events").unwrap());
        reg.delete("calendar.list_events").unwrap();
        assert!(!reg.exists("calendar.list_events").unwrap());
    }

    #[test]
    fn test_delete_nonexistent_fails() {
        let reg = test_registry();
        let result = reg.delete("nonexistent.func");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_security_tier_defaults_to_critical() {
        let reg = test_registry();
        // Manually insert a row with an unknown security tier
        reg.conn
            .execute(
                "INSERT INTO functions (name, description, category, permission, version,
             params_json, returns_json, examples_json, security_tier, lifecycle,
             source, proposed_by, call_count, success_count, created_at, updated_at)
             VALUES ('test.unknown_tier', 'test', 'test', 'test', 1,
             '[]', '{}', '[]', 'nonexistent_tier', 'accepted',
             'core', 'system', 0, 0, 0, 0)",
                [],
            )
            .unwrap();

        let func = reg.lookup("test.unknown_tier").unwrap().unwrap();
        assert_eq!(func.security_tier, SecurityTier::Critical);
    }

    #[test]
    fn test_seed_updates_on_version_bump() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("functions.db");
        let reg = FunctionRegistry::open(&db_path).unwrap();
        reg.seed_core_functions().unwrap();

        // Simulate usage stats
        reg.record_call("storage.get", true).unwrap();
        reg.record_call("storage.get", true).unwrap();
        reg.record_call("storage.get", false).unwrap();

        let before = reg.lookup("storage.get").unwrap().unwrap();
        assert_eq!(before.call_count, 3);
        assert_eq!(before.success_count, 2);

        // Simulate a version bump by directly updating the version in the DB to 0
        // so the code's version (1) is higher
        reg.conn
            .execute(
                "UPDATE functions SET version = 0 WHERE name = 'storage.get'",
                [],
            )
            .unwrap();

        // Re-seed — should update metadata but preserve stats
        reg.seed_core_functions().unwrap();

        let after = reg.lookup("storage.get").unwrap().unwrap();
        assert_eq!(after.call_count, 3, "call_count should be preserved");
        assert_eq!(after.success_count, 2, "success_count should be preserved");
        assert_eq!(after.version, 1, "version should be updated");
    }

    #[test]
    fn test_prompt_text_separates_proposed_from_core() {
        let reg = test_registry();

        // Register a proposed function
        let proposed = FunctionDef {
            name: "calendar.list_events".into(),
            description: "List calendar events".into(),
            category: "calendar".into(),
            permission: "calendar.list_events".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Events".into(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };
        reg.register(&proposed).unwrap();

        let text = reg.to_prompt_text().unwrap();
        assert!(text.contains("AVAILABLE FUNCTIONS:"));
        assert!(text.contains("PROPOSED FUNCTIONS (not yet callable):"));
        assert!(text.contains("storage.get("));
        assert!(text.contains("calendar.list_events("));
    }

    #[test]
    fn test_openai_tools_excludes_proposed() {
        let reg = test_registry();

        // Register a proposed function
        let proposed = FunctionDef {
            name: "calendar.list_events".into(),
            description: "List calendar events".into(),
            category: "calendar".into(),
            permission: "calendar.list_events".into(),
            version: 1,
            params: vec![],
            returns: ReturnSchema {
                description: "Events".into(),
                schema_type: "object".into(),
                fields: vec![],
            },
            examples: vec![],
            security_tier: SecurityTier::ExternalRead,
            lifecycle: FunctionLifecycle::Accepted,
            source: FunctionSource::LlmProposed,
            proposed_by: "llm".into(),
            call_count: 0,
            success_count: 0,
            created_at: 0,
            updated_at: 0,
        };
        reg.register(&proposed).unwrap();

        let tools = reg.to_openai_tools().unwrap();
        assert_eq!(tools.len(), 11); // Only core functions
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"calendar.list_events"));
    }

    #[test]
    fn test_deprecated_not_available() {
        let reg = test_registry();
        reg.transition("flow.stop", FunctionLifecycle::Deprecated)
            .unwrap();
        let available = reg.list_available().unwrap();
        assert_eq!(available.len(), 10); // One less
        assert!(!available.iter().any(|f| f.name == "flow.stop"));
    }
}
