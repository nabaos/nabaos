//! Cost tracker — tracks LLM spending per chain, per day, cumulative.
//!
//! Records every LLM call with token counts and estimated cost.
//! Calculates savings from cache hits (avoided LLM calls).

use crate::core::error::{NyayaError, Result};

/// Cost per 1M tokens (USD) for supported providers.
/// These are approximate and should be updated as pricing changes.
pub struct PricingTable {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

impl PricingTable {
    pub fn for_provider(provider: &str, model: &str) -> Self {
        match (provider, model) {
            ("anthropic", m) if m.contains("haiku") => PricingTable {
                input_per_million: 0.80,
                output_per_million: 4.00,
            },
            ("anthropic", m) if m.contains("sonnet") => PricingTable {
                input_per_million: 3.00,
                output_per_million: 15.00,
            },
            ("anthropic", m) if m.contains("opus") => PricingTable {
                input_per_million: 15.00,
                output_per_million: 75.00,
            },
            ("openai", m) if m.contains("gpt-4o-mini") => PricingTable {
                input_per_million: 0.15,
                output_per_million: 0.60,
            },
            ("openai", m) if m.contains("gpt-4o") => PricingTable {
                input_per_million: 2.50,
                output_per_million: 10.00,
            },
            ("deepseek", _) => PricingTable {
                input_per_million: 0.14,
                output_per_million: 0.28,
            },
            _ => PricingTable {
                input_per_million: 1.00,
                output_per_million: 5.00,
            },
        }
    }

    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_per_million
            + (output_tokens as f64 / 1_000_000.0) * self.output_per_million
    }
}

/// SQLite-backed cost tracker.
pub struct CostTracker {
    conn: rusqlite::Connection,
}

impl CostTracker {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Cost DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cost_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cost_usd REAL NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cache_savings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id TEXT,
                saved_provider TEXT NOT NULL,
                saved_model TEXT NOT NULL,
                estimated_input_tokens INTEGER NOT NULL,
                estimated_output_tokens INTEGER NOT NULL,
                estimated_cost_usd REAL NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cost_ts ON cost_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_savings_ts ON cache_savings(timestamp);",
        )
        .map_err(|e| NyayaError::Cache(format!("Cost table creation failed: {}", e)))?;

        Ok(Self { conn })
    }

    /// Record an LLM call.
    pub fn record_call(
        &self,
        chain_id: Option<&str>,
        provider: &str,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
    ) -> Result<f64> {
        let pricing = PricingTable::for_provider(provider, model);
        let cost = pricing.estimate_cost(input_tokens, output_tokens);
        let now = now_millis();

        self.conn
            .execute(
                "INSERT INTO cost_log (chain_id, provider, model, input_tokens, output_tokens, cost_usd, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![chain_id, provider, model, input_tokens, output_tokens, cost, now],
            )
            .map_err(|e| NyayaError::Cache(format!("Cost record failed: {}", e)))?;

        Ok(cost)
    }

    /// Record a cache hit (saved LLM call).
    pub fn record_cache_saving(
        &self,
        chain_id: Option<&str>,
        provider: &str,
        model: &str,
        estimated_input_tokens: u32,
        estimated_output_tokens: u32,
    ) -> Result<f64> {
        let pricing = PricingTable::for_provider(provider, model);
        let saved = pricing.estimate_cost(estimated_input_tokens, estimated_output_tokens);
        let now = now_millis();

        self.conn
            .execute(
                "INSERT INTO cache_savings (chain_id, saved_provider, saved_model,
                 estimated_input_tokens, estimated_output_tokens, estimated_cost_usd, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    chain_id,
                    provider,
                    model,
                    estimated_input_tokens,
                    estimated_output_tokens,
                    saved,
                    now
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("Savings record failed: {}", e)))?;

        Ok(saved)
    }

    /// Record a semantic cache hit — logs the estimated savings from avoiding an LLM call.
    pub fn record_cache_hit(&self) -> Result<()> {
        // Estimate: a cache hit saves ~500 input + 200 output tokens on Haiku
        let _ = self.record_cache_saving(None, "anthropic", "claude-haiku-4-5", 500, 200)?;
        Ok(())
    }

    /// Get cost summary for a time period.
    pub fn summary(&self, since_ms: Option<i64>) -> Result<CostSummary> {
        let since = since_ms.unwrap_or(0);

        let total_spent: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(cost_usd), 0.0) FROM cost_log WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let total_calls: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM cost_log WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let total_saved: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(estimated_cost_usd), 0.0) FROM cache_savings WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let cache_hits: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM cache_savings WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let total_tokens_in: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(input_tokens), 0) FROM cost_log WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let total_tokens_out: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(output_tokens), 0) FROM cost_log WHERE timestamp >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Summary query failed: {}", e)))?;

        let savings_pct = if total_spent + total_saved > 0.0 {
            total_saved / (total_spent + total_saved) * 100.0
        } else {
            0.0
        };

        Ok(CostSummary {
            total_spent_usd: total_spent,
            total_saved_usd: total_saved,
            savings_percent: savings_pct,
            total_llm_calls: total_calls as u64,
            total_cache_hits: cache_hits as u64,
            total_input_tokens: total_tokens_in as u64,
            total_output_tokens: total_tokens_out as u64,
        })
    }

    /// Check if spending is within the daily budget limit.
    /// Returns Ok(true) if under budget, Ok(false) if over.
    pub fn check_budget(&self, daily_limit: f64) -> Result<bool> {
        // Calculate start of today in milliseconds
        let now = now_millis();
        let day_ms: i64 = 24 * 60 * 60 * 1000;
        let today_start = (now / day_ms) * day_ms;

        let today_spent: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(cost_usd), 0.0) FROM cost_log WHERE timestamp >= ?1",
                rusqlite::params![today_start],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Budget check failed: {}", e)))?;

        Ok(today_spent < daily_limit)
    }

    /// Check if an estimated cost fits within a per-task budget.
    pub fn estimate_ok(&self, estimated_cost: f64, per_task_limit: f64) -> bool {
        estimated_cost <= per_task_limit
    }

    /// Return a dashboard view with daily, weekly, and monthly cost breakdowns.
    pub fn dashboard(&self) -> Result<CostDashboard> {
        let now = now_millis();
        let day_ms: i64 = 24 * 60 * 60 * 1000;

        let daily = self.summary(Some(now - day_ms))?;
        let weekly = self.summary(Some(now - 7 * day_ms))?;
        let monthly = self.summary(Some(now - 30 * day_ms))?;

        let daily_cache_hit_rate = if daily.total_llm_calls + daily.total_cache_hits > 0 {
            daily.total_cache_hits as f64 / (daily.total_llm_calls + daily.total_cache_hits) as f64
                * 100.0
        } else {
            0.0
        };

        Ok(CostDashboard {
            daily,
            weekly,
            monthly,
            daily_cache_hit_rate,
        })
    }
}

