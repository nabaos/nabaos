//! Export analyzer — inspects cached work entries to determine dependency graphs
//! and platform compatibility for cross-platform export.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cache::intent_cache::IntentCacheEntry;
use crate::cache::semantic_cache::{CacheImplementation, CachedWork};
use crate::export::target::ExportTarget;
use crate::runtime::plugin::AbilitySource;

/// Per-platform recommendation from the LLM, indicating viability and reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformRecommendation {
    pub viable: bool,
    pub reason: String,
}

/// Dependency graph for a cached work entry, describing what external
/// resources it requires and its estimated binary size.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub abilities_used: Vec<String>,
    pub needs_network: bool,
    pub needs_filesystem: bool,
    pub needs_gpio: bool,
    pub is_pure_compute: bool,
    pub estimated_size_kb: u64,
    pub unresolved: Vec<String>,
}

/// Result of checking a dependency graph against a specific export target.
#[derive(Debug, Clone)]
pub struct PlatformCompatibility {
    pub target: ExportTarget,
    pub compatible: bool,
    pub reasons: Vec<String>,
}

/// Stateless analyzer that inspects cached work entries to produce dependency
/// graphs and platform compatibility reports.
pub struct ExportAnalyzer;

impl ExportAnalyzer {
    /// Analyze a `CachedWork` entry to determine its dependency graph.
    ///
    /// For `ToolSequence` implementations, each tool name is looked up via
    /// `ability_lookup` to determine what external resources it needs.
    /// For `Wasm` and `RustSource`, the entry is treated as pure compute
    /// unless the abilities say otherwise.
    pub fn analyze_cached_work(
        entry: &CachedWork,
        ability_lookup: &dyn Fn(&str) -> Option<AbilitySource>,
    ) -> DependencyGraph {
        match &entry.implementation {
            CacheImplementation::ToolSequence { steps } => Self::analyze_tool_calls_iter(
                steps.iter().map(|s| s.tool.as_str()),
                steps.len(),
                ability_lookup,
            ),
            CacheImplementation::Wasm { .. } | CacheImplementation::RustSource { .. } => {
                // Self-contained: no tool calls, pure compute by default.
                DependencyGraph {
                    abilities_used: Vec::new(),
                    needs_network: false,
                    needs_filesystem: false,
                    needs_gpio: false,
                    is_pure_compute: true,
                    estimated_size_kb: 50, // base size
                    unresolved: Vec::new(),
                }
            }
        }
    }

    /// Analyze an `IntentCacheEntry` to determine its dependency graph.
    ///
    /// Same logic as `analyze_cached_work` but operates on the
    /// `tool_sequence` field of an intent cache entry.
    pub fn analyze_intent_entry(
        entry: &IntentCacheEntry,
        ability_lookup: &dyn Fn(&str) -> Option<AbilitySource>,
    ) -> DependencyGraph {
        Self::analyze_tool_calls_iter(
            entry.tool_sequence.iter().map(|tc| tc.tool.as_str()),
            entry.tool_sequence.len(),
            ability_lookup,
        )
    }

    /// Check compatibility of a dependency graph against all known platforms.
    ///
    /// Returns one `PlatformCompatibility` entry per platform from
    /// `ExportTarget::all()`.
    pub fn check_compatibility(graph: &DependencyGraph) -> Vec<PlatformCompatibility> {
        ExportTarget::all()
            .iter()
            .map(|target| {
                let caps = target.capabilities();
                let mut reasons = Vec::new();
                let mut compatible = true;

                // Filesystem needed but platform lacks it
                if graph.needs_filesystem && !caps.has_filesystem {
                    compatible = false;
                    reasons.push(format!("{} does not support filesystem access", target));
                }

                // GPIO needed but platform lacks it
                if graph.needs_gpio && !caps.has_gpio {
                    compatible = false;
                    reasons.push(format!("{} does not support GPIO", target));
                }

                // Estimated size exceeds memory limit
                if graph.estimated_size_kb > caps.max_memory_kb {
                    compatible = false;
                    reasons.push(format!(
                        "estimated size {}KB exceeds {} memory limit of {}KB",
                        graph.estimated_size_kb, target, caps.max_memory_kb
                    ));
                }

                // Unresolved abilities: flag as warning but don't necessarily block
                if !graph.unresolved.is_empty() {
                    reasons.push(format!(
                        "unresolved abilities: {}",
                        graph.unresolved.join(", ")
                    ));
                }

                PlatformCompatibility {
                    target: *target,
                    compatible,
                    reasons,
                }
            })
            .collect()
    }

