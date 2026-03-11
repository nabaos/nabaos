// PEA Document Generator — skeleton template + per-section LaTeX assembly.
//
// Uses a safe LaTeX skeleton template with commonly-available packages,
// generates content per-section via LLM (body only, no preamble), and
// sanitizes each section before assembly. Falls back to HTML on failure.

use std::path::{Path, PathBuf};

use crate::core::error::{NyayaError, Result};
use crate::modules::latex::LatexBackend;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Style configuration — LLM-driven document styling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImageQuery {
    pub query: String,
    pub placement: String,
    #[serde(default)]
    pub chapter: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StyleConfig {
    pub theme: String,
    pub primary_color: String,
    pub accent_color: String,
    pub font_family: String,
    pub ornament_style: String,
    pub watermark_text: Option<String>,
    #[serde(default)]
    pub watermark_opacity: f64,
    pub chapter_style: String,
    #[serde(default)]
    pub use_drop_caps: bool,
    #[serde(default)]
    pub image_queries: Vec<ImageQuery>,
    #[serde(default = "default_true")]
    pub skip_stock_images: bool,
}

fn default_true() -> bool { true }

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            theme: "clean".into(),
            primary_color: "#333333".into(),
            accent_color: "#0066CC".into(),
            font_family: "serif".into(),
            ornament_style: "none".into(),
            watermark_text: None,
            watermark_opacity: 0.0,
            chapter_style: "clean".into(),
            use_drop_caps: false,
            image_queries: vec![],
            skip_stock_images: true,
        }
    }
}

impl StyleConfig {
    /// Returns true when stock images should be skipped (analytical/report themes).
    /// Override with `NABA_PEA_SKIP_STOCKS=0` env var.
    pub fn should_skip_stock_images(&self) -> bool {
        if let Ok(v) = std::env::var("NABA_PEA_SKIP_STOCKS") {
            if v == "0" || v.eq_ignore_ascii_case("false") {
                return false;
            }
        }
        if !self.skip_stock_images {
            return false;
        }
        matches!(
            self.theme.to_ascii_lowercase().as_str(),
            "analytical" | "academic" | "corporate" | "technical" | "minimal" | "editorial" | "clean"
        )
    }
}

const STYLE_ANALYSIS_PROMPT: &str = "\
You are a professional document designer. Analyze the document objective and content, \
then output a JSON StyleConfig that determines the visual styling. Your choices should \
be content-appropriate: cookbooks get warm/ornamental themes, research papers get clean/academic \
themes, creative writing gets artistic themes, business reports get corporate themes. \
Output ONLY valid JSON matching this schema exactly — no explanation, no markdown fences.";

fn build_style_analysis_prompt(
    objective_desc: &str,
    task_results: &[(String, String)],
) -> String {
    let content_preview: String = task_results
        .iter()
        .take(3)
        .map(|(desc, text)| {
            let preview = if text.len() > 500 { &text[..500] } else { text.as_str() };
            format!("- {}: {}", desc, preview)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Document objective: {}\n\n\
         Content preview:\n{}\n\n\
         Output a JSON object with these exact fields:\n\
         {{\n\
           \"theme\": \"oriental|academic|corporate|creative|editorial|technical|minimal\",\n\
           \"primary_color\": \"#hex\",\n\
           \"accent_color\": \"#hex\",\n\
           \"font_family\": \"serif|sans-serif|monospace\",\n\
           \"ornament_style\": \"floral|geometric|minimal|none\",\n\
           \"watermark_text\": \"short text or null\",\n\
           \"watermark_opacity\": 0.0-0.15,\n\
           \"chapter_style\": \"ornate|clean|academic|editorial\",\n\
           \"use_drop_caps\": true/false,\n\
           \"image_queries\": [\n\
             {{\"query\": \"search terms for stock photo\", \"placement\": \"chapter_header|section_illustration|title_page\", \"chapter\": \"chapter name or null\"}}\n\
           ],\n\
           \"skip_stock_images\": true\n\
         }}\n\n\
         Generate 3-6 image_queries that would enhance this document. RULES for queries:\n\
         - Be SPECIFIC to the actual content (e.g., \"UN Security Council emergency session 2026\" \
         not generic \"diplomacy meeting\")\n\
         - Include the year/event when relevant for news/current affairs topics\n\
         - For sections with data/statistics, do NOT add image queries — charts will be auto-generated\n\
         - For technical docs or code references, use an empty array\n\
         - Prefer editorial/journalistic photography queries over generic stock photos\n\
         - Each query should relate to a specific chapter, not the whole document",
        objective_desc, content_preview
    )
}

/// Analyze document content and generate a StyleConfig via LLM.
///
/// Falls back to `StyleConfig::default()` if LLM call fails or returns invalid JSON.
pub fn analyze_style(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)],
) -> StyleConfig {
    let prompt = build_style_analysis_prompt(objective_desc, task_results);

    let input = serde_json::json!({
        "system": STYLE_ANALYSIS_PROMPT,
        "prompt": prompt,
    });

    let result = match registry.execute_ability(manifest, "llm.chat", &input.to_string()) {
        Ok(r) => r,
        Err(_) => return StyleConfig::default(),
    };

    let raw = String::from_utf8_lossy(&result.output).to_string();

    // Try to extract JSON from the response (may have markdown fences)
    let json_str = if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            &raw[start..=end]
        } else {
            &raw
        }
    } else {
        &raw
    };

    serde_json::from_str(json_str).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A sourced image with caption, file path, and optional attribution.
