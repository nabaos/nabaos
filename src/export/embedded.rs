use std::path::{Path, PathBuf};

use crate::cache::semantic_cache::{CacheImplementation, CachedWork};
use crate::core::error::{NyayaError, Result};
use crate::export::target::ExportTarget;

/// An artifact produced by an embedded code generator.
#[derive(Debug, Clone)]
pub struct EmbeddedArtifact {
    pub output_dir: PathBuf,
    pub target: ExportTarget,
    pub artifact_path: PathBuf,
    pub build_instructions: String,
}

/// Generates a cross-compiled Rust project targeting Raspberry Pi (aarch64-unknown-linux-gnu).
pub struct RaspberryPiGenerator;

impl RaspberryPiGenerator {
    pub fn generate(entry: &CachedWork, output_dir: &Path) -> Result<EmbeddedArtifact> {
        let src_dir = output_dir.join("src");
        let cargo_dir = output_dir.join(".cargo");

        std::fs::create_dir_all(&src_dir)?;
        std::fs::create_dir_all(&cargo_dir)?;

        // Generate Cargo.toml
        let mut cargo_toml = String::new();
        cargo_toml.push_str("[package]\n");
        cargo_toml.push_str("name = \"export-embedded\"\n");
        cargo_toml.push_str("version = \"0.1.0\"\n");
        cargo_toml.push_str("edition = \"2021\"\n");
        cargo_toml.push('\n');
        cargo_toml.push_str("[dependencies]\n");
        cargo_toml.push_str("serde = { version = \"1\", features = [\"derive\"] }\n");
        cargo_toml.push_str("serde_json = \"1\"\n");
        std::fs::write(output_dir.join("Cargo.toml"), &cargo_toml)?;

        // Generate .cargo/config.toml
        let mut cargo_config = String::new();
        cargo_config.push_str("[target.aarch64-unknown-linux-gnu]\n");
        cargo_config.push_str("linker = \"aarch64-linux-gnu-gcc\"\n");
        std::fs::write(cargo_dir.join("config.toml"), &cargo_config)?;

        // Generate src/main.rs
        let main_rs = generate_rpi_main(entry)?;
        std::fs::write(src_dir.join("main.rs"), &main_rs)?;

        Ok(EmbeddedArtifact {
            output_dir: output_dir.to_path_buf(),
            target: ExportTarget::RaspberryPi,
            artifact_path: output_dir
                .join("target/aarch64-unknown-linux-gnu/release/export-embedded"),
            build_instructions: "cross build --target aarch64-unknown-linux-gnu --release"
                .to_string(),
        })
    }
}

