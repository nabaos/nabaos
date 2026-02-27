//! Sandboxed filesystem operations — constitution-gated read/write.

use crate::core::error::{NyayaError, Result};
use std::path::{Path, PathBuf};

/// Sandboxed filesystem with path allowlisting.
pub struct SandboxedFs {
    allowed_paths: Vec<PathBuf>,
    write_enabled: bool,
}

impl SandboxedFs {
    /// Create a new sandboxed filesystem with allowed paths.
    pub fn new(allowed_paths: Vec<PathBuf>, write_enabled: bool) -> Self {
        Self {
            allowed_paths,
            write_enabled,
        }
    }

    /// Validate that a path is within allowed directories and safe from traversal.
    /// If `must_exist` is false, the parent directory is canonicalized instead
    /// (used for write targets that don't exist yet).
    fn validate_path_inner(&self, path: &str, must_exist: bool) -> Result<PathBuf> {
        let requested = Path::new(path);

        // Reject obvious traversal patterns before canonicalization
        if path.contains("..") {
            return Err(NyayaError::Config(format!(
                "Path traversal rejected: {}",
                path
            )));
        }

        // Canonicalize to resolve symlinks. For non-existent files,
        // canonicalize the parent and append the file name.
        let canonical = if must_exist || requested.exists() {
            requested
                .canonicalize()
                .map_err(|e| NyayaError::Config(format!("Invalid path '{}': {}", path, e)))?
        } else {
            // File doesn't exist yet — canonicalize parent
            let parent = requested
                .parent()
                .ok_or_else(|| NyayaError::Config(format!("No parent directory for '{}'", path)))?;
            let file_name = requested
                .file_name()
                .ok_or_else(|| NyayaError::Config(format!("No file name in '{}'", path)))?;
            let canon_parent = parent
                .canonicalize()
                .map_err(|e| NyayaError::Config(format!("Invalid path '{}': {}", path, e)))?;
            canon_parent.join(file_name)
        };

        // Check against allowed paths
        for allowed in &self.allowed_paths {
            // Canonicalize allowed path too (if it exists)
            let allowed_canonical = allowed.canonicalize().unwrap_or_else(|_| allowed.clone());
            if canonical.starts_with(&allowed_canonical) {
                return Ok(canonical);
            }
        }

        Err(NyayaError::Config(format!(
            "Path '{}' not in allowed directories: {:?}",
            path, self.allowed_paths
        )))
    }

    /// Validate a path that must already exist (reads, list_dir).
    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        self.validate_path_inner(path, true)
    }

    /// Validate a path for writing (file may not exist yet).
    fn validate_write_path(&self, path: &str) -> Result<PathBuf> {
        self.validate_path_inner(path, false)
    }

    /// Read a file's contents as a string.
    pub fn read_file(&self, path: &str) -> Result<String> {
        let validated = self.validate_path(path)?;
        std::fs::read_to_string(&validated)
            .map_err(|e| NyayaError::Config(format!("File read error: {}", e)))
    }

    /// Write content to a file.
    pub fn write_file(&self, path: &str, content: &str) -> Result<()> {
        if !self.write_enabled {
            return Err(NyayaError::Config(
                "File write disabled by constitution".into(),
            ));
        }
        let validated = self.validate_write_path(path)?;

        // Create parent directories if needed
        if let Some(parent) = validated.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| NyayaError::Config(format!("Failed to create directory: {}", e)))?;
        }

        std::fs::write(&validated, content)
            .map_err(|e| NyayaError::Config(format!("File write error: {}", e)))
    }

    /// List files in a directory (non-recursive).
    pub fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let validated = self.validate_path(path)?;
        let entries = std::fs::read_dir(&validated)
            .map_err(|e| NyayaError::Config(format!("Directory read error: {}", e)))?;

        let mut files = Vec::new();
        for entry in entries.flatten() {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
        files.sort();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nyaya_fs_test_{}", suffix));
        let _ = fs::remove_dir_all(&dir); // clean slate
        fs::create_dir_all(&dir).unwrap();
        // Create a test file
        fs::write(dir.join("test.txt"), "hello world").unwrap();
        dir
    }

    fn cleanup_test_dir(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_read_allowed_path() {
        let dir = setup_test_dir("read_ok");
        let sfs = SandboxedFs::new(vec![dir.clone()], false);
        let content = sfs
            .read_file(dir.join("test.txt").to_str().unwrap())
            .unwrap();
        assert_eq!(content, "hello world");
        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_read_disallowed_path() {
        let dir = setup_test_dir("read_deny");
        let sfs = SandboxedFs::new(vec![dir.clone()], false);
        let result = sfs.read_file("/etc/passwd");
        assert!(result.is_err());
        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_write_allowed_path() {
        let dir = setup_test_dir("write_ok");
        let sfs = SandboxedFs::new(vec![dir.clone()], true);
        let write_path = dir.join("output.txt");
        sfs.write_file(write_path.to_str().unwrap(), "written content")
            .unwrap();
        let content = std::fs::read_to_string(&write_path).unwrap();
        assert_eq!(content, "written content");
        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_write_disabled() {
        let dir = setup_test_dir("write_off");
        let sfs = SandboxedFs::new(vec![dir.clone()], false); // write_enabled = false
        let result = sfs.write_file(dir.join("output.txt").to_str().unwrap(), "data");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disabled"));
        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_traversal_rejected() {
        let dir = setup_test_dir("traversal");
        let sfs = SandboxedFs::new(vec![dir.clone()], false);
        let result = sfs.read_file(&format!("{}/../../../etc/passwd", dir.display()));
        assert!(result.is_err());
        cleanup_test_dir(&dir);
    }
}
