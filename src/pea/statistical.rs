// PEA Statistical Analysis Phase — LLM-guided rigorous statistical analysis.
//
// Pipeline position: after research, before composition planning.
//
// Three-step process:
//   1. Data Extraction — LLM scans corpus, extracts structured quantitative data
//   2. Statistical Computation — LLM generates Python script executed via script.run
//   3. Downstream Injection — results passed to section generators + evidence gate

use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};
use crate::pea::research::ResearchCorpus;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisMode {
    ExtractAndValidate,
    ComparativeAnalysis,
    MetaAnalysis,
}

impl std::fmt::Display for AnalysisMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExtractAndValidate => write!(f, "extract_and_validate"),
            Self::ComparativeAnalysis => write!(f, "comparative_analysis"),
            Self::MetaAnalysis => write!(f, "meta_analysis"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataEntry {
    pub variable: String,
    pub value: f64,
    pub unit: String,
    pub context: String,
    pub confidence: f32,
    pub sample_size: Option<u64>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticalDataset {
    pub entries: Vec<DataEntry>,
    pub mode: AnalysisMode,
    pub methodology_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableSummary {
    pub variable: String,
    pub count: usize,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ranking {
    pub label: String,
    pub score: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticalResults {
    pub mode: AnalysisMode,
    pub summaries: Vec<VariableSummary>,
    pub rankings: Vec<Ranking>,
    pub key_findings: Vec<String>,
    pub methodology_note: String,
    pub raw_output: String,
}

impl StatisticalResults {
    /// Format results as context string for injection into section generation prompts.
    pub fn as_context(&self) -> String {
        let mut ctx = format!(
            "STATISTICAL ANALYSIS RESULTS (mode: {})\n\n",
            self.mode
        );

        if !self.summaries.is_empty() {
            ctx.push_str("Variable Summaries:\n");
            for s in &self.summaries {
                ctx.push_str(&format!(
                    "  - {}: mean={:.3} ±{:.3} {} (n={}, range {:.3}–{:.3})\n",
                    s.variable, s.mean, s.std_dev, s.unit, s.count, s.min, s.max
                ));
            }
            ctx.push('\n');
        }

        if !self.rankings.is_empty() {
            ctx.push_str("Rankings:\n");
            for (i, r) in self.rankings.iter().enumerate() {
                ctx.push_str(&format!(
                    "  {}. {} (score: {:.3}) — {}\n",
                    i + 1,
                    r.label,
                    r.score,
                    r.rationale
                ));
            }
            ctx.push('\n');
        }

        if !self.key_findings.is_empty() {
            ctx.push_str("Key Findings:\n");
            for f in &self.key_findings {
                ctx.push_str(&format!("  • {}\n", f));
            }
            ctx.push('\n');
        }

        if !self.methodology_note.is_empty() {
            ctx.push_str(&format!("Methodology: {}\n", self.methodology_note));
        }

        ctx
    }
}

// ---------------------------------------------------------------------------
// Auto-detection
// ---------------------------------------------------------------------------

/// Heuristic: does the objective warrant statistical analysis?
/// Checks for quantitative/analytical keywords without requiring an LLM call.
pub fn is_statistical_objective(objective: &str) -> bool {
    let lower = objective.to_ascii_lowercase();
    let keywords = [
        "statistical",
        "quantitative",
        "data-driven",
        "empirical",
        "regression",
        "correlation",
        "meta-analysis",
        "effect size",
        "significance",
        "survey",
        "benchmark",
        "performance comparison",
        "efficiency",
        "accuracy",
        "metrics",
        "measurements",
        "experimental results",
        "numerical",
    ];
    keywords.iter().filter(|k| lower.contains(*k)).count() >= 2
}

// ---------------------------------------------------------------------------
// Analyzer
// ---------------------------------------------------------------------------

pub struct StatisticalAnalyzer<'a> {
    registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
}

impl<'a> StatisticalAnalyzer<'a> {
    pub fn new(registry: &'a AbilityRegistry, manifest: &'a AgentManifest) -> Self {
        Self { registry, manifest }
    }

    /// Run the full statistical analysis pipeline.
    /// Returns None if no quantitative data was found in the corpus.
    pub fn analyze(
        &self,
        objective: &str,
        corpus: &ResearchCorpus,
    ) -> Result<Option<StatisticalResults>> {
        // Step 1: Extract quantitative data from corpus
        eprintln!("[statistical] step 1: extracting quantitative data from {} sources...", corpus.sources.len());
        let dataset = self.extract_data(objective, corpus)?;

        if dataset.entries.is_empty() {
            eprintln!("[statistical] no quantitative data found, skipping analysis");
            return Ok(None);
        }

        eprintln!(
            "[statistical] extracted {} data entries, mode: {}",
            dataset.entries.len(),
            dataset.mode
        );

        // Step 2: Generate and execute statistical computation
        eprintln!("[statistical] step 2: computing statistics...");
        let results = self.compute_statistics(objective, &dataset)?;

        eprintln!(
            "[statistical] done: {} summaries, {} rankings, {} findings",
            results.summaries.len(),
            results.rankings.len(),
            results.key_findings.len()
        );

        Ok(Some(results))
    }

    // -- Step 1: Data Extraction -----------------------------------------------

    fn extract_data(
        &self,
        objective: &str,
        corpus: &ResearchCorpus,
    ) -> Result<StatisticalDataset> {
        // Build corpus excerpt for the LLM (top sources, truncated)
        let corpus_text: String = corpus
            .sources
            .iter()
            .take(15)
            .map(|s| {
                let body = crate::pea::research::safe_slice(&s.content, 2000);
                format!("SOURCE: {} ({})\n{}\n---", s.title, s.url, body)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "You are a research data analyst. Your task is to extract ALL quantitative data \
             from the following research corpus related to: \"{}\"\n\n\
             CORPUS:\n{}\n\n\
             Instructions:\n\
             1. Extract every numerical claim, measurement, benchmark result, statistic, \
                percentage, or quantitative finding.\n\
             2. For each, record: variable name, numeric value, unit, source context, \
                confidence (0.0-1.0), sample size (if stated), and source URL/title.\n\
             3. Decide the best analysis mode:\n\
                - \"ExtractAndValidate\": when data is heterogeneous, mainly validating claims\n\
                - \"ComparativeAnalysis\": when comparing methods/systems/approaches with metrics\n\
                - \"MetaAnalysis\": when synthesizing results across multiple studies\n\
             4. Write a brief methodology note explaining your extraction approach.\n\n\
             Respond in JSON ONLY:\n\
             {{\n\
               \"entries\": [\n\
                 {{\"variable\": \"...\", \"value\": 0.0, \"unit\": \"...\", \"context\": \"...\", \
                   \"confidence\": 0.8, \"sample_size\": null, \"source\": \"...\"}}\n\
               ],\n\
               \"mode\": \"ComparativeAnalysis\",\n\
               \"methodology_note\": \"...\"\n\
             }}",
            objective, corpus_text,
        );

        let input = serde_json::json!({
            "system": "You are a quantitative research analyst. Extract ALL numerical data from research sources. Output ONLY valid JSON.",
            "prompt": prompt,
            "max_tokens": 8192,
            "thinking": false,
        });

        let result = self
            .registry
            .execute_ability(self.manifest, "llm.chat", &input.to_string())
            .map_err(|e| NyayaError::Config(format!("statistical data extraction failed: {}", e)))?;

        let raw = String::from_utf8_lossy(&result.output).to_string();
        let json_str = crate::pea::composer::extract_json_pub(&raw);

        match serde_json::from_str::<StatisticalDataset>(json_str) {
            Ok(dataset) => Ok(dataset),
            Err(e) => {
                eprintln!(
                    "[statistical] JSON parse failed: {}, raw: {}",
                    e,
                    &raw[..raw.len().min(500)]
                );
                // Return empty dataset on parse failure
                Ok(StatisticalDataset {
                    entries: Vec::new(),
                    mode: AnalysisMode::ExtractAndValidate,
                    methodology_note: format!("extraction parse failed: {}", e),
                })
            }
        }
    }

    // -- Step 2: Statistical Computation ---------------------------------------

    fn compute_statistics(
        &self,
        objective: &str,
        dataset: &StatisticalDataset,
    ) -> Result<StatisticalResults> {
        let dataset_json =
            serde_json::to_string_pretty(dataset).unwrap_or_else(|_| "{}".to_string());

        let mode_instructions = match dataset.mode {
            AnalysisMode::ExtractAndValidate => {
                "Compute: per-variable summary stats (count, mean, std dev, min, max), \
                 flag outliers (>2 std devs), cross-validate claims that appear in multiple sources, \
                 identify contradictions."
            }
            AnalysisMode::ComparativeAnalysis => {
                "Compute: per-variable summary stats, rank entities/methods by each metric, \
                 compute effect sizes between top and bottom performers, \
                 identify statistically significant differences where sample sizes allow, \
                 produce a composite ranking."
            }
            AnalysisMode::MetaAnalysis => {
                "Compute: weighted mean effect sizes (inverse variance weighting), \
                 heterogeneity (I² statistic), forest plot data, \
                 sensitivity analysis excluding each study, \
                 publication bias assessment (funnel plot asymmetry if n>=10)."
            }
        };

        let prompt = format!(
            "You are a computational statistician. Generate a Python script that performs \
             rigorous statistical analysis on the following dataset.\n\n\
             OBJECTIVE: {}\n\
             ANALYSIS MODE: {}\n\n\
             DATASET (JSON):\n{}\n\n\
             INSTRUCTIONS:\n{}\n\n\
             The script MUST:\n\
             1. Parse the dataset from the embedded JSON string\n\
             2. Use only numpy and standard library (no pandas, scipy, etc.)\n\
             3. Print results as a SINGLE JSON object to stdout with this structure:\n\
             {{\n\
               \"summaries\": [{{\"variable\": \"...\", \"count\": 0, \"mean\": 0.0, \"std_dev\": 0.0, \"min\": 0.0, \"max\": 0.0, \"unit\": \"...\"}}],\n\
               \"rankings\": [{{\"label\": \"...\", \"score\": 0.0, \"rationale\": \"...\"}}],\n\
               \"key_findings\": [\"...\"],\n\
               \"methodology_note\": \"...\"\n\
             }}\n\
             4. Handle edge cases: single data point → std_dev=0, empty groups → skip\n\
             5. Do NOT import matplotlib or any plotting libraries\n\
             6. Output ONLY the Python code, no explanation",
            objective, dataset.mode, dataset_json, mode_instructions,
        );

        let input = serde_json::json!({
            "system": "You are a computational statistician. Output ONLY a Python script. No explanation, no markdown fences.",
            "prompt": prompt,
            "max_tokens": 4096,
            "thinking": false,
        });

        let result = self
            .registry
            .execute_ability(self.manifest, "llm.chat", &input.to_string())
            .map_err(|e| NyayaError::Config(format!("statistical script generation failed: {}", e)))?;

        let raw = String::from_utf8_lossy(&result.output).to_string();

        // Extract Python code (may be wrapped in markdown fences)
        let code = extract_python_code(&raw);

        // Execute via script.run ability
        let script_input = serde_json::json!({
            "language": "python",
            "code": code,
        });

        let script_result = self
            .registry
            .execute_ability(self.manifest, "script.run", &script_input.to_string())
            .map_err(|e| {
                eprintln!("[statistical] script execution failed: {}", e);
                NyayaError::Config(format!("statistical computation failed: {}", e))
            })?;

        let output = String::from_utf8_lossy(&script_result.output).to_string();
        eprintln!("[statistical] script output: {} bytes", output.len());

        // Parse results from script output
        let json_str = crate::pea::composer::extract_json_pub(&output);
        match serde_json::from_str::<ComputedResults>(json_str) {
            Ok(computed) => Ok(StatisticalResults {
                mode: dataset.mode.clone(),
                summaries: computed.summaries,
                rankings: computed.rankings,
                key_findings: computed.key_findings,
                methodology_note: computed
                    .methodology_note
                    .unwrap_or_else(|| dataset.methodology_note.clone()),
                raw_output: output,
            }),
            Err(e) => {
                eprintln!(
                    "[statistical] results parse failed: {}, output: {}",
                    e,
                    &output[..output.len().min(500)]
                );
                // Return minimal results with raw output for downstream use
                Ok(StatisticalResults {
                    mode: dataset.mode.clone(),
                    summaries: Vec::new(),
                    rankings: Vec::new(),
                    key_findings: vec![format!(
                        "Statistical computation produced output but parsing failed: {}",
                        e
                    )],
                    methodology_note: dataset.methodology_note.clone(),
                    raw_output: output,
                })
            }
        }
    }
}

/// Intermediate struct for parsing Python script output.
#[derive(Deserialize)]
struct ComputedResults {
    #[serde(default)]
    summaries: Vec<VariableSummary>,
    #[serde(default)]
    rankings: Vec<Ranking>,
    #[serde(default)]
    key_findings: Vec<String>,
    methodology_note: Option<String>,
}

/// Extract Python code from LLM response — handles markdown fences.
fn extract_python_code(raw: &str) -> &str {
    // Try to find ```python ... ``` block
    if let Some(start) = raw.find("```python") {
        let code_start = start + "```python".len();
        if let Some(end) = raw[code_start..].find("```") {
            return raw[code_start..code_start + end].trim();
        }
    }
    // Try generic ``` block
    if let Some(start) = raw.find("```") {
        let code_start = start + 3;
        // Skip language tag if on same line
        let line_end = raw[code_start..].find('\n').unwrap_or(0);
        let code_start = code_start + line_end;
        if let Some(end) = raw[code_start..].find("```") {
            return raw[code_start..code_start + end].trim();
        }
    }
    // Assume entire response is code
    raw.trim()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_statistical_objective_positive() {
        assert!(is_statistical_objective(
            "survey of transformer efficiency benchmarks and performance metrics"
        ));
        assert!(is_statistical_objective(
            "meta-analysis of empirical results in NLP"
        ));
        assert!(is_statistical_objective(
            "quantitative comparison of regression models"
        ));
    }

    #[test]
    fn test_is_statistical_objective_negative() {
        assert!(!is_statistical_objective("write a recipe for chocolate cake"));
        assert!(!is_statistical_objective("explain quantum computing basics"));
        // Single keyword not enough
        assert!(!is_statistical_objective("run a survey"));
    }

    #[test]
    fn test_analysis_mode_display() {
        assert_eq!(AnalysisMode::ExtractAndValidate.to_string(), "extract_and_validate");
        assert_eq!(AnalysisMode::ComparativeAnalysis.to_string(), "comparative_analysis");
        assert_eq!(AnalysisMode::MetaAnalysis.to_string(), "meta_analysis");
    }

    #[test]
    fn test_statistical_results_as_context() {
        let results = StatisticalResults {
            mode: AnalysisMode::ComparativeAnalysis,
            summaries: vec![VariableSummary {
                variable: "accuracy".to_string(),
                count: 5,
                mean: 0.95,
                std_dev: 0.02,
                min: 0.92,
                max: 0.98,
                unit: "%".to_string(),
            }],
            rankings: vec![Ranking {
                label: "ModelA".to_string(),
                score: 0.98,
                rationale: "highest accuracy".to_string(),
            }],
            key_findings: vec!["All models exceed 90% accuracy".to_string()],
            methodology_note: "Comparative ranking by mean performance".to_string(),
            raw_output: String::new(),
        };

        let ctx = results.as_context();
        assert!(ctx.contains("accuracy"));
        assert!(ctx.contains("ModelA"));
        assert!(ctx.contains("All models exceed 90%"));
        assert!(ctx.contains("Comparative ranking"));
    }

    #[test]
    fn test_extract_python_code_markdown() {
        let raw = "Here is the code:\n```python\nimport json\nprint('hello')\n```\nDone.";
        assert_eq!(extract_python_code(raw), "import json\nprint('hello')");
    }

    #[test]
    fn test_extract_python_code_plain() {
        let raw = "import json\nprint('hello')";
        assert_eq!(extract_python_code(raw), "import json\nprint('hello')");
    }

    #[test]
    fn test_data_entry_serialization() {
        let entry = DataEntry {
            variable: "latency".to_string(),
            value: 42.5,
            unit: "ms".to_string(),
            context: "inference time".to_string(),
            confidence: 0.9,
            sample_size: Some(100),
            source: "paper1".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: DataEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.variable, "latency");
        assert_eq!(parsed.value, 42.5);
    }
}
