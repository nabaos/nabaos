// src/export/ros2.rs
// ROS 2 package generator — produces a complete ROS 2 Rust package from a CachedWork entry.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::semantic_cache::{CacheImplementation, CachedWork, ParamType};
use crate::core::error::{NyayaError, Result};

/// An artifact produced by the ROS 2 package generator.
#[derive(Debug, Clone)]
pub struct Ros2Artifact {
    pub output_dir: PathBuf,
    pub package_name: String,
    pub package_xml: PathBuf,
    pub cargo_toml: PathBuf,
    pub launch_file: PathBuf,
    pub node_src: PathBuf,
    pub build_instructions: String,
}

/// Stateless generator that produces a ROS 2 package from a single `CachedWork` entry.
pub struct Ros2Generator;

impl Ros2Generator {
    /// Generate a complete ROS 2 package directory from a cached work entry.
    pub fn generate(entry: &CachedWork, output_dir: &Path) -> Result<Ros2Artifact> {
        let package_name = sanitize_package_name(&entry.id);

        // Create directory layout
        let src_dir = output_dir.join("src");
        let launch_dir = output_dir.join("launch");
        let config_dir = output_dir.join("config");

        fs::create_dir_all(&src_dir)
            .map_err(|e| NyayaError::Export(format!("Failed to create src directory: {e}")))?;
        fs::create_dir_all(&launch_dir)
            .map_err(|e| NyayaError::Export(format!("Failed to create launch directory: {e}")))?;
        fs::create_dir_all(&config_dir)
            .map_err(|e| NyayaError::Export(format!("Failed to create config directory: {e}")))?;

        // Generate package.xml
        let package_xml_path = output_dir.join("package.xml");
        fs::write(
            &package_xml_path,
            generate_package_xml(&package_name, &entry.description),
        )
        .map_err(|e| NyayaError::Export(format!("Failed to write package.xml: {e}")))?;

        // Generate Cargo.toml
        let cargo_toml_path = output_dir.join("Cargo.toml");
        fs::write(&cargo_toml_path, generate_cargo_toml(&package_name))
            .map_err(|e| NyayaError::Export(format!("Failed to write Cargo.toml: {e}")))?;

        // Generate src/main.rs
        let main_rs_path = src_dir.join("main.rs");
        let main_rs = generate_main_rs(entry, &package_name)?;
        fs::write(&main_rs_path, &main_rs)
            .map_err(|e| NyayaError::Export(format!("Failed to write main.rs: {e}")))?;

        // Generate launch/main.launch.py
        let launch_file_path = launch_dir.join("main.launch.py");
        fs::write(&launch_file_path, generate_launch_file(&package_name))
            .map_err(|e| NyayaError::Export(format!("Failed to write launch file: {e}")))?;

        // Generate config/params.yaml
        let params_yaml_path = config_dir.join("params.yaml");
        fs::write(
            &params_yaml_path,
            generate_params_yaml(entry, &package_name),
        )
        .map_err(|e| NyayaError::Export(format!("Failed to write params.yaml: {e}")))?;

        let build_instructions = format!(
            "cd <workspace_root> && colcon build --packages-select {}",
            package_name
        );

        Ok(Ros2Artifact {
            output_dir: output_dir.to_path_buf(),
            package_name,
            package_xml: package_xml_path,
            cargo_toml: cargo_toml_path,
            launch_file: launch_file_path,
            node_src: main_rs_path,
            build_instructions,
        })
    }
}

/// Sanitize an ID into a valid ROS 2 package name: lowercase, non-alphanumeric → `_`.
fn sanitize_package_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Return the ROS 2 topic name for a tool, stripping `hw.` prefix and `_read`/`_write`/`_bus` suffix.
fn topic_name_for_tool(tool: &str) -> String {
    let name = tool.strip_prefix("hw.").unwrap_or(tool);
    let name = name
        .strip_suffix("_read")
        .or_else(|| name.strip_suffix("_write"))
        .or_else(|| name.strip_suffix("_bus"))
        .unwrap_or(name);
    format!("/nyaya/{}", name)
}

/// Returns true if the tool name represents a read ability (ends with `_read` or `_bus`).
fn is_read_ability(tool: &str) -> bool {
    tool.ends_with("_read") || tool.ends_with("_bus")
}

