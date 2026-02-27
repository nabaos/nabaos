use std::fs;
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use super::types::{IntentFilter, ResourceLimits};
use crate::core::error::{NyayaError, Result};

/// Metadata for a .nap (Nyaya Agent Package) archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub intent_filters: Vec<IntentFilter>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub subscriptions: Vec<String>,
    #[serde(default)]
    pub data_namespace: Option<String>,
    #[serde(default)]
    pub triggers: super::types::AgentTriggers,
}

impl PackageMetadata {
    /// Validate the metadata fields.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(NyayaError::Config("Package name cannot be empty".into()));
        }
        if self.version.is_empty() {
            return Err(NyayaError::Config("Package version cannot be empty".into()));
        }
        // Name must be alphanumeric, hyphens, or underscores only
        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(NyayaError::Config(
                "Package name must contain only alphanumeric characters, hyphens, or underscores"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Returns the data namespace, falling back to the package name.
    pub fn namespace(&self) -> &str {
        self.data_namespace.as_deref().unwrap_or(&self.name)
    }
}

/// Extract a .nap package (tar.gz) into `target_dir`, read manifest.yaml, and return metadata.
pub fn extract_package(nap_path: &Path, target_dir: &Path) -> Result<PackageMetadata> {
    let file = fs::File::open(nap_path)
        .map_err(|e| NyayaError::Config(format!("Failed to open package: {}", e)))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    fs::create_dir_all(target_dir)?;
    archive
        .unpack(target_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to extract package: {}", e)))?;

    // Read manifest.yaml from the extracted directory
    let manifest_path = target_dir.join("manifest.yaml");
    if !manifest_path.exists() {
        return Err(NyayaError::Config(
            "Package does not contain manifest.yaml".into(),
        ));
    }

    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| NyayaError::Config(format!("Failed to read manifest.yaml: {}", e)))?;
    let metadata: PackageMetadata = serde_yaml::from_str(&manifest_content)?;
    metadata.validate()?;

    Ok(metadata)
}

/// Create a .nap package (tar.gz) from a source directory containing manifest.yaml.
pub fn create_package(source_dir: &Path, output_path: &Path) -> Result<()> {
    let manifest_path = source_dir.join("manifest.yaml");
    if !manifest_path.exists() {
        return Err(NyayaError::Config(
            "Source directory must contain manifest.yaml".into(),
        ));
    }

    // Validate the manifest before packaging
    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| NyayaError::Config(format!("Failed to read manifest.yaml: {}", e)))?;
    let metadata: PackageMetadata = serde_yaml::from_str(&manifest_content)?;
    metadata.validate()?;

    let file = fs::File::create(output_path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);

    builder
        .append_dir_all(".", source_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to create package: {}", e)))?;

    let encoder = builder
        .into_inner()
        .map_err(|e| NyayaError::Config(format!("Failed to finalize tar: {}", e)))?;
    encoder
        .finish()
        .map_err(|e| NyayaError::Config(format!("Failed to finalize gzip: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_metadata() -> PackageMetadata {
        PackageMetadata {
            name: "test-agent".to_string(),
            version: "1.0.0".to_string(),
            description: "A test agent".to_string(),
            author: "tester".to_string(),
            signature: String::new(),
            intent_filters: vec![],
            permissions: vec!["kv.read".to_string()],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            triggers: Default::default(),
        }
    }

    #[test]
    fn test_metadata_validation_ok() {
        let m = sample_metadata();
        assert!(m.validate().is_ok());
    }

    #[test]
    fn test_metadata_empty_name_rejected() {
        let mut m = sample_metadata();
        m.name = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_metadata_invalid_name_rejected() {
        let mut m = sample_metadata();
        m.name = "bad name!".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_metadata_empty_version_rejected() {
        let mut m = sample_metadata();
        m.version = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_namespace_falls_back_to_name() {
        let m = sample_metadata();
        assert_eq!(m.namespace(), "test-agent");
    }

    #[test]
    fn test_namespace_uses_data_namespace() {
        let mut m = sample_metadata();
        m.data_namespace = Some("custom-ns".to_string());
        assert_eq!(m.namespace(), "custom-ns");
    }

    #[test]
    fn test_create_and_extract_round_trip() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        // Write manifest.yaml
        let meta = sample_metadata();
        let yaml = serde_yaml::to_string(&meta).unwrap();
        fs::write(source_dir.join("manifest.yaml"), &yaml).unwrap();

        // Write an extra file
        fs::write(source_dir.join("README.txt"), "hello").unwrap();

        // Create package
        let nap_path = tmp.path().join("test-agent.nap");
        create_package(&source_dir, &nap_path).unwrap();
        assert!(nap_path.exists());

        // Extract package
        let extract_dir = tmp.path().join("extracted");
        let extracted_meta = extract_package(&nap_path, &extract_dir).unwrap();

        assert_eq!(extracted_meta.name, "test-agent");
        assert_eq!(extracted_meta.version, "1.0.0");
        assert!(extract_dir.join("manifest.yaml").exists());
        assert!(extract_dir.join("README.txt").exists());
    }

    #[test]
    fn test_extract_missing_manifest_rejected() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("empty_src");
        fs::create_dir_all(&source_dir).unwrap();

        // Create a tar.gz without manifest.yaml
        let nap_path = tmp.path().join("bad.nap");
        {
            let file = fs::File::create(&nap_path).unwrap();
            let encoder = GzEncoder::new(file, Compression::default());
            let mut builder = tar::Builder::new(encoder);
            // Add a dummy file
            let data = b"hello";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "dummy.txt", &data[..])
                .unwrap();
            let enc = builder.into_inner().unwrap();
            enc.finish().unwrap();
        }

        let extract_dir = tmp.path().join("extract_bad");
        let result = extract_package(&nap_path, &extract_dir);
        assert!(result.is_err());
    }
}