pub type ImageEntry = (String, PathBuf, Option<String>); // (caption, path, attribution)

/// Assemble all task results into a final document (PDF or HTML).
///
/// Returns the path to the generated output file.
pub fn assemble_document(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)], // (task_description, result_text)
    images: &[ImageEntry],             // (caption, image_path, attribution)
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to create output dir: {}", e)))?;

    // 1. Generate LaTeX source via LLM
    let tex_source = generate_latex_source(registry, manifest, objective_desc, task_results, images, style)?;

    // 2. Post-process: fix image paths for the output directory
    let tex_source = postprocess_latex(&tex_source, images, output_dir);

    // 3. Try to compile to PDF with self-healing retry loop
    let tex_path = output_dir.join("output.tex");
    let log_path = output_dir.join("output.log");
    let backend = LatexBackend::detect();

    let mut current_tex = tex_source;
    let max_retries = 3;

    for attempt in 0..max_retries {
        std::fs::write(&tex_path, &current_tex)
            .map_err(|e| NyayaError::Config(format!("Failed to write .tex file: {}", e)))?;

        match backend.compile(&tex_path, output_dir) {
            Ok(pdf_path) => return Ok(pdf_path),
            Err(compile_err) => {
                if attempt + 1 >= max_retries {
                    // All retries exhausted — fall back to HTML
                    break;
                }

                // Read the error log and ask LLM to fix
                let error_log = std::fs::read_to_string(&log_path).unwrap_or_else(|_| {
                    format!("Compilation error: {}", compile_err)
                });
                let log_tail = {
                    let lines: Vec<&str> = error_log.lines().collect();
                    let start = lines.len().saturating_sub(80);
                    lines[start..].join("\n")
                };

                match diagnose_and_fix_latex(registry, manifest, &current_tex, &log_tail) {
                    Ok(fixed_tex) => {
                        current_tex = fixed_tex;
                        // Loop continues with fixed source
                    }
                    Err(_) => break, // Can't fix — fall back to HTML
                }
            }
        }
    }

    // HTML fallback
    let html = generate_html_fallback(objective_desc, task_results, images, style);
    let html_path = output_dir.join("output.html");
    std::fs::write(&html_path, &html)
        .map_err(|e| NyayaError::Config(format!("Failed to write HTML: {}", e)))?;
    Ok(html_path)
}