    /// Generate a prompt that asks an LLM to evaluate platform viability for a
    /// cached work entry, given its dependency graph.  The LLM is expected to
    /// return JSON inside an ```export_recommendation code block.
    pub fn generate_recommendation_prompt(entry: &CachedWork, graph: &DependencyGraph) -> String {
        format!(
            "Evaluate the following cached work entry for cross-platform export viability.\n\
             \n\
             Task description: {description}\n\
             Abilities used: {abilities}\n\
             Dependency flags:\n\
             - needs_network: {needs_network}\n\
             - needs_filesystem: {needs_filesystem}\n\
             - needs_gpio: {needs_gpio}\n\
             Estimated size: {estimated_size_kb} KB\n\
             Is pure compute: {is_pure_compute}\n\
             \n\
             For each target platform (cloud_run, raspberry_pi, esp32, ros2), determine \
             whether the entry is viable and explain why.\n\
             \n\
             Respond with JSON inside an ```export_recommendation code block:\n\
             ```export_recommendation\n\
             {{\n\
               \"cloud_run\": {{ \"viable\": true, \"reason\": \"...\" }},\n\
               \"raspberry_pi\": {{ \"viable\": true, \"reason\": \"...\" }},\n\
               \"esp32\": {{ \"viable\": false, \"reason\": \"...\" }},\n\
               \"ros2\": {{ \"viable\": true, \"reason\": \"...\" }}\n\
             }}\n\
             ```",
            description = entry.description,
            abilities = if graph.abilities_used.is_empty() {
                "(none)".to_string()
            } else {
                graph.abilities_used.join(", ")
            },
            needs_network = graph.needs_network,
            needs_filesystem = graph.needs_filesystem,
            needs_gpio = graph.needs_gpio,
            estimated_size_kb = graph.estimated_size_kb,
            is_pure_compute = graph.is_pure_compute,
        )
    }

    /// Parse an LLM response that contains platform recommendations inside an
    /// ```export_recommendation code block.  Returns `None` if the markers are
    /// missing or the JSON is malformed.
    pub fn parse_recommendation(response: &str) -> Option<HashMap<String, PlatformRecommendation>> {
        let start_marker = "```export_recommendation";
        let start = response.find(start_marker)?;
        let json_start = start + start_marker.len();
        let json_end = response[json_start..].find("```")?;
        let json_str = response[json_start..json_start + json_end].trim();
        serde_json::from_str(json_str).ok()
    }

    // ---- private helpers ----

