use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// YAML frontmatter parsed from a KB markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KBFrontmatter {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub priority: Option<i32>,
}

/// A single knowledge base entry loaded from a markdown file.
#[derive(Debug, Clone)]
pub struct KBEntry {
    pub title: String,
    pub tags: Vec<String>,
    pub priority: i32,
    pub content: String,
    pub source: String,
}

/// Parse YAML frontmatter from a markdown string.
///
/// If the file starts with `---\n`, extract YAML between the first and second
/// `---\n` delimiters. Returns parsed frontmatter and the remaining body text.
/// If no frontmatter is found, returns `(None, full_text)`.
pub fn parse_frontmatter(raw: &str) -> (Option<KBFrontmatter>, &str) {
    if !raw.starts_with("---\n") {
        return (None, raw);
    }

    let after_first = &raw[4..]; // skip "---\n"
    if let Some(end_idx) = after_first.find("---\n") {
        let yaml_block = &after_first[..end_idx];
        let body = &after_first[end_idx + 4..];
        match serde_yaml::from_str::<KBFrontmatter>(yaml_block) {
            Ok(fm) => (Some(fm), body),
            Err(_) => (None, raw),
        }
    } else {
        (None, raw)
    }
}

/// Title-case a filename stem: replace `-` and `_` with spaces, capitalize
/// the first letter of each word.
fn title_case_stem(stem: &str) -> String {
    stem.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Load a single KB markdown file from disk.
///
/// Parses frontmatter if present. Falls back to a title-cased version of the
/// filename stem. Tags default to empty, priority defaults to 0.
pub fn load_kb_file(path: &Path) -> crate::core::error::Result<KBEntry> {
    let raw = std::fs::read_to_string(path)?;
    let (fm, body) = parse_frontmatter(&raw);

    let default_title = path
        .file_stem()
        .map(|s| title_case_stem(&s.to_string_lossy()))
        .unwrap_or_default();

    let fm = fm.unwrap_or_default();

    Ok(KBEntry {
        title: fm.title.unwrap_or(default_title),
        tags: fm.tags.unwrap_or_default(),
        priority: fm.priority.unwrap_or(0),
        content: body.to_string(),
        source: path.display().to_string(),
    })
}

/// Load all `.md` files from a directory, returning entries.
/// Returns an empty vec if the directory does not exist or cannot be read.
fn load_md_files(dir: &Path) -> Vec<KBEntry> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(kb) = load_kb_file(&path) {
                entries.push(kb);
            }
        }
    }
    entries
}

/// Load KB entries from the standard directory layout.
///
/// Loads files from:
/// - `base/_shared/*.md`
/// - `base/_default/*.md`
/// - `base/{agent_id}/*.md` (if agent_id is not `_default` or `_shared`)
///
/// Missing directories are silently skipped. Entries are sorted by priority
/// descending. Deduplication by title gives agent-specific entries priority
/// over `_default`, which wins over `_shared`.
pub fn load_kb_dir(base: &Path, agent_id: &str) -> Vec<KBEntry> {
    // Load in order of lowest to highest precedence.
    // Later entries with the same title overwrite earlier ones.
    let mut by_title: HashMap<String, KBEntry> = HashMap::new();

    // _shared (lowest precedence)
    for entry in load_md_files(&base.join("_shared")) {
        by_title.insert(entry.title.clone(), entry);
    }

    // _default (medium precedence)
    for entry in load_md_files(&base.join("_default")) {
        by_title.insert(entry.title.clone(), entry);
    }

    // agent-specific (highest precedence)
    if agent_id != "_default" && agent_id != "_shared" {
        for entry in load_md_files(&base.join(agent_id)) {
            by_title.insert(entry.title.clone(), entry);
        }
    }

    let mut entries: Vec<KBEntry> = by_title.into_values().collect();
    entries.sort_by(|a, b| b.priority.cmp(&a.priority));
    entries
}