fn generate_package_xml(package_name: &str, description: &str) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\"?>\n");
    s.push_str("<package format=\"3\">\n");
    s.push_str(&format!("  <name>{}</name>\n", package_name));
    s.push_str("  <version>0.1.0</version>\n");
    s.push_str(&format!("  <description>{}</description>\n", description));
    s.push_str("  <maintainer email=\"user@example.com\">nyaya-export</maintainer>\n");
    s.push_str("  <license>MIT</license>\n");
    s.push_str("  <buildtool_depend>ament_cargo</buildtool_depend>\n");
    s.push_str("  <depend>std_msgs</depend>\n");
    s.push_str("  <depend>sensor_msgs</depend>\n");
    s.push_str("</package>\n");
    s
}

fn generate_cargo_toml(package_name: &str) -> String {
    let mut s = String::new();
    s.push_str("[package]\n");
    s.push_str(&format!("name = \"{}\"\n", package_name));
    s.push_str("version = \"0.1.0\"\n");
    s.push_str("edition = \"2021\"\n");
    s.push('\n');
    s.push_str("[dependencies]\n");
    s.push_str("r2r = \"0.9\"\n");
    s.push_str("serde = { version = \"1\", features = [\"derive\"] }\n");
    s.push_str("serde_json = \"1\"\n");
    s.push_str("tokio = { version = \"1\", features = [\"full\"] }\n");
    s
}

fn generate_main_rs(entry: &CachedWork, package_name: &str) -> Result<String> {
    let entry_json = serde_json::to_string_pretty(entry)
        .map_err(|e| NyayaError::Export(format!("Failed to serialize entry: {e}")))?;

    let node_name = format!("nyaya_{}_node", package_name);

    let body = match &entry.implementation {
        CacheImplementation::ToolSequence { steps } => {
            generate_tool_sequence_body(steps, &node_name)
        }
        CacheImplementation::RustSource { code } => generate_rust_source_body(code, &node_name),
        CacheImplementation::Wasm { module_path } => generate_wasm_body(module_path, &node_name),
    };

    let mut out = String::new();
    out.push_str("use std::sync::{Arc, Mutex};\n\n");
    out.push_str("const CACHED_ENTRY: &str = r####\"");
    out.push_str(&entry_json);
    out.push_str("\"####;\n\n");
    out.push_str(&body);
    Ok(out)
}

fn generate_tool_sequence_body(
    steps: &[crate::cache::semantic_cache::ToolCall],
    node_name: &str,
) -> String {
    // Collect unique tool names preserving order
    let mut seen = HashSet::new();
    let mut unique_tools = Vec::new();
    for step in steps {
        if seen.insert(step.tool.clone()) {
            unique_tools.push(step.tool.clone());
        }
    }

    let mut s = String::new();
    s.push_str("#[tokio::main]\n");
    s.push_str("async fn main() -> Result<(), Box<dyn std::error::Error>> {\n");
    s.push_str("    let ctx = r2r::Context::create()?;\n");
    s.push_str(&format!(
        "    let mut node = r2r::Node::create(ctx, \"{}\", \"\")?;\n",
        node_name
    ));
    s.push_str("    let _ = CACHED_ENTRY;\n\n");

    // Create publishers for read tools and service servers for write tools
    for tool in &unique_tools {
        let topic = topic_name_for_tool(tool);
        let var_name = tool.replace(['.', '-'], "_");
        if is_read_ability(tool) {
            s.push_str(&format!(
                "    let {var_name}_pub = node.create_publisher::<r2r::std_msgs::msg::Float64>(\"{topic}\", r2r::QosProfile::default())?;\n"
            ));
        } else {
            s.push_str(&format!(
                "    let _{var_name}_srv = node.create_service::<r2r::std_srvs::srv::SetBool>(\"{topic}/set\")?;\n"
            ));
        }
    }

    s.push('\n');
    s.push_str("    // Timer callback at 1Hz\n");
    s.push_str("    let timer = node.create_wall_timer(std::time::Duration::from_secs(1))?;\n");
    s.push_str("    let handle = tokio::task::spawn(async move {\n");
    s.push_str("        loop {\n");
    s.push_str("            match timer.tick().await {\n");
    s.push_str("                Ok(_) => {\n");

    // Execute steps in order, publishing for read tools
    for step in steps {
        let var_name = step.tool.replace(['.', '-'], "_");
        if is_read_ability(&step.tool) {
            s.push_str(
                "                    let msg = r2r::std_msgs::msg::Float64 { data: 0.0 };\n",
            );
            s.push_str(&format!(
                "                    let _ = {var_name}_pub.publish(&msg);\n"
            ));
        } else {
            s.push_str(&format!(
                "                    // Write tool: {}\n",
                step.tool
            ));
        }
    }

    s.push_str("                }\n");
    s.push_str("                Err(_) => break,\n");
    s.push_str("            }\n");
    s.push_str("        }\n");
    s.push_str("    });\n\n");
    s.push_str("    // Spin the node\n");
    s.push_str("    loop {\n");
    s.push_str("        node.spin_once(std::time::Duration::from_millis(100));\n");
    s.push_str("        if handle.is_finished() {\n");
    s.push_str("            break;\n");
    s.push_str("        }\n");
    s.push_str("    }\n\n");
    s.push_str("    Ok(())\n");
    s.push_str("}\n");
    s
}

