# LLM-Guided Statistical Analysis Phase

> Date: 2026-03-11
> Status: Approved

## Problem

PEA documents contain quantitative claims extracted from research sources but perform no actual statistical analysis. The evidence gate checks for the presence of statistics but doesn't validate or compute them. Academic and data-driven objectives would benefit from rigorous statistical analysis that sections can reference.

## Design

### Pipeline Position

New phase between research completion and composition planning:

```
Research → [Statistical Analysis] → Plan Structure → Generate Sections → Quality Gates → Output
```

### Activation

`ComposerConfig.statistical_analysis: Option<bool>` — None = auto-detect, Some(true) = force on, Some(false) = force off. Env: `NABA_PEA_STATS=true|false|auto`.

Auto-detection: keyword scan for "statistical", "quantitative", "data-driven", "empirical", "regression", "correlation", "meta-analysis", "effect size", "significance", "survey".

### Three-Step Process

1. **Data Extraction** — LLM scans corpus, extracts structured quantitative data (variable, value, unit, context, confidence, sample size). LLM auto-selects analysis mode.
2. **Statistical Computation** — LLM generates Python script executed via `script.run`. Mode-specific: basic aggregates, comparative analysis, or meta-analysis.
3. **Downstream Injection** — Results passed to section generators, evidence gate, chart generation, and structure planning.

### Data Structures

- `StatisticalDataset` — extracted data entries + chosen analysis mode
- `StatisticalResults` — computed summaries, rankings, key findings, methodology note
- `AnalysisMode` — ExtractAndValidate | ComparativeAnalysis | MetaAnalysis

### Files

- New: `src/pea/statistical.rs`
- Modify: `src/pea/composer.rs`, `src/pea/mod.rs`