fn generate_rpi_main(entry: &CachedWork) -> Result<String> {
    let entry_json = serde_json::to_string_pretty(entry)
        .map_err(|e| NyayaError::Export(format!("Failed to serialize entry: {}", e)))?;

    let body = match &entry.implementation {
        CacheImplementation::ToolSequence { steps } => {
            let steps_json = serde_json::to_string_pretty(steps)
                .map_err(|e| NyayaError::Export(format!("Failed to serialize steps: {}", e)))?;
            let mut s = String::new();
            s.push_str("    let steps = r####\"");
            s.push_str(&steps_json);
            s.push_str("\"####;\n");
            s.push_str("    println!(\"{}\", steps);");
            s
        }
        CacheImplementation::RustSource { code } => {
            let mut s = String::new();
            s.push_str("    // Embedded Rust source (from cache)\n");
            s.push_str("    fn cached_logic() {\n");
            s.push_str("        ");
            s.push_str(code);
            s.push_str("\n    }\n");
            s.push_str("    cached_logic();");
            s
        }
        CacheImplementation::Wasm { module_path } => {
            let mut s = String::new();
            s.push_str("    let wasm_path = r####\"");
            s.push_str(module_path);
            s.push_str("\"####;\n");
            s.push_str("    println!(\"Wasm module: {}\", wasm_path);");
            s
        }
    };

    let mut out = String::new();
    out.push_str("use std::env;\n\n");
    out.push_str("const CACHED_ENTRY: &str = r####\"");
    out.push_str(&entry_json);
    out.push_str("\"####;\n\n");
    out.push_str("fn main() {\n");
    out.push_str("    let args: Vec<String> = env::args().skip(1).collect();\n");
    out.push_str("    let mut params = std::collections::HashMap::new();\n\n");
    out.push_str("    if args.is_empty() {\n");
    out.push_str("        // Read from stdin\n");
    out.push_str("        let mut input = String::new();\n");
    out.push_str("        std::io::stdin().read_line(&mut input).ok();\n");
    out.push_str("        for part in input.trim().split_whitespace() {\n");
    out.push_str("            if let Some((k, v)) = part.split_once('=') {\n");
    out.push_str("                params.insert(k.to_string(), v.to_string());\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        for arg in &args {\n");
    out.push_str("            if let Some((k, v)) = arg.split_once('=') {\n");
    out.push_str("                params.insert(k.to_string(), v.to_string());\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
    out.push_str("    let _ = params;\n");
    out.push_str("    let _ = CACHED_ENTRY;\n\n");
    out.push_str(&body);
    out.push('\n');
    out.push_str("}\n");
    Ok(out)
}

/// Generates a no_std Wasm project targeting ESP32 (wasm32-unknown-unknown).
pub struct Esp32Generator;

impl Esp32Generator {
    pub fn generate(entry: &CachedWork, output_dir: &Path) -> Result<EmbeddedArtifact> {
        let src_dir = output_dir.join("src");
        std::fs::create_dir_all(&src_dir)?;

        let cargo_toml = Self::esp32_cargo_toml();
        std::fs::write(output_dir.join("Cargo.toml"), &cargo_toml)?;

        let lib_rs = match &entry.implementation {
            CacheImplementation::ToolSequence { steps } => generate_esp32_tool_sequence(steps),
            CacheImplementation::RustSource { code } => {
                let mut s = String::new();
                s.push_str("#![no_std]\n\n");
                s.push_str("// Embedded Rust source\n");
                s.push_str(code);
                s.push_str("\n\n");
                s.push_str("#[no_mangle]\n");
                s.push_str("pub extern \"C\" fn execute() -> i32 {\n");
                s.push_str("    0\n");
                s.push_str("}\n");
                s
            }
            CacheImplementation::Wasm { module_path } => {
                let mut s = String::new();
                s.push_str("#![no_std]\n\n");
                s.push_str("// Reference to existing Wasm module: ");
                s.push_str(module_path);
                s.push_str("\n\n");
                s.push_str("#[no_mangle]\n");
                s.push_str("pub extern \"C\" fn execute() -> i32 {\n");
                s.push_str("    0\n");
                s.push_str("}\n");
                s
            }
        };
        std::fs::write(src_dir.join("lib.rs"), &lib_rs)?;

        Ok(EmbeddedArtifact {
            output_dir: output_dir.to_path_buf(),
            target: ExportTarget::Esp32,
            artifact_path: output_dir
                .join("target/wasm32-unknown-unknown/release/export_wasm.wasm"),
            build_instructions: "cargo build --target wasm32-unknown-unknown --release".to_string(),
        })
    }

    fn esp32_cargo_toml() -> String {
        let mut s = String::new();
        s.push_str("[package]\n");
        s.push_str("name = \"export-wasm\"\n");
        s.push_str("version = \"0.1.0\"\n");
        s.push_str("edition = \"2021\"\n\n");
        s.push_str("[lib]\n");
        s.push_str("crate-type = [\"cdylib\"]\n\n");
        s.push_str("[profile.release]\n");
        s.push_str("opt-level = \"z\"\n");
        s.push_str("lto = true\n");
        s.push_str("strip = true\n");
        s
    }
}

fn generate_esp32_tool_sequence(steps: &[crate::cache::semantic_cache::ToolCall]) -> String {
    // Collect unique tool names, converting dots to underscores for valid identifiers
    let mut seen = std::collections::HashSet::new();
    let mut unique_tools = Vec::new();
    for step in steps {
        let fn_name = step.tool.replace('.', "_");
        if seen.insert(fn_name.clone()) {
            unique_tools.push(fn_name);
        }
    }

    let mut extern_fns = Vec::new();
    for name in &unique_tools {
        extern_fns.push(format!("    fn {}() -> i32;", name));
    }

    let mut calls = Vec::new();
    for step in steps {
        let fn_name = step.tool.replace('.', "_");
        calls.push(format!("    let _ = unsafe {{ {}() }};", fn_name));
    }

    let mut out = String::new();
    out.push_str("#![no_std]\n\n");
    out.push_str("extern \"C\" {\n");
    out.push_str(&extern_fns.join("\n"));
    out.push_str("\n}\n\n");
    out.push_str("#[no_mangle]\n");
    out.push_str("pub extern \"C\" fn execute() -> i32 {\n");
    out.push_str(&calls.join("\n"));
    out.push_str("\n    0\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::semantic_cache::{CacheImplementation, CachedWork, ToolCall};

    fn make_test_entry() -> CachedWork {
        CachedWork {
            id: "test-001".to_string(),
            description: "Test entry".to_string(),
            original_task: "Test task".to_string(),
            rationale: "Test rationale".to_string(),
            improvement_notes: None,
            parameters: vec![],
            implementation: CacheImplementation::ToolSequence {
                steps: vec![
                    ToolCall {
                        tool: "hw.temperature_read".to_string(),
                        args: serde_json::Map::new(),
                    },
                    ToolCall {
                        tool: "hw.led_write".to_string(),
                        args: serde_json::Map::new(),
                    },
                ],
            },
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
    fn test_rpi_generates_cargo_config_with_arm_linker() {
        let entry = make_test_entry();
        let dir = tempfile::tempdir().unwrap();
        let artifact = RaspberryPiGenerator::generate(&entry, dir.path()).unwrap();

        let config_path = artifact.output_dir.join(".cargo").join("config.toml");
        let config_content = std::fs::read_to_string(config_path).unwrap();
        assert!(config_content.contains("aarch64-linux-gnu-gcc"));
        assert!(config_content.contains("[target.aarch64-unknown-linux-gnu]"));
    }

    #[test]
    fn test_esp32_generates_no_std_lib() {
        let entry = make_test_entry();
        let dir = tempfile::tempdir().unwrap();
        let _artifact = Esp32Generator::generate(&entry, dir.path()).unwrap();

        let lib_path = dir.path().join("src").join("lib.rs");
        let lib_content = std::fs::read_to_string(lib_path).unwrap();
        assert!(lib_content.contains("#![no_std]"));
    }

    #[test]
    fn test_esp32_cargo_toml_has_cdylib_crate_type() {
        let entry = make_test_entry();
        let dir = tempfile::tempdir().unwrap();
        let _artifact = Esp32Generator::generate(&entry, dir.path()).unwrap();

        let cargo_path = dir.path().join("Cargo.toml");
        let cargo_content = std::fs::read_to_string(cargo_path).unwrap();
        assert!(cargo_content.contains(r#"crate-type = ["cdylib"]"#));
    }

    #[test]
    fn test_wasm_entry_esp32_generates_minimal_wrapper() {
        let mut entry = make_test_entry();
        entry.implementation = CacheImplementation::Wasm {
            module_path: "/path/to/existing.wasm".to_string(),
        };
        let dir = tempfile::tempdir().unwrap();
        let _artifact = Esp32Generator::generate(&entry, dir.path()).unwrap();

        let lib_path = dir.path().join("src").join("lib.rs");
        let lib_content = std::fs::read_to_string(lib_path).unwrap();
        assert!(lib_content.contains("#![no_std]"));
        assert!(lib_content.contains("/path/to/existing.wasm"));
        assert!(lib_content.contains(r#"pub extern "C" fn execute()"#));
    }

    #[test]
    fn test_build_instructions_correct_per_target() {
        let entry = make_test_entry();

        let rpi_dir = tempfile::tempdir().unwrap();
        let rpi = RaspberryPiGenerator::generate(&entry, rpi_dir.path()).unwrap();
        assert!(rpi.build_instructions.contains("cross build"));
        assert!(rpi.build_instructions.contains("aarch64-unknown-linux-gnu"));

        let esp_dir = tempfile::tempdir().unwrap();
        let esp = Esp32Generator::generate(&entry, esp_dir.path()).unwrap();
        assert!(esp
            .build_instructions
            .contains("cargo build --target wasm32"));
    }

    #[test]
    fn test_unique_tools_deduplicated_in_host_imports() {
        let mut entry = make_test_entry();
        entry.implementation = CacheImplementation::ToolSequence {
            steps: vec![
                ToolCall {
                    tool: "hw.temperature_read".to_string(),
                    args: serde_json::Map::new(),
                },
                ToolCall {
                    tool: "hw.temperature_read".to_string(),
                    args: serde_json::Map::new(),
                },
                ToolCall {
                    tool: "hw.led_write".to_string(),
                    args: serde_json::Map::new(),
                },
            ],
        };
        let dir = tempfile::tempdir().unwrap();
        let _artifact = Esp32Generator::generate(&entry, dir.path()).unwrap();

        let lib_path = dir.path().join("src").join("lib.rs");
        let lib_content = std::fs::read_to_string(lib_path).unwrap();
        // Count occurrences of the function declaration in the extern block
        let import_count = lib_content
            .matches("fn hw_temperature_read() -> i32;")
            .count();
        assert_eq!(
            import_count, 1,
            "hw_temperature_read should appear only once in imports"
        );

        // But the call should appear twice (once for each step)
        let call_count = lib_content.matches("hw_temperature_read()").count();
        assert!(
            call_count >= 2,
            "hw_temperature_read should be called twice"
        );
    }
}