fn generate_rust_source_body(code: &str, node_name: &str) -> String {
    let mut s = String::new();
    s.push_str("fn cached_logic() {\n");
    s.push_str("    ");
    s.push_str(code);
    s.push_str("\n}\n\n");
    s.push_str("#[tokio::main]\n");
    s.push_str("async fn main() -> Result<(), Box<dyn std::error::Error>> {\n");
    s.push_str("    let ctx = r2r::Context::create()?;\n");
    s.push_str(&format!(
        "    let mut node = r2r::Node::create(ctx, \"{}\", \"\")?;\n",
        node_name
    ));
    s.push_str("    let _ = CACHED_ENTRY;\n\n");
    s.push_str("    let timer = node.create_wall_timer(std::time::Duration::from_secs(1))?;\n");
    s.push_str("    let handle = tokio::task::spawn(async move {\n");
    s.push_str("        loop {\n");
    s.push_str("            match timer.tick().await {\n");
    s.push_str("                Ok(_) => cached_logic(),\n");
    s.push_str("                Err(_) => break,\n");
    s.push_str("            }\n");
    s.push_str("        }\n");
    s.push_str("    });\n\n");
    s.push_str("    loop {\n");
    s.push_str("        node.spin_once(std::time::Duration::from_millis(100));\n");
    s.push_str("        if handle.is_finished() {\n");
    s.push_str("            break;\n");
    s.push_str("        }\n");
    s.push_str("    }\n\n");
    s.push_str("    Ok(())\n");
    s.push_str("}\n");
    s
}

