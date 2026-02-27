// src/export/container.rs
// Cloud Run container generator — produces a deployable Dockerfile + Rust service
// from a CachedWork entry.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::semantic_cache::{CacheImplementation, CachedWork};
use crate::core::error::{NyayaError, Result};

/// Artifact paths produced by the Cloud Run generator.
#[derive(Debug, Clone)]
pub struct CloudRunArtifact {
    pub output_dir: PathBuf,
    pub dockerfile: PathBuf,
    pub main_rs: PathBuf,
    pub cargo_toml: PathBuf,
}

/// Stateless generator that produces a Cloud Run-deployable container project
/// from a single `CachedWork` entry.
pub struct CloudRunGenerator;

impl CloudRunGenerator {
    /// Generate a complete Cloud Run project directory from a cached work entry.
    pub fn generate(entry: &CachedWork, output_dir: &Path) -> Result<CloudRunArtifact> {
        // 1. Create output_dir/ and output_dir/src/
        fs::create_dir_all(output_dir.join("src"))
            .map_err(|e| NyayaError::Export(format!("Failed to create output directories: {e}")))?;

        let dockerfile_path = output_dir.join("Dockerfile");
        let cargo_toml_path = output_dir.join("Cargo.toml");
        let main_rs_path = output_dir.join("src").join("main.rs");

        // 2. Generate Dockerfile
        fs::write(&dockerfile_path, Self::generate_dockerfile())
            .map_err(|e| NyayaError::Export(format!("Failed to write Dockerfile: {e}")))?;

        // 3. Generate Cargo.toml
        fs::write(&cargo_toml_path, Self::generate_cargo_toml())
            .map_err(|e| NyayaError::Export(format!("Failed to write Cargo.toml: {e}")))?;

        // 4. Generate src/main.rs
        let entry_json = serde_json::to_string_pretty(entry).map_err(|e| {
            NyayaError::Export(format!("Failed to serialize CachedWork entry: {e}"))
        })?;
        fs::write(&main_rs_path, Self::generate_main_rs(entry, &entry_json))
            .map_err(|e| NyayaError::Export(format!("Failed to write main.rs: {e}")))?;

        Ok(CloudRunArtifact {
            output_dir: output_dir.to_path_buf(),
            dockerfile: dockerfile_path,
            main_rs: main_rs_path,
            cargo_toml: cargo_toml_path,
        })
    }

    fn generate_dockerfile() -> String {
        let mut s = String::new();
        s.push_str("# --- Build stage ---\n");
        s.push_str("FROM rust:1.80-slim AS builder\n");
        s.push_str("WORKDIR /app\n");
        s.push_str("COPY Cargo.toml Cargo.lock* ./\n");
        s.push_str("COPY src/ src/\n");
        s.push_str("RUN cargo build --release\n");
        s.push('\n');
        s.push_str("# --- Runtime stage ---\n");
        s.push_str("FROM gcr.io/distroless/cc-debian12\n");
        s.push_str("COPY --from=builder /app/target/release/export-service /\n");
        s.push_str("EXPOSE 8080\n");
        s.push_str("ENTRYPOINT [\"/export-service\"]\n");
        s
    }

    fn generate_cargo_toml() -> String {
        let mut s = String::new();
        s.push_str("[package]\n");
        s.push_str("name = \"export-service\"\n");
        s.push_str("version = \"0.1.0\"\n");
        s.push_str("edition = \"2021\"\n");
        s.push('\n');
        s.push_str("[dependencies]\n");
        s.push_str("serde = { version = \"1\", features = [\"derive\"] }\n");
        s.push_str("serde_json = \"1\"\n");
        s.push_str("tiny-http = \"0.12\"\n");
        s
    }

    fn generate_main_rs(entry: &CachedWork, entry_json: &str) -> String {
        let execute_body = Self::generate_execute_handler(entry);

        let mut s = String::new();
        s.push_str("use std::env;\n\n");

        // Use raw string literal to safely embed JSON without escaping issues
        s.push_str("const ENTRY_JSON: &str = r####\"");
        s.push_str(entry_json);
        s.push_str("\"####;\n\n");

        s.push_str("fn main() {\n");
        s.push_str("    let port = env::var(\"PORT\").unwrap_or_else(|_| \"8080\".to_string());\n");
        s.push_str("    let addr = format!(\"0.0.0.0:{}\", port);\n");
        s.push_str("    let server = tiny_http::Server::http(&addr)\n");
        s.push_str("        .expect(\"Failed to start server\");\n\n");
        s.push_str("    eprintln!(\"export-service listening on {}\", addr);\n\n");
        s.push_str("    for request in server.incoming_requests() {\n");
        s.push_str("        match request.url() {\n");

        // /health handler
        s.push_str("            \"/health\" => {\n");
        s.push_str("                let response = tiny_http::Response::from_string(\"ok\");\n");
        s.push_str("                let _ = request.respond(response);\n");
        s.push_str("            }\n");

        // /meta handler
        s.push_str("            \"/meta\" => {\n");
        s.push_str("                let response = tiny_http::Response::from_string(ENTRY_JSON)\n");
        s.push_str("                    .with_header(\"Content-Type: application/json\".parse::<tiny_http::Header>().unwrap());\n");
        s.push_str("                let _ = request.respond(response);\n");
        s.push_str("            }\n");

        // /execute handler
        s.push_str("            \"/execute\" => {\n");
        s.push_str(&execute_body);
        s.push_str("            }\n");

        // default 404
        s.push_str("            _ => {\n");
        s.push_str(
            "                let response = tiny_http::Response::from_string(\"not found\")\n",
        );
        s.push_str("                    .with_status_code(404);\n");
        s.push_str("                let _ = request.respond(response);\n");
        s.push_str("            }\n");

        s.push_str("        }\n");
        s.push_str("    }\n");
        s.push_str("}\n");
        s
    }