/// Generate a TikZ infographic via LLM for a given description.
///
/// Returns raw TikZ code (\\begin{tikzpicture}...\\end{tikzpicture}).
pub fn generate_infographic(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    description: &str,
) -> Result<String> {
    let input = serde_json::json!({
        "system": TIKZ_SYSTEM_PROMPT,
        "prompt": format!("Create a TikZ diagram for: {}", description),
    });

    let result = registry
        .execute_ability(manifest, "llm.chat", &input.to_string())
        .map_err(|e| NyayaError::Config(format!("LLM call for infographic failed: {}", e)))?;

    let output = String::from_utf8_lossy(&result.output).to_string();

    // Extract just the tikzpicture environment if wrapped in other text
    if let Some(tikz) = extract_tikz(&output) {
        Ok(tikz)
    } else {
        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// LaTeX skeleton template — safe, commonly-available packages only
// ---------------------------------------------------------------------------

/// Safe LaTeX skeleton with only packages guaranteed to be in TeX Live / tectonic.
/// NO pgfornament, NO lettrine, NO draftwatermark — these cause compilation failures.
pub(crate) const LATEX_SKELETON: &str = r#"\documentclass[12pt,a4paper]{report}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage[margin=2.5cm]{geometry}
\usepackage{graphicx}
\usepackage{xcolor}
\usepackage{hyperref}
\usepackage{booktabs}
\usepackage{fancyhdr}
\usepackage{tcolorbox}
\usepackage{tikz}
\usepackage{multicol}
\usepackage{enumitem}
\usepackage{titlesec}
%%STYLE_PREAMBLE%%
\pagestyle{fancy}
\fancyhf{}
\fancyhead[L]{\leftmark}
\fancyhead[R]{\thepage}
\renewcommand{\headrulewidth}{0.4pt}
\hypersetup{colorlinks=true,linkcolor=primarycolor,urlcolor=accentcolor}
\begin{document}
%%TITLE_PAGE%%
\tableofcontents
\newpage
%%CONTENT%%
%%PHOTO_CREDITS%%
\end{document}
"#;

// ---------------------------------------------------------------------------
// LaTeX generation via skeleton + per-section LLM calls
// ---------------------------------------------------------------------------

fn generate_latex_source(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
) -> Result<String> {
    // 1. Build style preamble
    let s = style.cloned().unwrap_or_default();
    let primary_hex = s.primary_color.trim_start_matches('#');
    let accent_hex = s.accent_color.trim_start_matches('#');
    let style_preamble = format!(
        "\\definecolor{{primarycolor}}{{HTML}}{{{}}}\n\
         \\definecolor{{accentcolor}}{{HTML}}{{{}}}\n\
         \\titleformat{{\\chapter}}{{\\normalfont\\LARGE\\bfseries\\color{{primarycolor}}}}{{\\thechapter.}}{{1em}}{{}}\n\
         \\titleformat{{\\section}}{{\\normalfont\\Large\\bfseries\\color{{accentcolor}}}}{{\\thesection}}{{1em}}{{}}",
        primary_hex, accent_hex
    );

    // 2. Build title page
    let title_page = build_title_page(objective_desc, &s);

    // 3. Generate each section's LaTeX body via LLM
    let mut section_bodies = Vec::new();
    for (i, (desc, text)) in task_results.iter().enumerate() {
        let section_tex = generate_section_latex(registry, manifest, desc, text, i, images);
        let sanitized = sanitize_latex(&section_tex);
        section_bodies.push(sanitized);
    }
    let content = section_bodies.join("\n\n");

    // 4. Build photo credits
    let photo_credits = if images.is_empty() {
        String::new()
    } else {
        let mut credits = String::from("\\chapter*{Photo Credits}\n\\addcontentsline{toc}{chapter}{Photo Credits}\n\\begin{itemize}\n");
        for (caption, _, attribution) in images {
            let attr = attribution.as_deref().unwrap_or("Unknown source");
            let safe_caption = latex_escape(caption);
            let safe_attr = latex_escape(attr);
            credits.push_str(&format!("\\item \\textbf{{{}}} — {}\n", safe_caption, safe_attr));
        }
        credits.push_str("\\end{itemize}\n");
        credits
    };

    // 5. Assemble skeleton
    let tex = LATEX_SKELETON
        .replace("%%STYLE_PREAMBLE%%", &style_preamble)
        .replace("%%TITLE_PAGE%%", &title_page)
        .replace("%%CONTENT%%", &content)
        .replace("%%PHOTO_CREDITS%%", &photo_credits);

    Ok(tex)
}

/// Generate LaTeX body content for a single section via LLM.
/// Returns LaTeX body only (no preamble, no \begin{document}).
fn generate_section_latex(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    title: &str,
    content: &str,
    section_idx: usize,
    images: &[ImageEntry],
) -> String {
    // Find images relevant to this section
    let image_hints: String = images
        .iter()
        .filter(|(caption, _, _)| {
            let t_lower = title.to_lowercase();
            let c_lower = caption.to_lowercase();
            // Simple relevance: if any word in caption appears in title
            c_lower.split_whitespace().any(|w| w.len() > 3 && t_lower.contains(w))
        })
        .map(|(caption, path, _)| {
            let fname = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            format!("- {}: \\includegraphics[width=0.8\\textwidth]{{{}}}", caption, fname)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let image_instruction = if image_hints.is_empty() {
        String::new()
    } else {
        format!("\n\nAvailable images for this section (use \\includegraphics where appropriate):\n{}", image_hints)
    };

    let prompt = format!(
        "Convert the following content into LaTeX body content for Chapter {} titled \"{}\".\n\n\
         Content:\n{}\n{}\n\n\
         RULES:\n\
         - Output ONLY LaTeX body content (\\chapter, \\section, \\subsection, etc.)\n\
         - Do NOT output \\documentclass, \\usepackage, \\begin{{document}}, or \\end{{document}}\n\
         - Use \\chapter{{{}}} as the top-level heading\n\
         - Use proper LaTeX formatting: itemize/enumerate for lists, booktabs for tables, tcolorbox for callouts\n\
         - Escape special characters: &, %, $, #, _, {{}}, ~, ^, \\\n\
         - Do NOT use pgfornament, lettrine, or draftwatermark\n\
         - Keep TikZ simple: no blur, no shadow options, no external libraries",
        section_idx + 1, title, content, image_instruction, title
    );

    let input = serde_json::json!({
        "system": SECTION_SYSTEM_PROMPT,
        "prompt": prompt,
        "max_tokens": 8192,
    });

    match registry.execute_ability(manifest, "llm.chat", &input.to_string()) {
        Ok(result) => String::from_utf8_lossy(&result.output).to_string(),
        Err(e) => {
            // Fallback: escape the raw content and wrap in a chapter
            tracing::warn!("LLM section generation failed for '{}': {}", title, e);
            let escaped = latex_escape(content);
            format!("\\chapter{{{}}}\n\n{}", latex_escape(title), escaped)
        }
    }
}

/// Build a LaTeX title page.
fn build_title_page(objective_desc: &str, style: &StyleConfig) -> String {
    let safe_title = latex_escape(objective_desc);
    let theme_note = if style.theme != "clean" {
        format!("\\large\\textit{{{} Edition}}", latex_escape(&style.theme))
    } else {
        String::new()
    };
    format!(
        "\\begin{{titlepage}}\n\
         \\centering\n\
         \\vspace*{{3cm}}\n\
         {{\\Huge\\bfseries\\color{{primarycolor}} {}}}\n\
         \\vspace{{1cm}}\n\n\
         {}\n\
         \\vfill\n\
         {{\\large Generated by NabaOS PEA Engine}}\n\
         \\vspace{{1cm}}\n\n\
         {{\\large \\today}}\n\
         \\end{{titlepage}}\n",
        safe_title, theme_note
    )
}

/// Sanitize LaTeX output from LLM to prevent compilation failures.
pub(crate) fn sanitize_latex(tex: &str) -> String {
    let mut result = tex.to_string();

    // Strip LLM thinking tokens (e.g. Qwen <think>...</think>)
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!("{}{}", &result[..start], &result[start + end + 8..]);
        } else {
            let line_end = result[start..].find('\n').unwrap_or(result.len() - start);
            result = format!("{}{}", &result[..start], &result[start + line_end..]);
        }
    }
    result = result.replace("</think>", "");
    result = result.replace("<think>", "");

    // Strip any preamble/document wrapper the LLM might have emitted
    // Remove \documentclass lines
    result = result.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("\\documentclass")
                && !trimmed.starts_with("\\begin{document}")
                && !trimmed.starts_with("\\end{document}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Remove unsafe packages that cause compilation failures
    let unsafe_packages = [
        "pgfornament", "lettrine", "draftwatermark", "fontspec",
        "luatexja", "polyglossia",
    ];
    for pkg in &unsafe_packages {
        // Match \usepackage{pkg} and \usepackage[options]{pkg}
        let pattern1 = format!("\\usepackage{{{}}}", pkg);
        let pattern2 = format!("\\usepackage[", );
        result = result.lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.contains(&pattern1) && !(trimmed.starts_with(&pattern2) && trimmed.contains(&format!("{{{}}}", pkg)))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    // Remove problematic TikZ options
    result = result.replace("blur radius", "");
    result = result.replace("shadow xshift", "");
    result = result.replace("shadow yshift", "");

    // Fix .tikz references: \includegraphics{*.tikz} -> \input{*.tikz}
    let mut fixed = String::with_capacity(result.len());
    for line in result.lines() {
        if line.contains("\\includegraphics") && line.contains(".tikz") {
            // Extract the filename and replace with \input
            if let Some(start) = line.find('{') {
                if let Some(end) = line.rfind('}') {
                    let filename = &line[start + 1..end];
                    fixed.push_str(&format!("\\input{{{}}}", filename));
                    fixed.push('\n');
                    continue;
                }
            }
        }
        fixed.push_str(line);
        fixed.push('\n');
    }

    // Strip markdown code fences
    fixed = fixed.replace("```latex", "").replace("```tex", "").replace("```", "");

    // Convert \cite{...} to inline text — we don't generate .bib files,
    // so \cite refs always render as [?]. Replace with footnote-style refs.
    fixed = remove_unresolved_cites(&fixed);

    // Balance LaTeX environments: close any unclosed \begin{X} with \end{X}
    fixed = balance_environments(&fixed);

    // Balance braces and math mode
    fixed = balance_braces_and_math(&fixed);

    fixed
}

/// Remove `\cite{key}` commands since we don't generate .bib files.
/// These always render as `[?]` in the PDF. Replace with empty string —
/// the actual source is usually already cited inline as a footnote or URL.
fn remove_unresolved_cites(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    let mut i = 0;
    while i < tex.len() {
        if tex[i..].starts_with("\\cite{") {
            let start = i + 6;
            if let Some(end) = tex[start..].find('}') {
                i = start + end + 1;
                continue;
            }
        }
        // Safe for multi-byte: advance by char
        if let Some(ch) = tex[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

/// Close any unclosed LaTeX environments.
///
/// Scans for `\begin{X}` / `\end{X}` pairs and appends missing `\end{X}`
/// at the end to prevent "ended by \end{document}" fatal errors.
fn balance_environments(tex: &str) -> String {
    let mut stack: Vec<String> = Vec::new();
    // Environments that are part of the skeleton, not LLM-generated
    let skeleton_envs = ["document", "titlepage"];

    for line in tex.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\\begin{") {
            if let Some(end) = rest.find('}') {
                let env = &rest[..end];
                if !skeleton_envs.contains(&env) {
                    stack.push(env.to_string());
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("\\end{") {
            if let Some(end) = rest.find('}') {
                let env = &rest[..end];
                if !skeleton_envs.contains(&env) {
                    // Pop matching env from stack (search from top)
                    if let Some(pos) = stack.iter().rposition(|e| e == env) {
                        stack.remove(pos);
                    }
                }
            }
        }
    }

    if stack.is_empty() {
        return tex.to_string();
    }

    // Close unclosed environments in reverse order
    let mut result = tex.to_string();
    for env in stack.iter().rev() {
        result.push_str(&format!("\n\\end{{{}}}", env));
    }
    result
}

/// Balance braces and math-mode delimiters.
///
/// Counts unmatched `{` and `$` and appends closers to prevent
/// "Missing } inserted" / "Missing $ inserted" fatal errors.
fn balance_braces_and_math(tex: &str) -> String {
    let mut brace_depth: i32 = 0;
    let mut math_open = false;
    let mut math_fixes = 0;
    let mut brace_fixes = 0;
    let mut prev_char = ' ';

    for ch in tex.chars() {
        match ch {
            '{' if prev_char != '\\' => brace_depth += 1,
            '}' if prev_char != '\\' => brace_depth -= 1,
            '$' if prev_char != '\\' => math_open = !math_open,
            _ => {}
        }
        prev_char = ch;
    }

    let mut result = tex.to_string();
    if math_open {
        result.push('$');
        math_fixes = 1;
    }
    if brace_depth > 0 {
        brace_fixes = brace_depth;
        for _ in 0..brace_depth {
            result.push('}');
        }
    }

    if math_fixes > 0 || brace_fixes > 0 {
        eprintln!(
            "[pea/doc] sanitize: closed {} unclosed braces, {} unclosed math-mode",
            brace_fixes, math_fixes
        );
    }

    result
}

/// Escape special LaTeX characters in plain text.
fn latex_escape(text: &str) -> String {
    text.replace('\\', "\\textbackslash{}")
        .replace('&', "\\&")
        .replace('%', "\\%")
        .replace('$', "\\$")
        .replace('#', "\\#")
        .replace('_', "\\_")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('~', "\\textasciitilde{}")
        .replace('^', "\\textasciicircum{}")
}

// ---------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------

pub(crate) fn postprocess_latex(tex: &str, images: &[ImageEntry], output_dir: &Path) -> String {
    let mut result = tex.to_string();

    // Fix image paths: replace any absolute/relative paths with just filenames
    // since we'll copy images to output_dir
    for (_, path, _) in images {
        if let Some(filename) = path.file_name() {
            let filename_str = filename.to_string_lossy();
            // Copy image to output dir
            let dest = output_dir.join(filename);
            let _ = std::fs::copy(path, &dest);

            // Replace full path references with just the filename
            let path_str = path.to_string_lossy();
            result = result.replace(&*path_str, &filename_str);
        }
    }

    result
}

/// Ask the LLM to fix LaTeX compilation errors, then sanitize the result.
pub(crate) fn diagnose_and_fix_latex(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    tex_source: &str,
    error_log: &str,
) -> Result<String> {
    let prompt = format!(
        "The following LaTeX document failed to compile. Fix the errors and return the COMPLETE corrected LaTeX source.\n\n\
         IMPORTANT RULES:\n\
         - Do NOT use pgfornament, lettrine, or draftwatermark packages\n\
         - Do NOT use TikZ blur or shadow options\n\
         - Only use packages from standard TeX Live: geometry, fancyhdr, graphicx, xcolor, booktabs, tcolorbox, tikz, hyperref, multicol, enumitem, titlesec\n\n\
         COMPILATION ERRORS:\n{}\n\n\
         ORIGINAL LATEX SOURCE:\n{}\n\n\
         Output ONLY the corrected LaTeX source, starting with \\documentclass and ending with \\end{{document}}. \
         Do not include any explanation.",
        error_log, tex_source
    );

    let input = serde_json::json!({
        "system": LATEX_SYSTEM_PROMPT,
        "prompt": prompt,
        "max_tokens": 16384,
    });

    let result = registry
        .execute_ability(manifest, "llm.chat", &input.to_string())
        .map_err(|e| NyayaError::Config(format!("LLM call for LaTeX fix failed: {}", e)))?;

    let raw_output = String::from_utf8_lossy(&result.output).to_string();
    let extracted = extract_latex_source(&raw_output);
    // Apply sanitization to the LLM fix as well
    Ok(sanitize_latex(&extracted))
}

fn extract_latex_source(raw: &str) -> String {
    // If wrapped in ```latex ... ``` or ```tex ... ```, extract inner content
    if let Some(start) = raw.find("\\documentclass") {
        if let Some(end) = raw.rfind("\\end{document}") {
            return raw[start..end + "\\end{document}".len()].to_string();
        }
    }
    raw.to_string()
}

fn extract_tikz(raw: &str) -> Option<String> {
    let start = raw.find("\\begin{tikzpicture}")?;
    let end = raw.rfind("\\end{tikzpicture}")?;
    if end < start {
        return None;
    }
    Some(raw[start..end + "\\end{tikzpicture}".len()].to_string())
}

// ---------------------------------------------------------------------------
// HTML fallback
// ---------------------------------------------------------------------------

fn generate_html_fallback(
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
) -> String {
    let escaped_title = html_escape(objective_desc);

    let s = style.cloned().unwrap_or_default();

    let font_stack = match s.font_family.as_str() {
        "sans-serif" => "'Helvetica Neue', Arial, sans-serif",
        "monospace" => "'JetBrains Mono', 'Fira Code', monospace",
        _ => "'Georgia', 'Palatino', serif",
    };

    let watermark_css = if let Some(ref wm) = s.watermark_text {
        format!(
            r##"
body::before {{
  content: "{}";
  position: fixed; top: 50%; left: 50%;
  transform: translate(-50%, -50%) rotate(-30deg);
  font-size: 6rem; color: {}; opacity: {:.2};
  pointer-events: none; z-index: -1; white-space: nowrap;
}}"##,
            html_escape(wm),
            s.primary_color,
            s.watermark_opacity.max(0.03).min(0.15),
        )
    } else {
        String::new()
    };

    let primary = &s.primary_color;
    let accent = &s.accent_color;

    let mut sections = String::new();
    for (i, (desc, text)) in task_results.iter().enumerate() {
        sections.push_str(&format!(
            "<section>\n<h2>{}. {}</h2>\n<div class=\"content\">{}</div>\n</section>\n",
            i + 1,
            html_escape(desc),
            text_to_html(text)
        ));
    }

    let mut image_html = String::new();
    let mut credits: Vec<String> = Vec::new();
    for (caption, path, attribution) in images {
        if let Some(filename) = path.file_name() {
            let attr_line = if let Some(attr) = attribution {
                credits.push(format!(
                    "<li><strong>{}</strong> — {}</li>",
                    html_escape(caption),
                    html_escape(attr)
                ));
                format!(
                    "<figcaption>{} <span class=\"attribution\">{}</span></figcaption>",
                    html_escape(caption),
                    html_escape(attr)
                )
            } else {
                format!("<figcaption>{}</figcaption>", html_escape(caption))
            };
            image_html.push_str(&format!(
                "<figure><img src=\"{}\" alt=\"{}\">{}</figure>\n",
                filename.to_string_lossy(),
                html_escape(caption),
                attr_line,
            ));
        }
    }

    let credits_html = if credits.is_empty() {
        String::new()
    } else {
        format!(
            "<section class=\"credits\">\n<h2>Photo Credits</h2>\n<ul>\n{}\n</ul>\n\
             <p class=\"credits-note\">Images used under royalty-free license. \
             Attribution provided as required by the image source.</p>\n</section>\n",
            credits.join("\n")
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
:root {{ --primary-color: {primary}; --accent-color: {accent}; }}
body {{ font-family: {font_stack}; max-width: 800px; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: var(--primary-color); }}
h1 {{ text-align: center; border-bottom: 2px solid var(--primary-color); padding-bottom: 0.5em; }}
h2 {{ color: var(--accent-color); border-bottom: 1px solid #ddd; padding-bottom: 0.3em; }}
.content {{ margin: 1em 0; }}
figure {{ text-align: center; margin: 2em 0; }}
figure img {{ max-width: 100%; height: auto; border-radius: 6px; box-shadow: 0 2px 8px rgba(0,0,0,0.15); }}
figcaption {{ font-style: italic; color: #666; margin-top: 0.5em; }}
figcaption .attribution {{ font-size: 0.85em; color: #999; display: block; }}
.credits {{ margin-top: 3em; padding-top: 1em; border-top: 1px solid #ddd; }}
.credits-note {{ font-size: 0.85em; color: #888; font-style: italic; }}
section {{ margin-bottom: 2em; }}
.toc {{ background: #f9f9f9; padding: 1em 2em; border-radius: 4px; margin: 1em 0 2em; border-left: 4px solid var(--accent-color); }}
.toc h3 {{ margin-top: 0; }}
.toc ol {{ padding-left: 1.5em; }}
{watermark_css}
</style>
</head>
<body>
<h1>{title}</h1>
<nav class="toc">
<h3>Contents</h3>
<ol>
{toc}
</ol>
</nav>
{sections}
{images}
{credits}
<footer><p><em>Generated by NabaOS PEA Engine</em></p></footer>
</body>
</html>"#,
        title = escaped_title,
        primary = primary,
        accent = accent,
        font_stack = font_stack,
        watermark_css = watermark_css,
        toc = task_results
            .iter()
            .enumerate()
            .map(|(_, (desc, _))| format!("<li>{}</li>", html_escape(desc)))
            .collect::<Vec<_>>()
            .join("\n"),
        sections = sections,
        images = image_html,
        credits = credits_html,
    )
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn text_to_html(text: &str) -> String {
    let escaped = html_escape(text);
    // Convert double newlines to paragraphs
    let paragraphs: Vec<&str> = escaped.split("\n\n").collect();
    if paragraphs.len() > 1 {
        paragraphs
            .iter()
            .map(|p| format!("<p>{}</p>", p.replace('\n', "<br>")))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("<p>{}</p>", escaped.replace('\n', "<br>"))
    }
}

// ---------------------------------------------------------------------------
// System prompts
// ---------------------------------------------------------------------------

const LATEX_SYSTEM_PROMPT: &str = "\
You are an expert LaTeX typesetter. You produce complete, compilable LaTeX documents \
with professional formatting. You use ONLY standard TeX Live packages (geometry, fancyhdr, \
graphicx, xcolor, booktabs, tcolorbox, tikz, multicol, hyperref, enumitem, titlesec). \
NEVER use pgfornament, lettrine, draftwatermark, or fontspec. Always output ONLY the \
LaTeX source code. Never include explanation text outside the LaTeX document.";

const SECTION_SYSTEM_PROMPT: &str = "\
You are an expert LaTeX typesetter generating section body content. Output ONLY LaTeX \
body commands (\\chapter, \\section, \\subsection, text, itemize, enumerate, tables, \
tcolorbox). Do NOT output \\documentclass, \\usepackage, \\begin{document}, or \
\\end{document}. Use ONLY standard packages. NEVER use pgfornament, lettrine, or \
draftwatermark. Keep TikZ simple — no blur, shadow, or external libraries.";

const TIKZ_SYSTEM_PROMPT: &str = "\
You are a TikZ/PGF expert. You produce clean, compilable TikZ code for diagrams, \
infographics, flowcharts, timelines, and data visualizations. Output ONLY the \
\\begin{tikzpicture}...\\end{tikzpicture} block with no surrounding text.";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_latex_source_plain() {
        let raw = "\\documentclass{article}\n\\begin{document}\nHello\n\\end{document}";
        let result = extract_latex_source(raw);
        assert!(result.starts_with("\\documentclass"));
        assert!(result.ends_with("\\end{document}"));
    }

    #[test]
    fn test_extract_latex_source_with_markdown_fences() {
        let raw = "Here is the document:\n```latex\n\\documentclass{article}\n\\begin{document}\nHello\n\\end{document}\n```\nDone.";
        let result = extract_latex_source(raw);
        assert!(result.starts_with("\\documentclass"));
        assert!(result.ends_with("\\end{document}"));
    }

    #[test]
    fn test_extract_tikz() {
        let raw = "Here is the diagram:\n\\begin{tikzpicture}\n\\draw (0,0) -- (1,1);\n\\end{tikzpicture}\nDone.";
        let tikz = extract_tikz(raw).unwrap();
        assert!(tikz.starts_with("\\begin{tikzpicture}"));
        assert!(tikz.ends_with("\\end{tikzpicture}"));
    }

    #[test]
    fn test_extract_tikz_no_match() {
        assert!(extract_tikz("no tikz here").is_none());
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<b>hello & \"world\"</b>"), "&lt;b&gt;hello &amp; &quot;world&quot;&lt;/b&gt;");
    }

    #[test]
    fn test_generate_html_fallback_structure() {
        let results = vec![
            ("Introduction".to_string(), "This is the intro.".to_string()),
            ("Chapter 1".to_string(), "Content here.".to_string()),
        ];
        let html = generate_html_fallback("Test Document", &results, &[], None);
        assert!(html.contains("<title>Test Document</title>"));
        assert!(html.contains("<h2>1. Introduction</h2>"));
        assert!(html.contains("<h2>2. Chapter 1</h2>"));
        assert!(html.contains("NabaOS PEA Engine"));
    }

    #[test]
    fn test_styled_html_fallback() {
        let style = StyleConfig {
            primary_color: "#8B4513".into(),
            accent_color: "#DAA520".into(),
            font_family: "serif".into(),
            watermark_text: Some("Test Watermark".into()),
            watermark_opacity: 0.08,
            ..Default::default()
        };
        let results = vec![("Chapter 1".into(), "Content here.".into())];
        let html = generate_html_fallback("Test Doc", &results, &[], Some(&style));
        assert!(html.contains("#8B4513"));
        assert!(html.contains("Test Watermark"));
        assert!(html.contains("--primary-color"));
    }

    #[test]
    fn test_html_attribution_credits() {
        let images: Vec<ImageEntry> = vec![
            ("Biryani dish".to_string(), PathBuf::from("/tmp/biryani.jpg"), Some("Photo by Chef on Unsplash".to_string())),
            ("Spice market".to_string(), PathBuf::from("/tmp/spices.jpg"), None),
        ];
        let results = vec![("Chapter 1".into(), "Content.".into())];
        let html = generate_html_fallback("Test", &results, &images, None);
        assert!(html.contains("Photo by Chef on Unsplash"));
        assert!(html.contains("class=\"attribution\""));
        assert!(html.contains("Photo Credits"));
        assert!(html.contains("royalty-free license"));
        assert!(html.contains("Spice market</figcaption>"));
    }

    #[test]
    fn test_text_to_html_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let html = text_to_html(text);
        assert!(html.contains("<p>First paragraph.</p>"));
        assert!(html.contains("<p>Second paragraph.</p>"));
    }

    #[test]
    fn test_postprocess_latex_image_paths() {
        let tex = "\\includegraphics{/tmp/images/photo.jpg}";
        let images: Vec<ImageEntry> = vec![
            ("A photo".to_string(), PathBuf::from("/tmp/images/photo.jpg"), Some("Photo by Test on Unsplash".to_string())),
        ];
        let result = postprocess_latex(tex, &images, Path::new("/tmp/test_output"));
        assert!(result.contains("photo.jpg"));
        assert!(!result.contains("/tmp/images/"));
    }

    #[test]
    fn test_parse_style_config_valid() {
        let json = r##"{
            "theme": "oriental",
            "primary_color": "#8B4513",
            "accent_color": "#DAA520",
            "font_family": "serif",
            "ornament_style": "floral",
            "watermark_text": "Mughlai Cuisine",
            "watermark_opacity": 0.08,
            "chapter_style": "ornate",
            "use_drop_caps": true,
            "image_queries": [
                {"query": "mughlai biryani", "placement": "chapter_header", "chapter": "Biryani"}
            ]
        }"##;
        let config: StyleConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.theme, "oriental");
        assert_eq!(config.image_queries.len(), 1);
        assert_eq!(config.image_queries[0].query, "mughlai biryani");
        assert!(config.watermark_text.is_some());
    }

    #[test]
    fn test_parse_style_config_minimal() {
        let json = r##"{
            "theme": "academic",
            "primary_color": "#333333",
            "accent_color": "#0066CC",
            "font_family": "serif",
            "ornament_style": "none",
            "watermark_text": null,
            "watermark_opacity": 0.0,
            "chapter_style": "clean",
            "use_drop_caps": false,
            "image_queries": []
        }"##;
        let config: StyleConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.theme, "academic");
        assert!(config.watermark_text.is_none());
        assert!(config.image_queries.is_empty());
    }

    #[test]
    fn test_style_config_default() {
        let config = StyleConfig::default();
        assert_eq!(config.theme, "clean");
        assert_eq!(config.ornament_style, "none");
        assert!(!config.use_drop_caps);
    }

    #[test]
    fn test_build_style_analysis_prompt() {
        let prompt = build_style_analysis_prompt(
            "Create a Mughlai cookbook with 10 recipes",
            &[("Introduction".to_string(), "Mughlai cuisine...".to_string())],
        );
        assert!(prompt.contains("Mughlai cookbook"));
        assert!(prompt.contains("image_queries"));
    }

    // --- LaTeX sanitization tests ---

    #[test]
    fn test_sanitize_strips_documentclass() {
        let input = "\\documentclass{article}\n\\usepackage{graphicx}\n\\begin{document}\nHello\n\\end{document}";
        let result = sanitize_latex(input);
        assert!(!result.contains("\\documentclass"));
        assert!(!result.contains("\\begin{document}"));
        assert!(!result.contains("\\end{document}"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_sanitize_removes_unsafe_packages() {
        let input = "\\usepackage{pgfornament}\n\\usepackage{lettrine}\n\\usepackage{draftwatermark}\n\\chapter{Test}";
        let result = sanitize_latex(input);
        assert!(!result.contains("pgfornament"));
        assert!(!result.contains("lettrine"));
        assert!(!result.contains("draftwatermark"));
        assert!(result.contains("\\chapter{Test}"));
    }

    #[test]
    fn test_sanitize_removes_blur_radius() {
        let input = "\\node[blur radius=3pt] {Hello};";
        let result = sanitize_latex(input);
        assert!(!result.contains("blur radius"));
    }

    #[test]
    fn test_sanitize_fixes_tikz_includegraphics() {
        let input = "\\includegraphics{diagram.tikz}";
        let result = sanitize_latex(input);
        assert!(result.contains("\\input{diagram.tikz}"));
        assert!(!result.contains("\\includegraphics"));
    }

    #[test]
    fn test_sanitize_strips_markdown_fences() {
        let input = "```latex\n\\chapter{Test}\n```";
        let result = sanitize_latex(input);
        assert!(!result.contains("```"));
        assert!(result.contains("\\chapter{Test}"));
    }

    #[test]
    fn test_latex_escape() {
        assert_eq!(latex_escape("100% & $50 #1"), "100\\% \\& \\$50 \\#1");
        assert_eq!(latex_escape("a_b"), "a\\_b");
    }

    #[test]
    fn test_latex_skeleton_is_valid() {
        // Verify skeleton contains required placeholders
        assert!(LATEX_SKELETON.contains("%%STYLE_PREAMBLE%%"));
        assert!(LATEX_SKELETON.contains("%%TITLE_PAGE%%"));
        assert!(LATEX_SKELETON.contains("%%CONTENT%%"));
        assert!(LATEX_SKELETON.contains("%%PHOTO_CREDITS%%"));
        // Verify it doesn't contain unsafe packages
        assert!(!LATEX_SKELETON.contains("pgfornament"));
        assert!(!LATEX_SKELETON.contains("lettrine"));
        assert!(!LATEX_SKELETON.contains("draftwatermark"));
    }

    #[test]
    fn test_build_title_page() {
        let style = StyleConfig::default();
        let title_page = build_title_page("Test Document", &style);
        assert!(title_page.contains("Test Document"));
        assert!(title_page.contains("\\begin{titlepage}"));
        assert!(title_page.contains("\\end{titlepage}"));
        assert!(title_page.contains("NabaOS PEA Engine"));
    }

    // --- Environment balancing tests ---

    #[test]
    fn test_balance_environments_unclosed_tabular() {
        let input = "\\begin{tabular}{|l|l|}\nA & B \\\\\n";
        let result = balance_environments(input);
        assert!(result.contains("\\end{tabular}"));
    }

    #[test]
    fn test_balance_environments_already_balanced() {
        let input = "\\begin{itemize}\n\\item A\n\\end{itemize}\n";
        let result = balance_environments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_balance_environments_nested() {
        let input = "\\begin{table}\n\\begin{tabular}{l}\nA\n\\end{tabular}\n";
        let result = balance_environments(input);
        assert!(result.contains("\\end{table}"));
        // tabular was already closed, only table should be added
        assert_eq!(result.matches("\\end{table}").count(), 1);
    }

    #[test]
    fn test_balance_environments_skips_document() {
        // document env is in skeleton, should not be touched
        let input = "\\begin{itemize}\n\\item A\n";
        let result = balance_environments(input);
        assert!(result.contains("\\end{itemize}"));
        assert!(!result.contains("\\end{document}"));
    }

    #[test]
    fn test_balance_braces_unclosed() {
        let input = "Hello {world";
        let result = balance_braces_and_math(input);
        assert!(result.ends_with('}'));
    }

    #[test]
    fn test_balance_braces_balanced() {
        let input = "Hello {world}";
        let result = balance_braces_and_math(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_balance_math_unclosed() {
        let input = "The value is $x + y";
        let result = balance_braces_and_math(input);
        assert!(result.ends_with('$'));
    }

    #[test]
    fn test_sanitize_closes_unclosed_environments() {
        let input = "\\begin{tabular}{|l|l|}\nA & B \\\\\nC & D \\\\\n";
        let result = sanitize_latex(input);
        assert!(result.contains("\\end{tabular}"));
    }

    #[test]
    fn test_sanitize_strips_think_tags() {
        let input = "Hello </think> world <think>reasoning</think> end";
        let result = sanitize_latex(input);
        assert!(!result.contains("</think>"));
        assert!(!result.contains("<think>"));
        assert!(result.contains("Hello"));
        assert!(result.contains("end"));
    }

    #[test]
    fn test_remove_unresolved_cites() {
        let input = "As shown \\cite{smith2024} in recent work \\cite{jones2023}.";
        let result = remove_unresolved_cites(input);
        assert_eq!(result, "As shown  in recent work .");
        assert!(!result.contains("\\cite"));
    }

    #[test]
    fn test_remove_unresolved_cites_no_cites() {
        let input = "No citations here.";
        let result = remove_unresolved_cites(input);
        assert_eq!(result, input);
    }

    // --- Skip stock images tests ---

    #[test]
    fn test_skip_stock_images_defaults_true() {
        let config = StyleConfig::default();
        assert!(config.skip_stock_images);
    }

    #[test]
    fn test_analytical_theme_skips() {
        for theme in &["analytical", "academic", "corporate", "technical", "minimal", "editorial", "clean"] {
            let config = StyleConfig { theme: theme.to_string(), ..Default::default() };
            assert!(config.should_skip_stock_images(), "theme '{}' should skip stock images", theme);
        }
    }

    #[test]
    fn test_creative_theme_keeps() {
        let config = StyleConfig { theme: "creative".into(), ..Default::default() };
        assert!(!config.should_skip_stock_images());
        let config2 = StyleConfig { theme: "oriental".into(), ..Default::default() };
        assert!(!config2.should_skip_stock_images());
    }
}