fn generate_wasm_body(module_path: &str, node_name: &str) -> String {
    let mut s = String::new();
    s.push_str("// NOTE: Wasm runtime bridge required.\n");
    s.push_str(&format!("// Wasm module path: {}\n", module_path));
    s.push_str(
        "// This node needs a wasm runtime (e.g. wasmtime) to execute the cached module.\n\n",
    );
    s.push_str("#[tokio::main]\n");
    s.push_str("async fn main() -> Result<(), Box<dyn std::error::Error>> {\n");
    s.push_str("    let ctx = r2r::Context::create()?;\n");
    s.push_str(&format!(
        "    let mut node = r2r::Node::create(ctx, \"{}\", \"\")?;\n",
        node_name
    ));
    s.push_str("    let _ = CACHED_ENTRY;\n\n");
    s.push_str(&format!(
        "    let _wasm_path = r####\"{}\"####;\n",
        module_path
    ));
    s.push_str("    eprintln!(\"Wasm runtime bridge not yet implemented for ROS 2 node.\");\n\n");
    s.push_str("    loop {\n");
    s.push_str("        node.spin_once(std::time::Duration::from_millis(100));\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn generate_launch_file(package_name: &str) -> String {
    let mut s = String::new();
    s.push_str("from launch import LaunchDescription\n");
    s.push_str("from launch_ros.actions import Node\n\n");
    s.push_str("def generate_launch_description():\n");
    s.push_str("    return LaunchDescription([\n");
    s.push_str("        Node(\n");
    s.push_str(&format!("            package='{}',\n", package_name));
    s.push_str(&format!("            executable='{}',\n", package_name));
    s.push_str(&format!(
        "            name='nyaya_{}_node',\n",
        package_name
    ));
    s.push_str("            output='screen',\n");
    s.push_str("        ),\n");
    s.push_str("    ])\n");
    s
}

fn generate_params_yaml(entry: &CachedWork, package_name: &str) -> String {
    let node_name = format!("nyaya_{}_node", package_name);
    let mut s = String::new();
    s.push_str(&format!("{}:\n", node_name));
    s.push_str("  ros__parameters:\n");

    if entry.parameters.is_empty() {
        s.push_str("    # No parameters defined\n");
    } else {
        for param in &entry.parameters {
            let default = match &param.param_type {
                ParamType::Text => "\"\"".to_string(),
                ParamType::Number => "0.0".to_string(),
                ParamType::Boolean => "false".to_string(),
                ParamType::FilePath => "\"\"".to_string(),
                ParamType::Url => "\"\"".to_string(),
                ParamType::EmailAddress => "\"\"".to_string(),
                ParamType::DateTime => "\"\"".to_string(),
                ParamType::List(_) => "[]".to_string(),
            };
            s.push_str(&format!(
                "    {}: {}  # {}\n",
                param.name, default, param.description
            ));
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::semantic_cache::{
        CacheImplementation, CachedWork, ParamType, Parameter, ToolCall,
    };
    use tempfile::TempDir;

    fn make_test_entry() -> CachedWork {
        CachedWork {
            id: "test-ros2-001".to_string(),
            description: "Test ROS 2 entry".to_string(),
            original_task: "Test task".to_string(),
            rationale: "Test rationale".to_string(),
            improvement_notes: None,
            parameters: vec![Parameter {
                name: "threshold".to_string(),
                param_type: ParamType::Number,
                description: "Temperature threshold".to_string(),
                required: true,
            }],
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
    fn generates_all_expected_files() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("ros2-output");
        let entry = make_test_entry();

        let artifact = Ros2Generator::generate(&entry, &out).unwrap();

        assert!(artifact.package_xml.exists(), "package.xml should exist");
        assert!(artifact.cargo_toml.exists(), "Cargo.toml should exist");
        assert!(artifact.node_src.exists(), "src/main.rs should exist");
        assert!(artifact.launch_file.exists(), "launch file should exist");
        assert!(out.join("config").exists(), "config dir should exist");
        assert!(
            out.join("config").join("params.yaml").exists(),
            "params.yaml should exist"
        );
    }

    #[test]
    fn package_xml_contains_ament_cargo_and_std_msgs() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("ros2-output");
        let entry = make_test_entry();

        let artifact = Ros2Generator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.package_xml).unwrap();

        assert!(
            content.contains("ament_cargo"),
            "package.xml should contain ament_cargo"
        );
        assert!(
            content.contains("std_msgs"),
            "package.xml should contain std_msgs"
        );
    }

    #[test]
    fn cargo_toml_has_r2r_and_tokio() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("ros2-output");
        let entry = make_test_entry();

        let artifact = Ros2Generator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.cargo_toml).unwrap();

        assert!(
            content.contains("r2r"),
            "Cargo.toml should contain r2r dependency"
        );
        assert!(
            content.contains("tokio"),
            "Cargo.toml should contain tokio dependency"
        );
    }

    #[test]
    fn main_rs_contains_node_name_and_cached_entry() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("ros2-output");
        let entry = make_test_entry();

        let artifact = Ros2Generator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.node_src).unwrap();

        assert!(
            content.contains("nyaya_test_ros2_001_node"),
            "main.rs should contain the node name"
        );
        assert!(
            content.contains("CACHED_ENTRY"),
            "main.rs should contain CACHED_ENTRY constant"
        );
    }

    #[test]
    fn launch_file_contains_package_name_and_executable() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("ros2-output");
        let entry = make_test_entry();

        let artifact = Ros2Generator::generate(&entry, &out).unwrap();
        let content = fs::read_to_string(&artifact.launch_file).unwrap();

        assert!(
            content.contains("package='test_ros2_001'"),
            "launch file should contain correct package name"
        );
        assert!(
            content.contains("executable='test_ros2_001'"),
            "launch file should contain correct executable name"
        );
    }

    #[test]
    fn topic_name_for_tool_maps_correctly() {
        assert_eq!(
            topic_name_for_tool("hw.temperature_read"),
            "/nyaya/temperature"
        );
        assert_eq!(topic_name_for_tool("hw.led_write"), "/nyaya/led");
        assert_eq!(topic_name_for_tool("hw.i2c_bus"), "/nyaya/i2c");
        assert_eq!(topic_name_for_tool("plain_tool"), "/nyaya/plain_tool");
    }
}
