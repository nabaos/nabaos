use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::error::Result;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinancialAccountType {
    Payment,
    BankAccount,
    CryptoWallet,
    Budget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinancialOp {
    CheckBalance,
    Send,
    Receive,
    QueryHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TxStatus {
    Pending,
    Approved,
    Executed,
    Failed,
    Rejected,
}

// ---------------------------------------------------------------------------
// Config + Account
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialAccountConfig {
    pub account_type: FinancialAccountType,
    pub provider: String,
    pub currency: String,
    pub api_endpoint: Option<String>,
    pub credential_id: Option<String>,
    pub daily_limit: Option<f64>,
    pub threshold_2fa: Option<f64>,
    #[serde(default)]
    pub default_quota: Option<super::registry::LeaseQuota>,
}

#[derive(Debug, Clone)]
pub struct FinancialAccount {
    pub id: String,
    pub name: String,
    pub config: FinancialAccountConfig,
    pub status: super::ResourceStatus,
    pub balance: Option<f64>,
}

// ---------------------------------------------------------------------------
// Transaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialTransaction {
    pub tx_id: String,
    pub account_id: String,
    pub agent_id: String,
    pub lease_id: String,
    pub operation: FinancialOp,
    pub amount: Option<f64>,
    pub currency: String,
    pub status: TxStatus,
    pub approval_method: Option<String>,
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// Resource trait implementation
// ---------------------------------------------------------------------------

impl super::Resource for FinancialAccount {
    fn id(&self) -> &str {
        &self.id
    }

    fn resource_type(&self) -> super::ResourceType {
        super::ResourceType::Financial
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> &super::ResourceStatus {
        &self.status
    }

    fn capabilities(&self) -> Vec<super::ResourceCapability> {
        vec![
            super::ResourceCapability::ReadData,
            super::ResourceCapability::WriteData,
        ]
    }

    fn cost_model(&self) -> Option<super::CostModel> {
        match self.config.account_type {
            FinancialAccountType::Budget => Some(super::CostModel::PerCall(0.0)),
            _ => None,
        }
    }

    fn security_tier(&self, capability: &super::ResourceCapability) -> super::SecurityTier {
        match capability {
            super::ResourceCapability::WriteData => super::SecurityTier::Critical,
            super::ResourceCapability::ReadData => super::SecurityTier::ExternalRead,
            _ => super::default_security_tier(&super::ResourceType::Financial, capability),
        }
    }

    fn health_check(&self) -> Result<super::HealthReport> {
        Ok(super::HealthReport {
            healthy: true,
            message: format!("Financial account {} is reachable", self.id),
            checked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        })
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("provider".to_string(), self.config.provider.clone());
        map.insert("currency".to_string(), self.config.currency.clone());
        map.insert(
            "account_type".to_string(),
            serde_json::to_string(&self.config.account_type).unwrap_or_default(),
        );
        map
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

pub fn requires_2fa(config: &FinancialAccountConfig, amount: Option<f64>) -> bool {
    if let (Some(threshold), Some(amt)) = (config.threshold_2fa, amount) {
        amt >= threshold
    } else {
        // If no threshold configured, require 2FA for all financial writes
        true
    }
}

pub fn requires_password_escalation(config: &FinancialAccountConfig, amount: Option<f64>) -> bool {
    if let (Some(threshold), Some(amt)) = (config.threshold_2fa, amount) {
        amt >= threshold * 2.0
    } else {
        false
    }
}

pub fn financial_account_from_config(
    id: &str,
    name: &str,
    config: FinancialAccountConfig,
) -> FinancialAccount {
    FinancialAccount {
        id: id.to_string(),
        name: name.to_string(),
        config,
        status: super::ResourceStatus::Available,
        balance: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{Resource, ResourceCapability, ResourceType, SecurityTier};

    fn test_config(threshold: Option<f64>) -> FinancialAccountConfig {
        FinancialAccountConfig {
            account_type: FinancialAccountType::BankAccount,
            provider: "test-bank".to_string(),
            currency: "USD".to_string(),
            api_endpoint: None,
            credential_id: None,
            daily_limit: None,
            threshold_2fa: threshold,
            default_quota: None,
        }
    }

    #[test]
    fn test_financial_account_resource_trait() {
        let account = financial_account_from_config("fin-1", "My Bank", test_config(Some(100.0)));

        assert_eq!(account.resource_type(), ResourceType::Financial);
        let caps = account.capabilities();
        assert!(caps.contains(&ResourceCapability::ReadData));
        assert!(caps.contains(&ResourceCapability::WriteData));
        assert_eq!(caps.len(), 2);
        assert_eq!(account.id(), "fin-1");
        assert_eq!(account.name(), "My Bank");
    }

    #[test]
    fn test_financial_security_tiers() {
        let account =
            financial_account_from_config("fin-2", "Secure Account", test_config(Some(50.0)));

        assert_eq!(
            account.security_tier(&ResourceCapability::WriteData),
            SecurityTier::Critical
        );
        assert_eq!(
            account.security_tier(&ResourceCapability::ReadData),
            SecurityTier::ExternalRead
        );
    }

    #[test]
    fn test_requires_2fa() {
        let config_with_threshold = test_config(Some(100.0));

        // Below threshold — false
        assert!(!requires_2fa(&config_with_threshold, Some(50.0)));
        // At threshold — true
        assert!(requires_2fa(&config_with_threshold, Some(100.0)));
        // Above threshold — true
        assert!(requires_2fa(&config_with_threshold, Some(200.0)));

        // No threshold configured — defaults true
        let config_no_threshold = test_config(None);
        assert!(requires_2fa(&config_no_threshold, Some(10.0)));
        assert!(requires_2fa(&config_no_threshold, None));
    }

    #[test]
    fn test_requires_password_escalation() {
        let config = test_config(Some(100.0));

        // Below double threshold (200) — false
        assert!(!requires_password_escalation(&config, Some(150.0)));
        // At double threshold — true
        assert!(requires_password_escalation(&config, Some(200.0)));
        // Above double threshold — true
        assert!(requires_password_escalation(&config, Some(300.0)));

        // No threshold — false
        let config_no = test_config(None);
        assert!(!requires_password_escalation(&config_no, Some(1000.0)));
    }

    #[test]
    fn test_financial_transaction_serde() {
        let tx = FinancialTransaction {
            tx_id: "tx-001".to_string(),
            account_id: "fin-1".to_string(),
            agent_id: "agent-1".to_string(),
            lease_id: "lease-1".to_string(),
            operation: FinancialOp::Send,
            amount: Some(42.50),
            currency: "USD".to_string(),
            status: TxStatus::Approved,
            approval_method: Some("2fa_totp".to_string()),
            timestamp: 1700000000,
        };

        let json = serde_json::to_string(&tx).unwrap();
        let back: FinancialTransaction = serde_json::from_str(&json).unwrap();

        assert_eq!(back.tx_id, "tx-001");
        assert_eq!(back.operation, FinancialOp::Send);
        assert_eq!(back.status, TxStatus::Approved);
        assert_eq!(back.amount, Some(42.50));
        assert_eq!(back.approval_method, Some("2fa_totp".to_string()));
    }
}