/// Compile KB entries into a single string suitable for LLM context injection.
///
/// Returns an empty string if there are no entries. Otherwise produces:
/// ```text
/// KNOWLEDGE BASE:
///
/// ## Title
/// content
///
/// ## Title 2
/// content 2
/// ```
pub fn compile_kb(entries: &[KBEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut out = String::from("KNOWLEDGE BASE:\n\n");
    for entry in entries {
        out.push_str(&format!("## {}\n{}\n\n", entry.title, entry.content));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_frontmatter_full() {
        let md = "---\ntitle: My Title\ntags:\n  - rust\n  - agent\npriority: 10\n---\nBody text here.\n";
        let (fm, body) = parse_frontmatter(md);
        let fm = fm.expect("should parse frontmatter");
        assert_eq!(fm.title.as_deref(), Some("My Title"));
        assert_eq!(
            fm.tags.as_deref(),
            Some(&["rust".to_string(), "agent".to_string()][..])
        );
        assert_eq!(fm.priority, Some(10));
        assert_eq!(body, "Body text here.\n");
    }

    #[test]
    fn test_parse_frontmatter_partial() {
        let md = "---\ntitle: Only Title\n---\nSome content.\n";
        let (fm, body) = parse_frontmatter(md);
        let fm = fm.expect("should parse frontmatter");
        assert_eq!(fm.title.as_deref(), Some("Only Title"));
        assert!(fm.tags.is_none());
        assert!(fm.priority.is_none());
        assert_eq!(body, "Some content.\n");
    }

    #[test]
    fn test_parse_frontmatter_none() {
        let md = "# Just a heading\n\nSome plain markdown.\n";
        let (fm, body) = parse_frontmatter(md);
        assert!(fm.is_none());
        assert_eq!(body, md);
    }

    #[test]
    fn test_load_kb_file_with_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test-entry.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "---\ntitle: Custom Title\ntags:\n  - demo\npriority: 5\n---\nHello world."
        )
        .unwrap();
        drop(f);

        let entry = load_kb_file(&path).unwrap();
        assert_eq!(entry.title, "Custom Title");
        assert_eq!(entry.tags, vec!["demo".to_string()]);
        assert_eq!(entry.priority, 5);
        assert!(entry.content.contains("Hello world."));
        assert!(entry.source.contains("test-entry.md"));
    }

    #[test]
    fn test_load_kb_file_no_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("my-cool_doc.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "# Heading\n\nPlain content.").unwrap();
        drop(f);

        let entry = load_kb_file(&path).unwrap();
        assert_eq!(entry.title, "My Cool Doc");
        assert!(entry.tags.is_empty());
        assert_eq!(entry.priority, 0);
    }

    #[test]
    fn test_load_kb_dir_shared_and_agent() {
        let dir = TempDir::new().unwrap();

        // Create _shared dir with a low-priority entry
        let shared_dir = dir.path().join("_shared");
        std::fs::create_dir_all(&shared_dir).unwrap();
        let mut f = std::fs::File::create(shared_dir.join("shared-info.md")).unwrap();
        writeln!(
            f,
            "---\ntitle: Shared Info\npriority: 1\n---\nShared content."
        )
        .unwrap();
        drop(f);

        // Create agent-specific dir with a high-priority entry
        let agent_dir = dir.path().join("test_agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let mut f = std::fs::File::create(agent_dir.join("agent-info.md")).unwrap();
        writeln!(
            f,
            "---\ntitle: Agent Info\npriority: 10\n---\nAgent content."
        )
        .unwrap();
        drop(f);

        let entries = load_kb_dir(dir.path(), "test_agent");
        assert_eq!(entries.len(), 2);
        // Sorted by priority descending: Agent Info (10) first, Shared Info (1) second
        assert_eq!(entries[0].title, "Agent Info");
        assert_eq!(entries[1].title, "Shared Info");
    }

    #[test]
    fn test_load_kb_dir_missing_dir() {
        let entries = load_kb_dir(Path::new("/nonexistent/kb/path"), "any_agent");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_compile_kb_entries() {
        let entries = vec![
            KBEntry {
                title: "First".to_string(),
                tags: vec![],
                priority: 10,
                content: "Content A".to_string(),
                source: "a.md".to_string(),
            },
            KBEntry {
                title: "Second".to_string(),
                tags: vec![],
                priority: 5,
                content: "Content B".to_string(),
                source: "b.md".to_string(),
            },
        ];

        let compiled = compile_kb(&entries);
        assert!(compiled.starts_with("KNOWLEDGE BASE:\n\n"));
        assert!(compiled.contains("## First\nContent A"));
        assert!(compiled.contains("## Second\nContent B"));
        // Should not end with trailing whitespace
        assert_eq!(compiled, compiled.trim_end());

        // Empty entries returns empty string
        let empty = compile_kb(&[]);
        assert!(empty.is_empty());
    }
}
