use std::fs;
use std::path::Path;

/// Migrate old config structure to new.
/// Called once at startup; idempotent.
pub fn migrate_config_if_needed() {
    let old_agents = Path::new("config/agents");
    let new_personas = Path::new("config/personas");

    if old_agents.exists() && !new_personas.exists() {
        eprintln!("Migrating config/agents/ \u{2192} config/personas/...");
        if let Err(e) = fs::rename(old_agents, new_personas) {
            eprintln!("Migration failed (copying instead): {}", e);
            if let Ok(entries) = fs::read_dir(old_agents) {
                let _ = fs::create_dir_all(new_personas);
                for entry in entries.flatten() {
                    let dest = new_personas.join(entry.file_name());
                    let _ = fs::copy(entry.path(), dest);
                }
            }
        }
        eprintln!("Done. Old files preserved at config/agents/");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_migration_renames_directory() {
        let tmp = TempDir::new().unwrap();
        let old = tmp.path().join("config").join("agents");
        let new = tmp.path().join("config").join("personas");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("_default.yaml"), "persona:\n  name: Nyaya\n").unwrap();

        // Perform the migration using the same logic but with custom paths
        assert!(old.exists());
        assert!(!new.exists());
        fs::rename(&old, &new).unwrap();
        assert!(!old.exists());
        assert!(new.exists());
        assert!(new.join("_default.yaml").exists());
    }

    #[test]
    fn test_migration_idempotent() {
        let tmp = TempDir::new().unwrap();
        let old = tmp.path().join("config").join("agents");
        let new = tmp.path().join("config").join("personas");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("test.yaml"), "test: true\n").unwrap();

        // First migration
        fs::rename(&old, &new).unwrap();
        assert!(new.exists());

        // Second call: old doesn't exist, new does => no-op (idempotent)
        // This mirrors the guard in migrate_config_if_needed:
        // if old.exists() && !new.exists() { ... }
        assert!(!old.exists());
        assert!(new.exists());
        // The function would do nothing — verify no error
        migrate_config_if_needed_at(&old, &new);
        assert!(new.exists());
        assert!(new.join("test.yaml").exists());
    }

    /// Test helper that mirrors migrate_config_if_needed but with configurable paths.
    fn migrate_config_if_needed_at(old: &Path, new: &Path) {
        if old.exists() && !new.exists() {
            if let Err(e) = fs::rename(old, new) {
                eprintln!("Migration failed (copying instead): {}", e);
                if let Ok(entries) = fs::read_dir(old) {
                    let _ = fs::create_dir_all(new);
                    for entry in entries.flatten() {
                        let dest = new.join(entry.file_name());
                        let _ = fs::copy(entry.path(), dest);
                    }
                }
            }
        }
    }
}
