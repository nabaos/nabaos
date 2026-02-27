//! Agent catalog — browse and search pre-built agents.

use crate::core::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Catalog entry — metadata about an available agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub author: String,
    pub permissions: Vec<String>,
    pub path: PathBuf,
}

/// Browse the agent catalog.
pub struct AgentCatalog {
    catalog_dir: PathBuf,
}

impl AgentCatalog {
    pub fn new(catalog_dir: &Path) -> Self {
        Self {
            catalog_dir: catalog_dir.to_path_buf(),
        }
    }

    /// List all agents in the catalog.
    pub fn list(&self) -> Result<Vec<CatalogEntry>> {
        let mut entries = Vec::new();
        if !self.catalog_dir.exists() {
            return Ok(entries);
        }
        for entry in std::fs::read_dir(&self.catalog_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let manifest_path = entry.path().join("manifest.yaml");
                if manifest_path.exists() {
                    if let Ok(e) = self.parse_entry(&entry.path()) {
                        entries.push(e);
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
        Ok(entries)
    }

    /// Search catalog by keyword (matches name, description, category).
    pub fn search(&self, query: &str) -> Result<Vec<CatalogEntry>> {
        let q = query.to_lowercase();
        let all = self.list()?;
        Ok(all
            .into_iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.category.to_lowercase().contains(&q)
            })
            .collect())
    }

    /// Get info about a specific agent.
    pub fn get(&self, name: &str) -> Result<Option<CatalogEntry>> {
        let path = self.catalog_dir.join(name);
        if path.exists() && path.join("manifest.yaml").exists() {
            Ok(Some(self.parse_entry(&path)?))
        } else {
            Ok(None)
        }
    }

    /// Get the path to an agent's directory in the catalog.
    pub fn agent_path(&self, name: &str) -> PathBuf {
        self.catalog_dir.join(name)
    }

    fn parse_entry(&self, dir: &Path) -> Result<CatalogEntry> {
        let manifest_path = dir.join("manifest.yaml");
        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: serde_yaml::Value = serde_yaml::from_str(&content)?;

        Ok(CatalogEntry {
            name: manifest["name"].as_str().unwrap_or("unknown").to_string(),
            version: manifest["version"].as_str().unwrap_or("0.0.0").to_string(),
            description: manifest["description"].as_str().unwrap_or("").to_string(),
            category: manifest["category"]
                .as_str()
                .unwrap_or("uncategorized")
                .to_string(),
            author: manifest["author"]
                .as_str()
                .unwrap_or("community")
                .to_string(),
            permissions: manifest["permissions"]
                .as_sequence()
                .map(|s| {
                    s.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            path: dir.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = AgentCatalog::new(dir.path());
        assert_eq!(catalog.list().unwrap().len(), 0);
    }

    #[test]
    fn test_list_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("email-assistant");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("manifest.yaml"),
            r#"
name: email-assistant
version: "1.0.0"
description: "Smart email triage"
category: email
author: nyaya-official
permissions: [gmail.read, llm.query]
"#,
        )
        .unwrap();

        let catalog = AgentCatalog::new(dir.path());
        let list = catalog.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "email-assistant");

        let results = catalog.search("email").unwrap();
        assert_eq!(results.len(), 1);

        let results = catalog.search("nonexistent").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_get_specific_agent() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("test-bot");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("manifest.yaml"), "name: test-bot\nversion: \"1.0.0\"\ndescription: test\ncategory: testing\nauthor: test\npermissions: []\n").unwrap();

        let catalog = AgentCatalog::new(dir.path());
        assert!(catalog.get("test-bot").unwrap().is_some());
        assert!(catalog.get("nonexistent").unwrap().is_none());
    }
}
