use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Lease types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseStatus {
    Active,
    Expired,
    Revoked,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseQuota {
    pub max_cost_usd: Option<f64>,
    pub max_calls: Option<u64>,
    pub max_duration_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLease {
    pub lease_id: String,
    pub resource_id: String,
    pub agent_id: String,
    pub capabilities: Vec<super::ResourceCapability>,
    pub quota: LeaseQuota,
    pub used_cost_usd: f64,
    pub used_calls: u64,
    pub started_at: i64,
    pub expires_at: Option<i64>,
    pub status: LeaseStatus,
}

// ---------------------------------------------------------------------------
// ResourceRecord — flat struct for DB rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRecord {
    pub id: String,
    pub name: String,
    pub resource_type: super::ResourceType,
    pub status: super::ResourceStatus,
    pub cost_model: Option<super::CostModel>,
    pub metadata: HashMap<String, String>,
    pub config_json: String,
    pub registered_at: i64,
    pub last_health_check: Option<i64>,
}

impl ResourceRecord {
    pub fn resource_type_display(&self) -> &str {
        match self.resource_type {
            super::ResourceType::Compute => "compute",
            super::ResourceType::Financial => "financial",
            super::ResourceType::Device => "device",
            super::ResourceType::ApiService => "api_service",
        }
    }

    pub fn status_display(&self) -> String {
        match &self.status {
            super::ResourceStatus::Available => "available".to_string(),
            super::ResourceStatus::InUse { agent_id } => format!("in_use:{}", agent_id),
            super::ResourceStatus::Provisioning => "provisioning".to_string(),
            super::ResourceStatus::Degraded => "degraded".to_string(),
            super::ResourceStatus::Offline => "offline".to_string(),
            super::ResourceStatus::Terminated => "terminated".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct ResourceRegistry {
    conn: rusqlite::Connection,
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl ResourceRegistry {
    /// Open (or create) the registry database at the given path.
    /// Pass `Path::new(":memory:")` for an in-memory database.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS resources (
                id                TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                resource_type     TEXT NOT NULL,
                status_json       TEXT NOT NULL DEFAULT '\"available\"',
                cost_model_json   TEXT,
                metadata_json     TEXT NOT NULL DEFAULT '{}',
                config_json       TEXT NOT NULL DEFAULT '{}',
                registered_at     INTEGER NOT NULL,
                last_health_check INTEGER
            );

            CREATE TABLE IF NOT EXISTS resource_leases (
                lease_id          TEXT PRIMARY KEY,
                resource_id       TEXT NOT NULL,
                agent_id          TEXT NOT NULL,
                capabilities_json TEXT NOT NULL,
                quota_json        TEXT NOT NULL,
                used_cost_usd     REAL NOT NULL DEFAULT 0,
                used_calls        INTEGER NOT NULL DEFAULT 0,
                started_at        INTEGER NOT NULL,
                expires_at        INTEGER,
                status            TEXT NOT NULL DEFAULT 'active'
            );

            CREATE TABLE IF NOT EXISTS resource_usage_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                lease_id    TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                agent_id    TEXT NOT NULL,
                action      TEXT NOT NULL,
                cost_usd    REAL NOT NULL,
                detail      TEXT,
                timestamp   INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS financial_transactions (
                tx_id           TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL,
                agent_id        TEXT NOT NULL,
                lease_id        TEXT NOT NULL,
                operation       TEXT NOT NULL,
                amount          REAL,
                currency        TEXT NOT NULL,
                status          TEXT NOT NULL,
                approval_method TEXT,
                timestamp       INTEGER NOT NULL
            );",
        )?;

        Ok(Self { conn })
    }

    // -----------------------------------------------------------------------
    // Resource CRUD
    // -----------------------------------------------------------------------

    pub fn register(
        &self,
        id: &str,
        name: &str,
        resource_type: &super::ResourceType,
        config_json: &str,
    ) -> Result<()> {
        let type_json = serde_json::to_string(resource_type)?;
        let status_json = serde_json::to_string(&super::ResourceStatus::Available)?;
        let now = now_epoch();

        self.conn.execute(
            "INSERT INTO resources
                (id, name, resource_type, status_json, cost_model_json, metadata_json, config_json, registered_at, last_health_check)
             VALUES (?1, ?2, ?3, ?4, NULL, '{}', ?5, ?6, NULL)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, resource_type=excluded.resource_type, config_json=excluded.config_json",
            rusqlite::params![id, name, type_json, status_json, config_json, now],
        )?;

        Ok(())
    }

    pub fn unregister(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM resources WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    pub fn get_resource(&self, id: &str) -> Result<Option<ResourceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, resource_type, status_json, cost_model_json,
                    metadata_json, config_json, registered_at, last_health_check
             FROM resources WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![id], row_to_resource_record)?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_resources(&self) -> Result<Vec<ResourceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, resource_type, status_json, cost_model_json,
                    metadata_json, config_json, registered_at, last_health_check
             FROM resources",
        )?;

        let rows = stmt.query_map([], row_to_resource_record)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn update_status(&self, id: &str, status: &super::ResourceStatus) -> Result<()> {
        let status_json = serde_json::to_string(status)?;
        self.conn.execute(
            "UPDATE resources SET status_json = ?1 WHERE id = ?2",
            rusqlite::params![status_json, id],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Lease management
    // -----------------------------------------------------------------------

    pub fn acquire_lease(
        &self,
        agent_id: &str,
        resource_id: &str,
        capabilities: &[super::ResourceCapability],
        quota: LeaseQuota,
    ) -> Result<ResourceLease> {
        let lease_id = uuid::Uuid::new_v4().to_string();
        let now = now_epoch();
        let capabilities_json = serde_json::to_string(capabilities)?;
        let quota_json = serde_json::to_string(&quota)?;

        let expires_at: Option<i64> = quota.max_duration_secs.map(|d| now + d as i64);

        self.conn.execute(
            "INSERT INTO resource_leases
                (lease_id, resource_id, agent_id, capabilities_json, quota_json,
                 used_cost_usd, used_calls, started_at, expires_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7, 'active')",
            rusqlite::params![
                lease_id,
                resource_id,
                agent_id,
                capabilities_json,
                quota_json,
                now,
                expires_at
            ],
        )?;

        Ok(ResourceLease {
            lease_id,
            resource_id: resource_id.to_string(),
            agent_id: agent_id.to_string(),
            capabilities: capabilities.to_vec(),
            quota,
            used_cost_usd: 0.0,
            used_calls: 0,
            started_at: now,
            expires_at,
            status: LeaseStatus::Active,
        })
    }

    pub fn release_lease(&self, lease_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE resource_leases SET status = 'released' WHERE lease_id = ?1",
            rusqlite::params![lease_id],
        )?;
        Ok(())
    }

    pub fn revoke_lease(&self, lease_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE resource_leases SET status = 'revoked' WHERE lease_id = ?1",
            rusqlite::params![lease_id],
        )?;
        Ok(())
    }

    pub fn get_lease(&self, lease_id: &str) -> Result<Option<ResourceLease>> {
        let mut stmt = self.conn.prepare(
            "SELECT lease_id, resource_id, agent_id, capabilities_json, quota_json,
                    used_cost_usd, used_calls, started_at, expires_at, status
             FROM resource_leases WHERE lease_id = ?1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![lease_id], row_to_lease)?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_active_leases(&self) -> Result<Vec<ResourceLease>> {
        let mut stmt = self.conn.prepare(
            "SELECT lease_id, resource_id, agent_id, capabilities_json, quota_json,
                    used_cost_usd, used_calls, started_at, expires_at, status
             FROM resource_leases WHERE status = 'active'",
        )?;

        let rows = stmt.query_map([], row_to_lease)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn list_leases_for_agent(&self, agent_id: &str) -> Result<Vec<ResourceLease>> {
        let mut stmt = self.conn.prepare(
            "SELECT lease_id, resource_id, agent_id, capabilities_json, quota_json,
                    used_cost_usd, used_calls, started_at, expires_at, status
             FROM resource_leases WHERE agent_id = ?1 AND status = 'active'",
        )?;

        let rows = stmt.query_map(rusqlite::params![agent_id], row_to_lease)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Usage tracking
    // -----------------------------------------------------------------------

    pub fn record_usage(
        &self,
        lease_id: &str,
        action: &str,
        cost_usd: f64,
        detail: Option<&str>,
    ) -> Result<()> {
        // Get the lease to find resource_id and agent_id
        let lease = self
            .get_lease(lease_id)?
            .ok_or_else(|| NyayaError::Cache(format!("Lease not found: {}", lease_id)))?;

        let now = now_epoch();

        // Update the lease counters
        self.conn.execute(
            "UPDATE resource_leases
             SET used_calls = used_calls + 1, used_cost_usd = used_cost_usd + ?1
             WHERE lease_id = ?2",
            rusqlite::params![cost_usd, lease_id],
        )?;

        // Insert usage log entry
        self.conn.execute(
            "INSERT INTO resource_usage_log
                (lease_id, resource_id, agent_id, action, cost_usd, detail, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                lease_id,
                lease.resource_id,
                lease.agent_id,
                action,
                cost_usd,
                detail,
                now
            ],
        )?;

        Ok(())
    }

    /// Returns true if the lease is still within all quota limits.
    pub fn check_quota(&self, lease_id: &str) -> Result<bool> {
        let lease = self
            .get_lease(lease_id)?
            .ok_or_else(|| NyayaError::Cache(format!("Lease not found: {}", lease_id)))?;

        if let Some(max_calls) = lease.quota.max_calls {
            if lease.used_calls >= max_calls {
                return Ok(false);
            }
        }

        if let Some(max_cost) = lease.quota.max_cost_usd {
            if lease.used_cost_usd >= max_cost {
                return Ok(false);
            }
        }

        Ok(true)
    }

    // -----------------------------------------------------------------------
    // Maintenance
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Financial transactions
    // -----------------------------------------------------------------------

    pub fn log_transaction(&self, tx: &super::financial::FinancialTransaction) -> Result<()> {
        self.conn.execute(
            "INSERT INTO financial_transactions (tx_id, account_id, agent_id, lease_id, operation, amount, currency, status, approval_method, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                tx.tx_id, tx.account_id, tx.agent_id, tx.lease_id,
                serde_json::to_string(&tx.operation).unwrap_or_default(),
                tx.amount, tx.currency,
                serde_json::to_string(&tx.status).unwrap_or_default(),
                tx.approval_method, tx.timestamp
            ],
        )?;
        Ok(())
    }

    pub fn list_transactions(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<super::financial::FinancialTransaction>> {
        let mut stmt = self.conn.prepare(
            "SELECT tx_id, account_id, agent_id, lease_id, operation, amount, currency, status, approval_method, timestamp FROM financial_transactions WHERE account_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![account_id, limit as i64], |row| {
            Ok(super::financial::FinancialTransaction {
                tx_id: row.get(0)?,
                account_id: row.get(1)?,
                agent_id: row.get(2)?,
                lease_id: row.get(3)?,
                operation: serde_json::from_str(&row.get::<_, String>(4)?)
                    .unwrap_or(super::financial::FinancialOp::CheckBalance),
                amount: row.get(5)?,
                currency: row.get(6)?,
                status: serde_json::from_str(&row.get::<_, String>(7)?)
                    .unwrap_or(super::financial::TxStatus::Failed),
                approval_method: row.get(8)?,
                timestamp: row.get(9)?,
            })
        })?;
        let txns: std::result::Result<Vec<_>, _> = rows.collect();
        Ok(txns?)
    }

    // -----------------------------------------------------------------------
    // Maintenance
    // -----------------------------------------------------------------------

    /// Expire active leases whose `expires_at` is in the past. Returns count.
    pub fn expire_leases(&self) -> Result<usize> {
        let now = now_epoch();
        let count = self.conn.execute(
            "UPDATE resource_leases SET status = 'expired'
             WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < ?1",
            rusqlite::params![now],
        )?;
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Row mapper helpers
// ---------------------------------------------------------------------------

fn row_to_resource_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResourceRecord> {
    let resource_type_str: String = row.get(2)?;
    let status_json_str: String = row.get(3)?;
    let cost_model_str: Option<String> = row.get(4)?;
    let metadata_str: String = row.get(5)?;

    let resource_type: super::ResourceType =
        serde_json::from_str(&resource_type_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let status: super::ResourceStatus = serde_json::from_str(&status_json_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let cost_model: Option<super::CostModel> = match cost_model_str {
        Some(s) => Some(serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };

    let metadata: HashMap<String, String> = serde_json::from_str(&metadata_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(ResourceRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        resource_type,
        status,
        cost_model,
        metadata,
        config_json: row.get(6)?,
        registered_at: row.get(7)?,
        last_health_check: row.get(8)?,
    })
}

fn row_to_lease(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResourceLease> {
    let capabilities_json: String = row.get(3)?;
    let quota_json: String = row.get(4)?;
    let status_str: String = row.get(9)?;

    let capabilities: Vec<super::ResourceCapability> = serde_json::from_str(&capabilities_json)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let quota: LeaseQuota = serde_json::from_str(&quota_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let status: LeaseStatus =
        serde_json::from_str(&format!("\"{}\"", status_str)).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
        })?;

    Ok(ResourceLease {
        lease_id: row.get(0)?,
        resource_id: row.get(1)?,
        agent_id: row.get(2)?,
        capabilities,
        quota,
        used_cost_usd: row.get(5)?,
        used_calls: row.get(6)?,
        started_at: row.get(7)?,
        expires_at: row.get(8)?,
        status,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ResourceCapability, ResourceStatus, ResourceType};

    fn mem_registry() -> ResourceRegistry {
        ResourceRegistry::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_register_and_get_resource() {
        let reg = mem_registry();
        reg.register(
            "r1",
            "My Compute",
            &ResourceType::Compute,
            "{\"gpu\": true}",
        )
        .unwrap();

        let rec = reg.get_resource("r1").unwrap().expect("should exist");
        assert_eq!(rec.id, "r1");
        assert_eq!(rec.name, "My Compute");
        assert_eq!(rec.resource_type, ResourceType::Compute);
        assert_eq!(rec.status, ResourceStatus::Available);
        assert_eq!(rec.config_json, "{\"gpu\": true}");
    }

    #[test]
    fn test_list_resources() {
        let reg = mem_registry();
        reg.register("r1", "Compute A", &ResourceType::Compute, "{}")
            .unwrap();
        reg.register("r2", "API B", &ResourceType::ApiService, "{}")
            .unwrap();

        let list = reg.list_resources().unwrap();
        assert_eq!(list.len(), 2);
        let ids: Vec<&str> = list.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"r1"));
        assert!(ids.contains(&"r2"));
    }

    #[test]
    fn test_unregister_resource() {
        let reg = mem_registry();
        reg.register("r1", "Temp", &ResourceType::Device, "{}")
            .unwrap();
        assert!(reg.get_resource("r1").unwrap().is_some());

        reg.unregister("r1").unwrap();
        assert!(reg.get_resource("r1").unwrap().is_none());
    }

    #[test]
    fn test_acquire_and_get_lease() {
        let reg = mem_registry();
        reg.register("r1", "Compute", &ResourceType::Compute, "{}")
            .unwrap();

        let lease = reg
            .acquire_lease(
                "agent-1",
                "r1",
                &[ResourceCapability::ReadData, ResourceCapability::Execute],
                LeaseQuota {
                    max_cost_usd: Some(10.0),
                    max_calls: Some(100),
                    max_duration_secs: None,
                },
            )
            .unwrap();

        assert_eq!(lease.agent_id, "agent-1");
        assert_eq!(lease.resource_id, "r1");
        assert_eq!(lease.status, LeaseStatus::Active);
        assert_eq!(lease.used_calls, 0);

        let fetched = reg
            .get_lease(&lease.lease_id)
            .unwrap()
            .expect("should exist");
        assert_eq!(fetched.lease_id, lease.lease_id);
        assert_eq!(fetched.capabilities.len(), 2);
    }

    #[test]
    fn test_release_lease() {
        let reg = mem_registry();
        reg.register("r1", "X", &ResourceType::Compute, "{}")
            .unwrap();

        let lease = reg
            .acquire_lease(
                "agent-1",
                "r1",
                &[ResourceCapability::ReadData],
                LeaseQuota {
                    max_cost_usd: None,
                    max_calls: None,
                    max_duration_secs: None,
                },
            )
            .unwrap();

        reg.release_lease(&lease.lease_id).unwrap();

        let fetched = reg
            .get_lease(&lease.lease_id)
            .unwrap()
            .expect("should exist");
        assert_eq!(fetched.status, LeaseStatus::Released);
    }

    #[test]
    fn test_record_usage_and_check_quota() {
        let reg = mem_registry();
        reg.register("r1", "X", &ResourceType::Compute, "{}")
            .unwrap();

        let lease = reg
            .acquire_lease(
                "agent-1",
                "r1",
                &[ResourceCapability::Execute],
                LeaseQuota {
                    max_cost_usd: None,
                    max_calls: Some(3),
                    max_duration_secs: None,
                },
            )
            .unwrap();

        // Record 2 usages — still within quota
        reg.record_usage(&lease.lease_id, "call", 0.01, None)
            .unwrap();
        reg.record_usage(&lease.lease_id, "call", 0.01, Some("second call"))
            .unwrap();
        assert!(reg.check_quota(&lease.lease_id).unwrap());

        // Record 1 more — now at limit (3 >= 3)
        reg.record_usage(&lease.lease_id, "call", 0.01, None)
            .unwrap();
        assert!(!reg.check_quota(&lease.lease_id).unwrap());
    }

    #[test]
    fn test_expire_leases() {
        let reg = mem_registry();
        reg.register("r1", "X", &ResourceType::Compute, "{}")
            .unwrap();

        // Acquire a lease that expires in the past by using max_duration_secs = 0
        // then manually set expires_at to the past
        let lease = reg
            .acquire_lease(
                "agent-1",
                "r1",
                &[ResourceCapability::ReadData],
                LeaseQuota {
                    max_cost_usd: None,
                    max_calls: None,
                    max_duration_secs: Some(0),
                },
            )
            .unwrap();

        // Manually set expires_at to a past timestamp to guarantee expiry
        reg.conn
            .execute(
                "UPDATE resource_leases SET expires_at = ?1 WHERE lease_id = ?2",
                rusqlite::params![now_epoch() - 10, lease.lease_id],
            )
            .unwrap();

        let count = reg.expire_leases().unwrap();
        assert_eq!(count, 1);

        let fetched = reg
            .get_lease(&lease.lease_id)
            .unwrap()
            .expect("should exist");
        assert_eq!(fetched.status, LeaseStatus::Expired);
    }

    #[test]
    fn test_list_leases_for_agent() {
        let reg = mem_registry();
        reg.register("r1", "X", &ResourceType::Compute, "{}")
            .unwrap();
        reg.register("r2", "Y", &ResourceType::ApiService, "{}")
            .unwrap();
        reg.register("r3", "Z", &ResourceType::Device, "{}")
            .unwrap();

        let quota = LeaseQuota {
            max_cost_usd: None,
            max_calls: None,
            max_duration_secs: None,
        };

        reg.acquire_lease(
            "agent-A",
            "r1",
            &[ResourceCapability::ReadData],
            quota.clone(),
        )
        .unwrap();
        reg.acquire_lease(
            "agent-A",
            "r2",
            &[ResourceCapability::WriteData],
            quota.clone(),
        )
        .unwrap();
        reg.acquire_lease("agent-B", "r3", &[ResourceCapability::Execute], quota)
            .unwrap();

        let a_leases = reg.list_leases_for_agent("agent-A").unwrap();
        assert_eq!(a_leases.len(), 2);
        assert!(a_leases.iter().all(|l| l.agent_id == "agent-A"));

        let b_leases = reg.list_leases_for_agent("agent-B").unwrap();
        assert_eq!(b_leases.len(), 1);
    }

    #[test]
    fn test_resource_record_display() {
        let rec = ResourceRecord {
            id: "gpu-a100".to_string(),
            name: "NVIDIA A100".to_string(),
            resource_type: ResourceType::Compute,
            status: ResourceStatus::Available,
            cost_model: None,
            metadata: HashMap::new(),
            config_json: "{}".to_string(),
            registered_at: 1000,
            last_health_check: None,
        };
        assert_eq!(rec.resource_type_display(), "compute");
        assert_eq!(rec.status_display(), "available");

        let rec2 = ResourceRecord {
            id: "stripe-acct".to_string(),
            name: "Stripe".to_string(),
            resource_type: ResourceType::Financial,
            status: ResourceStatus::InUse {
                agent_id: "agent-7".to_string(),
            },
            cost_model: None,
            metadata: HashMap::new(),
            config_json: "{}".to_string(),
            registered_at: 2000,
            last_health_check: None,
        };
        assert_eq!(rec2.resource_type_display(), "financial");
        assert_eq!(rec2.status_display(), "in_use:agent-7");

        let rec3 = ResourceRecord {
            id: "cam-01".to_string(),
            name: "Front Camera".to_string(),
            resource_type: ResourceType::Device,
            status: ResourceStatus::Offline,
            cost_model: None,
            metadata: HashMap::new(),
            config_json: "{}".to_string(),
            registered_at: 3000,
            last_health_check: None,
        };
        assert_eq!(rec3.resource_type_display(), "device");
        assert_eq!(rec3.status_display(), "offline");

        let rec4 = ResourceRecord {
            id: "openai-api".to_string(),
            name: "OpenAI".to_string(),
            resource_type: ResourceType::ApiService,
            status: ResourceStatus::Degraded,
            cost_model: None,
            metadata: HashMap::new(),
            config_json: "{}".to_string(),
            registered_at: 4000,
            last_health_check: None,
        };
        assert_eq!(rec4.resource_type_display(), "api_service");
        assert_eq!(rec4.status_display(), "degraded");
    }
}