/// Dashboard view combining daily/weekly/monthly cost summaries.
#[derive(Debug)]
pub struct CostDashboard {
    pub daily: CostSummary,
    pub weekly: CostSummary,
    pub monthly: CostSummary,
    /// Cache hit rate for the daily period (0-100).
    pub daily_cache_hit_rate: f64,
}

/// Cost summary statistics.
#[derive(Debug)]
pub struct CostSummary {
    pub total_spent_usd: f64,
    pub total_saved_usd: f64,
    pub savings_percent: f64,
    pub total_llm_calls: u64,
    pub total_cache_hits: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

impl std::fmt::Display for CostSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "LLM calls:     {}", self.total_llm_calls)?;
        writeln!(f, "Cache hits:    {}", self.total_cache_hits)?;
        writeln!(f, "Total spent:   ${:.4}", self.total_spent_usd)?;
        writeln!(f, "Total saved:   ${:.4}", self.total_saved_usd)?;
        writeln!(f, "Savings:       {:.1}%", self.savings_percent)?;
        writeln!(
            f,
            "Tokens:        {} in / {} out",
            self.total_input_tokens, self.total_output_tokens
        )
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_table() {
        let haiku = PricingTable::for_provider("anthropic", "claude-haiku-4-5");
        assert_eq!(haiku.input_per_million, 0.80);

        // 1000 input + 500 output tokens
        let cost = haiku.estimate_cost(1000, 500);
        assert!((cost - 0.0028).abs() < 0.0001);
    }

    #[test]
    fn test_record_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = CostTracker::open(&dir.path().join("cost.db")).unwrap();

        // Record some LLM calls
        let c1 = tracker
            .record_call(Some("weather"), "anthropic", "claude-haiku-4-5", 500, 200)
            .unwrap();
        let c2 = tracker
            .record_call(None, "anthropic", "claude-haiku-4-5", 800, 300)
            .unwrap();

        assert!(c1 > 0.0);
        assert!(c2 > 0.0);

        let summary = tracker.summary(None).unwrap();
        assert_eq!(summary.total_llm_calls, 2);
        assert!((summary.total_spent_usd - (c1 + c2)).abs() < 0.0001);
        assert_eq!(summary.total_input_tokens, 1300);
        assert_eq!(summary.total_output_tokens, 500);
    }

    #[test]
    fn test_cache_savings() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = CostTracker::open(&dir.path().join("cost.db")).unwrap();

        // One real call
        tracker
            .record_call(None, "anthropic", "claude-haiku-4-5", 500, 200)
            .unwrap();

        // Three cache hits (saved calls)
        for _ in 0..3 {
            tracker
                .record_cache_saving(Some("weather"), "anthropic", "claude-haiku-4-5", 500, 200)
                .unwrap();
        }

        let summary = tracker.summary(None).unwrap();
        assert_eq!(summary.total_llm_calls, 1);
        assert_eq!(summary.total_cache_hits, 3);
        assert!(summary.savings_percent > 70.0); // 3 out of 4 were cached
    }

    #[test]
    fn test_deepseek_pricing() {
        let ds = PricingTable::for_provider("deepseek", "deepseek-v3");
        // DeepSeek is cheapest
        let cost = ds.estimate_cost(10000, 5000);
        let haiku = PricingTable::for_provider("anthropic", "claude-haiku-4-5");
        let haiku_cost = haiku.estimate_cost(10000, 5000);
        assert!(cost < haiku_cost);
    }
}