    /// Shared analysis logic for an iterator of tool names.
    fn analyze_tool_calls_iter<'a>(
        tool_names: impl Iterator<Item = &'a str>,
        step_count: usize,
        ability_lookup: &dyn Fn(&str) -> Option<AbilitySource>,
    ) -> DependencyGraph {
        let mut abilities_used = Vec::new();
        let mut needs_network = false;
        let mut needs_filesystem = false;
        let mut needs_gpio = false;
        let mut unresolved = Vec::new();

        for tool_name in tool_names {
            abilities_used.push(tool_name.to_string());

            match ability_lookup(tool_name) {
                Some(AbilitySource::Cloud) => {
                    needs_network = true;
                }
                Some(AbilitySource::Subprocess) => {
                    needs_filesystem = true;
                }
                Some(AbilitySource::Hardware) => {
                    needs_gpio = true;
                }
                Some(AbilitySource::BuiltIn) | Some(AbilitySource::Plugin) => {
                    // No external dependency
                }
                None => {
                    unresolved.push(tool_name.to_string());
                }
            }
        }

        let is_pure_compute = !needs_network && !needs_filesystem && !needs_gpio;

        // Size estimation: base 50KB + 10KB per tool step + 200KB if needs network
        let estimated_size_kb = 50 + (10 * step_count as u64) + if needs_network { 200 } else { 0 };

        DependencyGraph {
            abilities_used,
            needs_network,
            needs_filesystem,
            needs_gpio,
            is_pure_compute,
            estimated_size_kb,
            unresolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::intent_cache::CachedToolCall;
    use crate::cache::semantic_cache::{CacheImplementation, CachedWork, ToolCall};

    /// Helper to create a minimal CachedWork with the given implementation.
    fn make_cached_work(implementation: CacheImplementation) -> CachedWork {
        CachedWork {
            id: "test-id".into(),
            description: "test entry".into(),
            original_task: "test task".into(),
            rationale: "test rationale".into(),
            improvement_notes: None,
            parameters: vec![],
            implementation,
            hit_count: 0,
            success_count: 0,
            failure_count: 0,
            created_at: 0,
            last_used_at: 0,
            similarity_threshold: 0.92,
            enabled: true,
        }
    }

    /// Helper to create a minimal IntentCacheEntry.
    fn make_intent_entry(tool_sequence: Vec<CachedToolCall>) -> IntentCacheEntry {
        IntentCacheEntry {
            intent_key: "test_intent".into(),
            description: "test intent".into(),
            tool_sequence,
            hit_count: 0,
            success_count: 0,
            failure_count: 0,
            enabled: true,
            created_at: 0,
            last_used_at: 0,
            response_text: None,
        }
    }

    // Test 1: Pure compute entry (empty ToolSequence) is compatible everywhere
    #[test]
    fn test_pure_compute_compatible_everywhere() {
        let entry = make_cached_work(CacheImplementation::ToolSequence { steps: vec![] });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|_| Some(AbilitySource::BuiltIn));

        assert!(graph.is_pure_compute);
        assert!(graph.abilities_used.is_empty());
        assert!(!graph.needs_network);
        assert!(!graph.needs_filesystem);
        assert!(!graph.needs_gpio);

        let compat = ExportAnalyzer::check_compatibility(&graph);
        assert_eq!(compat.len(), 4);
        for pc in &compat {
            assert!(pc.compatible, "expected compatible for {}", pc.target);
        }
    }

    // Test 2: Cloud ability sets needs_network
    #[test]
    fn test_cloud_ability_sets_needs_network() {
        let entry = make_cached_work(CacheImplementation::ToolSequence {
            steps: vec![ToolCall {
                tool: "weather_api".into(),
                args: serde_json::Map::new(),
            }],
        });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|name| match name {
            "weather_api" => Some(AbilitySource::Cloud),
            _ => None,
        });

        assert!(graph.needs_network);
        assert!(!graph.is_pure_compute);
        assert_eq!(graph.abilities_used, vec!["weather_api"]);
    }

    // Test 3: Hardware ability sets needs_gpio, CloudRun incompatible
    #[test]
    fn test_hardware_ability_cloudrun_incompatible() {
        let entry = make_cached_work(CacheImplementation::ToolSequence {
            steps: vec![ToolCall {
                tool: "read_sensor".into(),
                args: serde_json::Map::new(),
            }],
        });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|name| match name {
            "read_sensor" => Some(AbilitySource::Hardware),
            _ => None,
        });

        assert!(graph.needs_gpio);
        assert!(!graph.is_pure_compute);

        let compat = ExportAnalyzer::check_compatibility(&graph);
        let cloud_run = compat
            .iter()
            .find(|c| c.target == ExportTarget::CloudRun)
            .unwrap();
        assert!(!cloud_run.compatible);
        assert!(cloud_run.reasons.iter().any(|r| r.contains("GPIO")));

        // RaspberryPi and ESP32 both have GPIO
        let rpi = compat
            .iter()
            .find(|c| c.target == ExportTarget::RaspberryPi)
            .unwrap();
        assert!(rpi.compatible);
        let esp = compat
            .iter()
            .find(|c| c.target == ExportTarget::Esp32)
            .unwrap();
        assert!(esp.compatible);
    }

    // Test 4: Wasm/RustSource impl is self-contained (abilities_used empty)
    #[test]
    fn test_wasm_rust_source_self_contained() {
        let wasm_entry = make_cached_work(CacheImplementation::Wasm {
            module_path: "/tmp/module.wasm".into(),
        });
        let graph = ExportAnalyzer::analyze_cached_work(&wasm_entry, &|_| None);

        assert!(graph.is_pure_compute);
        assert!(graph.abilities_used.is_empty());
        assert_eq!(graph.estimated_size_kb, 50);

        let rust_entry = make_cached_work(CacheImplementation::RustSource {
            code: "fn main() {}".into(),
        });
        let graph2 = ExportAnalyzer::analyze_cached_work(&rust_entry, &|_| None);

        assert!(graph2.is_pure_compute);
        assert!(graph2.abilities_used.is_empty());
    }

    // Test 5: Unresolved abilities flagged in unresolved vec
    #[test]
    fn test_unresolved_abilities_flagged() {
        let entry = make_cached_work(CacheImplementation::ToolSequence {
            steps: vec![ToolCall {
                tool: "unknown_tool".into(),
                args: serde_json::Map::new(),
            }],
        });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|_| None);

        assert_eq!(graph.unresolved, vec!["unknown_tool"]);

        let compat = ExportAnalyzer::check_compatibility(&graph);
        // Still compatible (unresolved is a warning, not a blocker)
        for pc in &compat {
            assert!(
                pc.compatible,
                "should still be compatible for {}",
                pc.target
            );
            assert!(pc.reasons.iter().any(|r| r.contains("unresolved")));
        }
    }

    // Test 6: Large size exceeding ESP32 320KB limit makes ESP32 incompatible
    #[test]
    fn test_large_size_esp32_incompatible() {
        // We need enough tool steps to exceed 320KB.
        // Size = 50 + 10*N + 200 (if network). With network: 250 + 10*N.
        // For 320KB limit: 250 + 10*N > 320 → N > 7 → 8 steps.
        let steps: Vec<ToolCall> = (0..8)
            .map(|i| ToolCall {
                tool: format!("cloud_tool_{}", i),
                args: serde_json::Map::new(),
            })
            .collect();

        let entry = make_cached_work(CacheImplementation::ToolSequence { steps });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|_| Some(AbilitySource::Cloud));

        // 50 + 10*8 + 200 = 330 KB
        assert_eq!(graph.estimated_size_kb, 330);

        let compat = ExportAnalyzer::check_compatibility(&graph);
        let esp = compat
            .iter()
            .find(|c| c.target == ExportTarget::Esp32)
            .unwrap();
        assert!(!esp.compatible);
        assert!(esp.reasons.iter().any(|r| r.contains("memory limit")));

        // CloudRun and RPi should still be fine
        let cloud = compat
            .iter()
            .find(|c| c.target == ExportTarget::CloudRun)
            .unwrap();
        assert!(cloud.compatible);
    }

    // ---- LLM recommendation tests ----

    // Test: Prompt contains all graph fields
    #[test]
    fn test_recommendation_prompt_contains_graph_fields() {
        let entry = make_cached_work(CacheImplementation::ToolSequence {
            steps: vec![ToolCall {
                tool: "weather_api".into(),
                args: serde_json::Map::new(),
            }],
        });
        let graph = DependencyGraph {
            abilities_used: vec!["weather_api".into()],
            needs_network: true,
            needs_filesystem: false,
            needs_gpio: false,
            is_pure_compute: false,
            estimated_size_kb: 260,
            unresolved: vec![],
        };

        let prompt = ExportAnalyzer::generate_recommendation_prompt(&entry, &graph);

        assert!(
            prompt.contains("weather_api"),
            "should contain abilities_used"
        );
        assert!(
            prompt.contains("needs_network: true"),
            "should contain needs_network"
        );
        assert!(prompt.contains("needs_filesystem: false"));
        assert!(prompt.contains("needs_gpio: false"));
        assert!(
            prompt.contains("260 KB"),
            "should contain estimated_size_kb"
        );
        assert!(prompt.contains("Is pure compute: false"));
    }

    // Test: Prompt includes entry description
    #[test]
    fn test_recommendation_prompt_includes_description() {
        let entry = make_cached_work(CacheImplementation::Wasm {
            module_path: "/tmp/module.wasm".into(),
        });
        let graph = DependencyGraph {
            abilities_used: vec![],
            needs_network: false,
            needs_filesystem: false,
            needs_gpio: false,
            is_pure_compute: true,
            estimated_size_kb: 50,
            unresolved: vec![],
        };

        let prompt = ExportAnalyzer::generate_recommendation_prompt(&entry, &graph);
        assert!(
            prompt.contains("test entry"),
            "should contain entry description"
        );
    }

    // Test: Parse valid JSON between markers
    #[test]
    fn test_parse_recommendation_valid() {
        let response = r#"Here is my analysis:
```export_recommendation
{
  "cloud_run": { "viable": true, "reason": "Full network access available" },
  "raspberry_pi": { "viable": true, "reason": "Sufficient memory and GPIO" },
  "esp32": { "viable": false, "reason": "Exceeds memory limit" }
}
```
That's my recommendation."#;

        let result = ExportAnalyzer::parse_recommendation(response);
        assert!(result.is_some(), "should parse valid recommendation");
        let map = result.unwrap();
        assert_eq!(map.len(), 3);
        assert!(map["cloud_run"].viable);
        assert!(!map["esp32"].viable);
        assert!(map["esp32"].reason.contains("memory"));
    }

    // Test: Parse returns None for malformed/missing markers
    #[test]
    fn test_parse_recommendation_missing_markers() {
        // No markers at all
        assert!(ExportAnalyzer::parse_recommendation("just some text").is_none());

        // Wrong marker name
        let wrong = r#"```cache
{"cloud_run": {"viable": true, "reason": "ok"}}
```"#;
        assert!(ExportAnalyzer::parse_recommendation(wrong).is_none());

        // Correct marker but malformed JSON
        let malformed = r#"```export_recommendation
{not valid json}
```"#;
        assert!(ExportAnalyzer::parse_recommendation(malformed).is_none());
    }

    // Test 7: Entry with Subprocess ability sets needs_filesystem
    #[test]
    fn test_subprocess_ability_needs_filesystem() {
        let entry = make_intent_entry(vec![CachedToolCall {
            tool: "ffmpeg_transcode".into(),
            args: serde_json::Map::new(),
        }]);
        let graph = ExportAnalyzer::analyze_intent_entry(&entry, &|name| match name {
            "ffmpeg_transcode" => Some(AbilitySource::Subprocess),
            _ => None,
        });

        assert!(graph.needs_filesystem);
        assert!(!graph.is_pure_compute);
        assert_eq!(graph.abilities_used, vec!["ffmpeg_transcode"]);

        // ESP32 has no filesystem
        let compat = ExportAnalyzer::check_compatibility(&graph);
        let esp = compat
            .iter()
            .find(|c| c.target == ExportTarget::Esp32)
            .unwrap();
        assert!(!esp.compatible);
        assert!(esp.reasons.iter().any(|r| r.contains("filesystem")));
    }

    // Test: Ros2 is compatible with hardware-using entries (has_gpio=true)
    #[test]
    fn test_ros2_compatible_with_hardware_entry() {
        let entry = make_cached_work(CacheImplementation::ToolSequence {
            steps: vec![ToolCall {
                tool: "read_sensor".into(),
                args: serde_json::Map::new(),
            }],
        });
        let graph = ExportAnalyzer::analyze_cached_work(&entry, &|name| match name {
            "read_sensor" => Some(AbilitySource::Hardware),
            _ => None,
        });

        assert!(graph.needs_gpio);

        let compat = ExportAnalyzer::check_compatibility(&graph);
        let ros2 = compat
            .iter()
            .find(|c| c.target == ExportTarget::Ros2)
            .expect("Ros2 should appear in compatibility results");
        assert!(
            ros2.compatible,
            "Ros2 should be compatible with GPIO-using entries"
        );
    }
}