    fn generate_execute_handler(entry: &CachedWork) -> String {
        let mut s = String::new();
        match &entry.implementation {
            CacheImplementation::ToolSequence { steps } => {
                let steps_json = serde_json::to_string_pretty(steps).unwrap_or_default();
                s.push_str("                let body = r####\"");
                s.push_str(&steps_json);
                s.push_str("\"####;\n");
                s.push_str(
                    "                let response = tiny_http::Response::from_string(body)\n",
                );
                s.push_str("                    .with_header(\"Content-Type: application/json\".parse::<tiny_http::Header>().unwrap());\n");
                s.push_str("                let _ = request.respond(response);\n");
            }
            CacheImplementation::RustSource { code } => {
                s.push_str("                let code = r####\"");
                s.push_str(code);
                s.push_str("\"####;\n");
                s.push_str("                let body = format!(r#\"{{\"type\":\"rust_source\",\"code\":\"{}\"}}\")\"#, code);\n");
                s.push_str(
                    "                let response = tiny_http::Response::from_string(body)\n",
                );
                s.push_str("                    .with_header(\"Content-Type: application/json\".parse::<tiny_http::Header>().unwrap());\n");
                s.push_str("                let _ = request.respond(response);\n");
            }
            CacheImplementation::Wasm { .. } => {
                s.push_str("                let body = \"{\\\"message\\\":\\\"Wasm runtime required. Deploy with wasmtime to execute.\\\"}\";\n");
                s.push_str(
                    "                let response = tiny_http::Response::from_string(body)\n",
                );
                s.push_str("                    .with_header(\"Content-Type: application/json\".parse::<tiny_http::Header>().unwrap());\n");
                s.push_str("                let _ = request.respond(response);\n");
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::semantic_cache::{CacheImplementation, CachedWork};
    use tempfile::TempDir;

    fn make_test_entry() -> CachedWork {
        CachedWork {
            id: "test-001".to_string(),
            description: "Test entry".to_string(),
            original_task: "Test task".to_string(),
            rationale: "Test rationale".to_string(),
            improvement_notes: None,
            parameters: vec![],
            implementation: CacheImplementation::ToolSequence { steps: vec![] },
            hit_count: 0,
            success_count: 0,
            failure_count: 0,
            created_at: 0,
            last_used_at: 0,
            similarity_threshold: 0.92,
            enabled: true,
        }
    }

    #[test]
    fn generates_all_three_files() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("cloud-run-output");
        let entry = make_test_entry();

        let artifact = CloudRunGenerator::generate(&entry, &out).unwrap();

        assert!(artifact.dockerfile.exists(), "Dockerfile should exist");
        assert!(artifact.cargo_toml.exists(), "Cargo.toml should exist");
        assert!(artifact.main_rs.exists(), "src/main.rs should exist");
    }

    #[test]
    fn dockerfile_contains_expected_directives() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("cloud-run-output");
        let entry = make_test_entry();

        let artifact = CloudRunGenerator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.dockerfile).unwrap();

        assert!(
            content.contains("FROM rust:1.80-slim"),
            "Dockerfile should contain rust builder stage"
        );
        assert!(
            content.contains("EXPOSE 8080"),
            "Dockerfile should expose port 8080"
        );
    }

    #[test]
    fn main_rs_contains_embedded_entry_json() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("cloud-run-output");
        let entry = make_test_entry();

        let artifact = CloudRunGenerator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.main_rs).unwrap();

        assert!(
            content.contains("test-001"),
            "main.rs should contain the entry id"
        );
        assert!(
            content.contains("Test entry"),
            "main.rs should contain the entry description"
        );
        assert!(
            content.contains("ENTRY_JSON"),
            "main.rs should define ENTRY_JSON constant"
        );
    }

    #[test]
    fn cargo_toml_has_tiny_http_dependency() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("cloud-run-output");
        let entry = make_test_entry();

        let artifact = CloudRunGenerator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.cargo_toml).unwrap();

        assert!(
            content.contains("tiny-http"),
            "Cargo.toml should list tiny-http dependency"
        );
    }

    #[test]
    fn output_dir_structure_correct() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("cloud-run-output");
        let entry = make_test_entry();

        let _artifact = CloudRunGenerator::generate(&entry, &out).unwrap();

        assert!(out.exists(), "output_dir should exist");
        assert!(out.join("src").exists(), "src/ subdirectory should exist");
        assert!(out.join("src").is_dir(), "src/ should be a directory");
        assert!(out.join("Dockerfile").is_file());
        assert!(out.join("Cargo.toml").is_file());
        assert!(out.join("src").join("main.rs").is_file());
    }
}
