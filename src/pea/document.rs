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
        // Env var override takes priority
        if let Ok(v) = std::env::var("NABA_PEA_SKIP_STOCKS") {
            if v == "0" || v.eq_ignore_ascii_case("false") {
                return false;
            }
        }
        // Analytical/report themes ALWAYS skip stock images regardless of LLM output
        let is_analytical_theme = matches!(
            self.theme.to_ascii_lowercase().as_str(),
            "analytical" | "academic" | "corporate" | "technical" | "minimal" | "editorial" | "clean"
        );
        if is_analytical_theme {
            return true;
        }
        // Non-analytical themes keep stock images by default
        false
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
    output_mode: &crate::pea::objective::OutputMode,
) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to create output dir: {}", e)))?;

    // Branch by output mode
    use crate::pea::objective::OutputMode;
    match output_mode {
        OutputMode::Academic => {} // fall through to existing LaTeX pipeline below
        OutputMode::Magazine => {
            return generate_magazine_html(objective_desc, task_results, images, style, output_dir);
        }
        OutputMode::Blog => {
            return generate_blog_html(objective_desc, task_results, images, style, output_dir);
        }
        OutputMode::Video => {
            return generate_video(objective_desc, task_results, images, style, output_dir);
        }
    }

    // 1. Generate LaTeX source via LLM
    let tex_source = generate_latex_source(registry, manifest, objective_desc, task_results, images, style)?;

    // 2. Post-process: fix image paths for the output directory
    let tex_source = postprocess_latex(&tex_source, images, output_dir);

    // 3. Try to compile to PDF with self-healing retry loop
    let tex_path = output_dir.join("output.tex");
    let log_path = output_dir.join("output.log");
    let backend = LatexBackend::detect();

    let mut current_tex = tex_source;

    // 2b. LaTeX lint + auto-fix
    let lint_errors = lint_latex(&current_tex);
    if !lint_errors.is_empty() {
        for e in &lint_errors {
            eprintln!("[pea/doc] lint: {:?} — {}", e.severity, e.detail);
        }
        current_tex = auto_fix_lint(&current_tex, &lint_errors);
    }

    let max_retries = 3;

    for attempt in 0..max_retries {
        std::fs::write(&tex_path, &current_tex)
            .map_err(|e| NyayaError::Config(format!("Failed to write .tex file: {}", e)))?;

        // Use double-pass for documents with ToC (gated by NABA_PEA_DOUBLE_PASS)
        let double_pass = std::env::var("NABA_PEA_DOUBLE_PASS")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        let compile_result = if double_pass && current_tex.contains("\\tableofcontents") {
            backend.compile_twice(&tex_path, output_dir)
        } else {
            backend.compile(&tex_path, output_dir)
        };

        match compile_result {
            Ok(pdf_path) => {
                // Post-compile QA: analyse log + toc
                let toc_path = output_dir.join("output.toc");
                let (qa_warnings, _critical) = analyse_compile_log(&log_path, &toc_path);
                for w in &qa_warnings {
                    eprintln!("[pea/doc] compile QA: {}", w);
                }
                return Ok(pdf_path);
            }
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
\usepackage{tabularx}
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
    let mut claimed_charts: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, (desc, text)) in task_results.iter().enumerate() {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains("reference") || desc_lower.contains("bibliograph") {
            // Render APA bibliography directly — no LLM typesetter
            let section_tex = render_references_section_latex(desc, text);
            section_bodies.push(section_tex);
        } else {
            let section_tex = generate_section_latex(registry, manifest, desc, text, i, images, &mut claimed_charts);
            let sanitized = sanitize_latex(&section_tex);
            section_bodies.push(sanitized);
        }
    }

    // 3b. Force-embed any generated charts that keyword matching missed
    if !images.is_empty() {
        // Merge claimed charts with those found in section bodies
        for (_, path, _) in images.iter() {
            let fname = path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            if section_bodies.iter().any(|body| body.contains(&fname)) {
                claimed_charts.insert(fname);
            }
        }

        let unplaced: Vec<&ImageEntry> = images.iter()
            .filter(|(_, path, _)| {
                let fname = path.file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                !claimed_charts.contains(&fname)
            })
            .collect();

        if !unplaced.is_empty() {
            // Distribute unplaced charts to content sections (skip first = exec summary)
            let content_indices: Vec<usize> = (1..section_bodies.len())
                .filter(|i| section_bodies[*i].len() > 500)
                .collect();

            for (_i, (caption, path, _)) in unplaced.iter().enumerate() {
                let cap_lower = caption.to_lowercase();
                // Try keyword match: chart caption → section title
                let target_idx = task_results.iter().enumerate()
                    .find(|(_idx, (desc, _))| {
                        let d = desc.to_lowercase();
                        if cap_lower.contains("prisma") || cap_lower.contains("flow") {
                            d.contains("method") || d.contains("prisma") || d.contains("search")
                        } else if cap_lower.contains("source") || cap_lower.contains("distribution") {
                            d.contains("method") || d.contains("data") || d.contains("source")
                        } else if cap_lower.contains("prospect theory") || cap_lower.contains("value function") {
                            d.contains("theor") || d.contains("framework") || d.contains("prospect")
                        } else if cap_lower.contains("forest plot") || cap_lower.contains("effect size") {
                            d.contains("method") || d.contains("finding") || d.contains("result") || d.contains("empiric")
                        } else if cap_lower.contains("funnel plot") || cap_lower.contains("publication bias") {
                            d.contains("method") || d.contains("finding") || d.contains("result") || d.contains("bias")
                        } else {
                            false
                        }
                    })
                    .map(|(idx, _)| idx)
                    // Fallback: first content section
                    .or_else(|| content_indices.first().copied())
                    .unwrap_or(0);
                if target_idx < section_bodies.len() {
                    let fname = path.file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let safe_caption = latex_escape(caption);
                    let inject = format!(
                        "\n\n\\begin{{figure}}[htbp]\n\\centering\n\\includegraphics[width=0.85\\textwidth]{{{}}}\n\\caption{{{}}}\n\\end{{figure}}\n",
                        fname, safe_caption
                    );
                    section_bodies[target_idx].push_str(&inject);
                    claimed_charts.insert(fname.clone());
                    eprintln!("[document] force-embedding chart '{}' into section {}", fname, target_idx);
                }
            }
        }
    }

    let content = section_bodies.join("\n\n");

    // 4. Build photo credits (stock images only, not auto-generated charts)
    let stock_images: Vec<_> = images.iter()
        .filter(|(_, _, attribution)| {
            match attribution.as_deref() {
                Some(a) if a.contains("Auto-generated") => false,
                _ => true,
            }
        })
        .collect();
    let photo_credits = if stock_images.is_empty() {
        String::new()
    } else {
        let mut credits = String::from("\\chapter*{Photo Credits}\n\\addcontentsline{toc}{chapter}{Photo Credits}\n\\begin{itemize}\n");
        for (caption, _, attribution) in &stock_images {
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

/// Render APA bibliography directly in LaTeX — no LLM typesetter needed.
/// Uses description list with hanging indent for proper APA formatting.
fn render_references_section_latex(title: &str, content: &str) -> String {
    let safe_title = latex_escape(title);
    let mut tex = format!(
        "\\chapter*{{{}}}\n\\addcontentsline{{toc}}{{chapter}}{{{}}}\n\n\
         \\begin{{description}}[leftmargin=2em,labelindent=0em,itemsep=0.5em,parsep=0em,font=\\normalfont]\n",
        safe_title, safe_title
    );

    // Each reference entry is separated by blank lines
    for entry in content.split("\n\n") {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // Escape LaTeX special chars in the entry
        let safe_entry = latex_escape(entry);
        // Convert *Journal* markdown italics to \textit{Journal}
        let formatted = convert_markdown_italics(&safe_entry);
        // Wrap DOI/URLs in \url{}
        let formatted = wrap_urls_in_latex(&formatted);
        tex.push_str(&format!("\\item[] {}\n", formatted));
    }

    tex.push_str("\\end{description}\n");
    tex
}

/// Convert *text* markdown italics to \textit{text} for LaTeX.
fn convert_markdown_italics(s: &str) -> String {
    let re = regex::Regex::new(r"\*([^*]+)\*").unwrap();
    re.replace_all(s, "\\textit{$1}").to_string()
}

/// Wrap bare URLs in \url{} for LaTeX.
fn wrap_urls_in_latex(s: &str) -> String {
    let re = regex::Regex::new(r"(https?://[^\s,}]+)").unwrap();
    let mut result = String::new();
    let mut last_end = 0;
    for m in re.find_iter(s) {
        let prefix = &s[..m.start()];
        // Skip if already wrapped in \url{}
        if prefix.ends_with("\\url{") {
            continue;
        }
        result.push_str(&s[last_end..m.start()]);
        result.push_str(&format!("\\url{{{}}}", m.as_str()));
        last_end = m.end();
    }
    result.push_str(&s[last_end..]);
    result
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
    claimed_charts: &mut std::collections::HashSet<String>,
) -> String {
    // Find images relevant to this section — only unclaimed charts
    let mut hints_for_section = Vec::new();
    for (caption, path, _) in images.iter() {
        let fname = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if claimed_charts.contains(&fname) {
            continue; // Already assigned to an earlier section
        }
        let t_lower = title.to_lowercase();
        let c_lower = caption.to_lowercase();
        if c_lower.split_whitespace().any(|w| w.len() > 3 && t_lower.contains(w)) {
            claimed_charts.insert(fname.clone());
            hints_for_section.push(format!("- {}: \\includegraphics[width=0.8\\textwidth]{{{}}}", caption, fname));
        }
    }
    let image_hints: String = hints_for_section.join("\n");

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

    // Convert markdown pipe tables to LaTeX tabular environments
    fixed = convert_markdown_tables(&fixed);

    // Balance LaTeX environments: close any unclosed \begin{X} with \end{X}
    fixed = balance_environments(&fixed);

    // Balance braces and math mode
    fixed = balance_braces_and_math(&fixed);

    // Fix stray \item commands outside list environments
    fixed = fix_stray_items(&fixed);

    fixed
}

/// Like `sanitize_latex` but preserves \documentclass, \begin{document}, \end{document}.
/// Used by `diagnose_and_fix_latex` where the LLM returns a complete document.
pub(crate) fn sanitize_latex_preserve_structure(tex: &str) -> String {
    let mut result = tex.to_string();

    // Strip LLM thinking tokens
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

    // Remove unsafe packages
    let unsafe_packages = [
        "pgfornament", "lettrine", "draftwatermark", "fontspec",
        "luatexja", "polyglossia",
    ];
    for pkg in &unsafe_packages {
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

    // Fix .tikz references
    let mut fixed = String::with_capacity(result.len());
    for line in result.lines() {
        if line.contains("\\includegraphics") && line.contains(".tikz") {
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

    fixed = remove_unresolved_cites(&fixed);
    fixed = convert_markdown_tables(&fixed);
    fixed = balance_environments(&fixed);
    fixed = balance_braces_and_math(&fixed);
    fixed = fix_stray_items(&fixed);

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

/// Convert markdown pipe tables to LaTeX tabularx environments.
///
/// Detects consecutive lines with `|` delimiters, skips the separator line
/// (containing `---`), and emits a `\begin{table}...\end{table}` with
/// tabularx `X` columns that auto-distribute width across `\textwidth`.
fn convert_markdown_tables(tex: &str) -> String {
    let lines: Vec<&str> = tex.lines().collect();
    let mut out = String::with_capacity(tex.len());
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        // Detect start of a markdown table: line with 3+ pipes, followed by separator
        if line.matches('|').count() >= 3
            && i + 1 < lines.len()
            && lines[i + 1].trim().contains("---")
            && lines[i + 1].trim().contains('|')
            // Don't convert if this is already inside a LaTeX table
            && !line.contains("\\begin{")
        {
            // Parse header
            let header_cells: Vec<&str> = line.split('|')
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
                .collect();
            let ncols = header_cells.len();
            if ncols == 0 {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
                continue;
            }

            // Use tabularx with X columns to auto-distribute width
            let col_spec: String = (0..ncols)
                .map(|_| "X")
                .collect::<Vec<_>>()
                .join(" ");

            out.push_str("\\begin{table}[htbp]\n\\centering\n\\small\n");
            out.push_str(&format!("\\begin{{tabularx}}{{\\textwidth}}{{{}}}\n\\hline\n", col_spec));

            // Header row (bold)
            let header_tex: Vec<String> = header_cells.iter()
                .map(|c| format!("\\textbf{{{}}}", latex_escape(c)))
                .collect();
            out.push_str(&header_tex.join(" & "));
            out.push_str(" \\\\\n\\hline\n");

            // Skip separator line
            i += 2;

            // Data rows
            while i < lines.len() {
                let row = lines[i].trim();
                if row.matches('|').count() < 3 || row.is_empty() {
                    break;
                }
                let cells: Vec<&str> = row.split('|')
                    .map(|c| c.trim())
                    .filter(|c| !c.is_empty())
                    .collect();
                let row_tex: Vec<String> = cells.iter()
                    .map(|c| latex_escape(c))
                    .collect();
                out.push_str(&row_tex.join(" & "));
                out.push_str(" \\\\\n");
                i += 1;
            }

            out.push_str("\\hline\n\\end{tabularx}\n\\end{table}\n");
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }

    out
}

/// Close any unclosed LaTeX environments.
///
/// Scans for `\begin{X}` / `\end{X}` pairs and appends missing `\end{X}`
/// at the end to prevent "ended by \end{document}" fatal errors.
/// Fix stray `\item` commands that appear outside any list environment.
/// Wraps consecutive orphan `\item` lines in `\begin{itemize}...\end{itemize}`.
fn fix_stray_items(tex: &str) -> String {
    let list_envs = ["itemize", "enumerate", "description"];
    let lines: Vec<&str> = tex.lines().collect();
    let mut out = String::with_capacity(tex.len());
    let mut depth = 0i32; // nesting depth of list environments

    for line in &lines {
        let trimmed = line.trim();
        for env in &list_envs {
            if trimmed.contains(&format!("\\begin{{{}}}", env)) {
                depth += 1;
            }
            if trimmed.contains(&format!("\\end{{{}}}", env)) {
                depth -= 1;
            }
        }
        if depth < 0 {
            depth = 0;
        }
        if depth == 0 && trimmed.starts_with("\\item") {
            // Stray \item — wrap in itemize
            out.push_str("\\begin{itemize}\n");
            out.push_str(line);
            out.push('\n');
            out.push_str("\\end{itemize}\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

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
    for (caption, path, _) in images {
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

    // Wrap bare \includegraphics (not already inside \begin{figure}) in figure envs with labels
    result = wrap_bare_includegraphics(&result, images);

    result
}

/// Wrap \includegraphics lines that aren't inside a figure environment into
/// proper figure floats with caption and label. This prevents "Figure ??" refs.
fn wrap_bare_includegraphics(tex: &str, images: &[ImageEntry]) -> String {
    let lines: Vec<&str> = tex.lines().collect();
    let mut output = Vec::with_capacity(lines.len());
    let mut fig_counter = 0;
    let mut in_figure = false;

    for (i, line) in lines.iter().enumerate() {
        if line.contains("\\begin{figure}") {
            in_figure = true;
        }
        if line.contains("\\end{figure}") {
            in_figure = false;
        }

        if line.contains("\\includegraphics") && !in_figure {
            fig_counter += 1;
            let label = format!("fig:auto{}", fig_counter);

            // Try to find a matching caption from images
            let caption = images.iter()
                .find(|(_, p, _)| {
                    if let Some(fname) = p.file_name() {
                        line.contains(&fname.to_string_lossy().as_ref())
                    } else {
                        false
                    }
                })
                .map(|(c, _, _)| c.as_str())
                .unwrap_or("Figure");

            let safe_caption = super::super::modules::latex::latex_escape(caption);

            output.push("\\begin{figure}[htbp]".to_string());
            output.push("\\centering".to_string());
            output.push(line.to_string());
            output.push(format!("\\caption{{{}}}", safe_caption));
            output.push(format!("\\label{{{}}}", label));
            output.push("\\end{figure}".to_string());
        } else {
            output.push(line.to_string());
        }
    }

    output.join("\n")
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
    // Apply light sanitization that preserves document structure
    // (sanitize_latex strips \documentclass which breaks full-document fixes)
    Ok(sanitize_latex_preserve_structure(&extracted))
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
// Compile Log QA
// ---------------------------------------------------------------------------

/// Analyse a LaTeX compile log and .toc file for quality issues.
/// Returns (warnings, has_critical_issues).
pub(crate) fn analyse_compile_log(log_path: &Path, toc_path: &Path) -> (Vec<String>, bool) {
    let mut warnings = Vec::new();
    let mut critical = false;

    // Analyse .log file
    if let Ok(log) = std::fs::read_to_string(log_path) {
        let unresolved_refs = log.lines()
            .filter(|l| l.contains("LaTeX Warning: Reference"))
            .count();
        if unresolved_refs > 0 {
            warnings.push(format!("{} unresolved reference(s)", unresolved_refs));
            if unresolved_refs > 3 {
                critical = true;
            }
        }

        let overfull = log.lines()
            .filter(|l| l.contains("Overfull \\hbox"))
            .count();
        if overfull > 5 {
            warnings.push(format!("{} overfull hbox warnings", overfull));
        }
    }

    // Check .toc file
    if toc_path.exists() {
        match std::fs::read_to_string(toc_path) {
            Ok(toc) if toc.trim().is_empty() => {
                warnings.push("Table of Contents is empty".into());
                critical = true;
            }
            Ok(_) => {} // toc present and non-empty
            Err(_) => {
                warnings.push("Could not read .toc file".into());
            }
        }
    } else {
        warnings.push("No .toc file generated".into());
    }

    (warnings, critical)
}

// ---------------------------------------------------------------------------
// LaTeX Lint + Auto-Fix
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LintSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct LintError {
    pub severity: LintSeverity,
    pub kind: &'static str,
    pub detail: String,
    pub line: Option<usize>,
}

/// Lint a LaTeX document for common quality issues.
pub(crate) fn lint_latex(tex: &str) -> Vec<LintError> {
    let mut errors = Vec::new();

    let mut chapter_titles: Vec<String> = Vec::new();
    let mut caption_texts: Vec<(String, usize)> = Vec::new();

    for (line_num, line) in tex.lines().enumerate() {
        let ln = Some(line_num + 1);

        // Check for unresolved references: "Figure ??", "Table ??", "Chapter ??"
        if line.contains("Figure ??") || line.contains("Table ??") || line.contains("Chapter ??")
            || line.contains("Section ??")
        {
            errors.push(LintError {
                severity: LintSeverity::Error,
                kind: "unresolved_ref",
                detail: format!("Unresolved reference on line {}", line_num + 1),
                line: ln,
            });
        }

        // Check for duplicate \chapter{Title}
        if let Some(start) = line.find("\\chapter{") {
            let rest = &line[start + 9..];
            if let Some(end) = rest.find('}') {
                let title = rest[..end].to_string();
                if chapter_titles.contains(&title) {
                    errors.push(LintError {
                        severity: LintSeverity::Warning,
                        kind: "duplicate_chapter",
                        detail: format!("Duplicate chapter title: '{}'", title),
                        line: ln,
                    });
                } else {
                    chapter_titles.push(title);
                }
            }
        }

        // Check for bare URLs not in \url{} or \href{}
        if (line.contains("http://") || line.contains("https://")) {
            // Simple heuristic: URL not preceded by \url{ or \href{
            let has_bare_url = {
                let mut found = false;
                for proto in &["http://", "https://"] {
                    if let Some(pos) = line.find(proto) {
                        let before = if pos >= 5 { &line[pos.saturating_sub(10)..pos] } else { &line[..pos] };
                        if !before.contains("\\url{") && !before.contains("\\href{") && !before.contains("url=") {
                            found = true;
                            break;
                        }
                    }
                }
                found
            };
            if has_bare_url {
                errors.push(LintError {
                    severity: LintSeverity::Warning,
                    kind: "bare_url",
                    detail: format!("Bare URL on line {}", line_num + 1),
                    line: ln,
                });
            }
        }

        // Check for overly wide tabulars (>6 columns)
        if line.contains("\\begin{tabular}") || line.contains("\\begin{tabularx}") {
            let tag = if line.contains("\\begin{tabularx}") { "\\begin{tabularx}" } else { "\\begin{tabular}{" };
            if let Some(start) = line.find(tag) {
                // For tabularx, skip the width argument: \begin{tabularx}{\textwidth}{...}
                let after_tag = &line[start + tag.len()..];
                let rest = if line.contains("\\begin{tabularx}") {
                    // Skip to second { for the column spec
                    if let Some(brace) = after_tag.find("}{") {
                        &after_tag[brace + 2..]
                    } else {
                        after_tag
                    }
                } else {
                    after_tag
                };
                if let Some(end) = rest.find('}') {
                    let col_spec = &rest[..end];
                    let col_count = col_spec
                        .chars()
                        .filter(|c| matches!(c, 'l' | 'c' | 'r' | 'p' | 'X'))
                        .count();
                    if col_count > 6 {
                        errors.push(LintError {
                            severity: LintSeverity::Warning,
                            kind: "wide_tabular",
                            detail: format!("Tabular with {} columns (>6) on line {}", col_count, line_num + 1),
                            line: ln,
                        });
                    }
                }
            }
        }

        // Track \caption{...} texts for duplicate detection
        if let Some(start) = line.find("\\caption{") {
            let rest = &line[start + 9..];
            if let Some(end) = rest.find('}') {
                let cap_text = rest[..end].to_string();
                caption_texts.push((cap_text, line_num + 1));
            }
        }

        // Prompt residue: caption that looks like a search query (>80 chars ending with a year)
        if let Some(start) = line.find("\\caption{") {
            let rest = &line[start + 9..];
            if let Some(end) = rest.find('}') {
                let cap = &rest[..end];
                let year_suffix = regex::Regex::new(r"\b(20\d{2}|19\d{2})\s*$").unwrap();
                if cap.len() > 80 && year_suffix.is_match(cap) {
                    errors.push(LintError {
                        severity: LintSeverity::Warning,
                        kind: "prompt_residue_caption",
                        detail: format!("Possible prompt residue in caption on line {}: '{}...'", line_num + 1, &cap[..60]),
                        line: ln,
                    });
                }
            }
        }
    }

    // Check for duplicate captions across the entire document
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (text, line) in &caption_texts {
        let normalized = text.trim().to_ascii_lowercase();
        if let Some(&first_line) = seen.get(&normalized) {
            errors.push(LintError {
                severity: LintSeverity::Warning,
                kind: "duplicate_caption",
                detail: format!("Duplicate caption '{}' on line {} (first on line {})", text, line, first_line),
                line: Some(*line),
            });
        } else {
            seen.insert(normalized, *line);
        }
    }

    errors
}

/// Auto-fix lint errors where possible. Currently wraps bare URLs in \url{}.
pub(crate) fn auto_fix_lint(tex: &str, errors: &[LintError]) -> String {
    let has_bare_urls = errors.iter().any(|e| e.kind == "bare_url");
    let has_unresolved = errors.iter().any(|e| e.kind == "unresolved_ref");
    let has_dup_captions = errors.iter().any(|e| e.kind == "duplicate_caption");

    if !has_bare_urls && !has_unresolved && !has_dup_captions {
        return tex.to_string();
    }

    // Fix duplicate captions by appending sequential letters
    let mut tex = tex.to_string();
    if has_dup_captions {
        let mut caption_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let cap_re = regex::Regex::new(r"\\caption\{([^}]+)\}").unwrap();
        tex = cap_re.replace_all(&tex, |caps: &regex::Captures| {
            let text = caps.get(1).unwrap().as_str();
            let key = text.trim().to_ascii_lowercase();
            let count = caption_counts.entry(key).or_insert(0);
            *count += 1;
            if *count > 1 {
                let suffix = (b'a' + (*count - 1) as u8) as char;
                format!("\\caption{{{} ({})}}", text, suffix)
            } else {
                caps[0].to_string()
            }
        }).to_string();
    }

    // Fix "Figure ??", "Table ??", etc. by removing the unresolved reference
    if has_unresolved {
        let ref_re = regex::Regex::new(r"(Figure|Table|Chapter|Section)\s+\?\?").unwrap();
        tex = ref_re.replace_all(&tex, |caps: &regex::Captures| {
            let kind = caps.get(1).unwrap().as_str();
            format!("the {}", kind.to_lowercase())
        }).to_string();
    }

    if !has_bare_urls {
        return tex;
    }

    let url_re = regex::Regex::new(r"(https?://[^\s\}\)>,]+)").unwrap();
    let mut result = String::with_capacity(tex.len() + 64);

    for line in tex.lines() {
        if (line.contains("http://") || line.contains("https://"))
            && !line.contains("\\url{")
            && !line.contains("\\href{")
        {
            // Replace URLs not already wrapped
            let mut new_line = String::new();
            let mut last = 0;
            for m in url_re.find_iter(line) {
                let before = &line[..m.start()];
                // Check if preceded by \url{ or \href{
                let already_wrapped = before.ends_with("\\url{") || before.ends_with("\\href{");
                new_line.push_str(&line[last..m.start()]);
                if already_wrapped {
                    new_line.push_str(m.as_str());
                } else {
                    new_line.push_str(&format!("\\url{{{}}}", m.as_str()));
                }
                last = m.end();
            }
            new_line.push_str(&line[last..]);
            result.push_str(&new_line);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing extra newline if original didn't have one
    if !tex.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

// ---------------------------------------------------------------------------
// Magazine HTML generator
// ---------------------------------------------------------------------------

fn generate_magazine_html(
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    let s = style.cloned().unwrap_or_default();
    let title = html_escape(objective_desc);
    let primary = &s.primary_color;
    let accent = &s.accent_color;

    let font_stack = match s.font_family.as_str() {
        "sans-serif" => "'Helvetica Neue', Arial, sans-serif",
        "monospace" => "'JetBrains Mono', 'Fira Code', monospace",
        _ => "'Georgia', 'Palatino', serif",
    };

    // Build sections with magazine layout
    let mut sections = String::new();
    for (i, (desc, text)) in task_results.iter().enumerate() {
        let first_char = text.chars().next().unwrap_or('T');
        let rest = if text.len() > 1 { &text[first_char.len_utf8()..] } else { "" };
        let drop_cap = if i == 0 || s.use_drop_caps {
            format!(
                "<span class=\"drop-cap\">{}</span>{}",
                html_escape(&first_char.to_string()),
                text_to_html(rest),
            )
        } else {
            text_to_html(text)
        };

        // Pull quote from first sentence
        let pull_quote = text.split('.').next().unwrap_or("").trim();
        let pull_quote_html = if !pull_quote.is_empty() && pull_quote.len() > 20 && pull_quote.len() < 200 {
            format!(
                "<blockquote class=\"pull-quote\">{}</blockquote>",
                html_escape(pull_quote),
            )
        } else {
            String::new()
        };

        sections.push_str(&format!(
            "<section class=\"magazine-section\">\n\
             <h2>{num}. {heading}</h2>\n\
             {pull_quote}\n\
             <div class=\"content\">{body}</div>\n\
             </section>\n",
            num = i + 1,
            heading = html_escape(desc),
            pull_quote = pull_quote_html,
            body = drop_cap,
        ));
    }

    // Image gallery
    let mut gallery = String::new();
    let mut credits: Vec<String> = Vec::new();
    for (caption, path, attribution) in images {
        if let Some(filename) = path.file_name() {
            if let Some(attr) = attribution {
                credits.push(format!(
                    "<li><strong>{}</strong> — {}</li>",
                    html_escape(caption), html_escape(attr),
                ));
            }
            gallery.push_str(&format!(
                "<figure class=\"gallery-item\">\
                 <img src=\"{}\" alt=\"{}\">\
                 <figcaption>{}</figcaption>\
                 </figure>\n",
                filename.to_string_lossy(),
                html_escape(caption),
                html_escape(caption),
            ));
        }
    }

    let gallery_section = if gallery.is_empty() {
        String::new()
    } else {
        format!("<section class=\"gallery\"><h2>Gallery</h2><div class=\"gallery-grid\">{}</div></section>", gallery)
    };

    let credits_section = if credits.is_empty() {
        String::new()
    } else {
        format!(
            "<section class=\"credits\"><h2>Photo Credits</h2><ul>{}</ul></section>",
            credits.join("\n"),
        )
    };

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
:root {{ --primary: {primary}; --accent: {accent}; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: {font_stack}; color: #222; background: #fafafa; line-height: 1.7; }}

/* Hero header */
.hero {{
  background: linear-gradient(135deg, {primary} 0%, {accent} 100%);
  color: #fff; padding: 4rem 2rem; text-align: center;
  margin-bottom: 3rem;
}}
.hero h1 {{ font-size: 2.8rem; font-weight: 700; margin-bottom: 0.5rem; letter-spacing: -0.02em; }}
.hero .subtitle {{ font-size: 1.1rem; opacity: 0.85; }}

/* Multi-column layout */
.magazine-body {{ max-width: 1100px; margin: 0 auto; padding: 0 2rem; }}
.magazine-section {{ margin-bottom: 3rem; break-inside: avoid; }}
.magazine-section h2 {{ color: var(--accent); font-size: 1.6rem; margin-bottom: 1rem; padding-bottom: 0.4rem; border-bottom: 2px solid var(--accent); }}
.magazine-section .content {{ columns: 2; column-gap: 2.5rem; text-align: justify; }}

/* Drop cap */
.drop-cap {{ float: left; font-size: 3.5rem; line-height: 0.8; padding: 0.1em 0.1em 0 0; color: var(--accent); font-weight: 700; }}

/* Pull quotes */
.pull-quote {{
  float: right; width: 40%; margin: 0 0 1rem 1.5rem; padding: 1rem 1.5rem;
  border-left: 4px solid var(--accent); font-size: 1.2rem; font-style: italic;
  color: var(--primary); background: rgba(0,0,0,0.02); line-height: 1.5;
}}

/* Gallery */
.gallery {{ margin: 3rem 0; }}
.gallery h2 {{ text-align: center; color: var(--accent); margin-bottom: 1.5rem; }}
.gallery-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 1.5rem; }}
.gallery-item {{ text-align: center; }}
.gallery-item img {{ width: 100%; height: auto; border-radius: 6px; box-shadow: 0 2px 10px rgba(0,0,0,0.12); }}
.gallery-item figcaption {{ font-style: italic; color: #666; margin-top: 0.5rem; font-size: 0.9rem; }}

/* Credits */
.credits {{ margin: 3rem 0; padding: 1.5rem; background: #f0f0f0; border-radius: 8px; }}
.credits h2 {{ margin-bottom: 1rem; color: var(--primary); }}

/* Print styles */
@media print {{
  .hero {{ background: none; color: #000; border-bottom: 3px solid #000; }}
  .magazine-section .content {{ columns: 1; }}
  .pull-quote {{ float: none; width: 100%; margin: 1rem 0; }}
}}
@media (max-width: 768px) {{
  .magazine-section .content {{ columns: 1; }}
  .pull-quote {{ float: none; width: 100%; margin: 1rem 0; }}
  .hero h1 {{ font-size: 2rem; }}
}}
</style>
</head>
<body>
<header class="hero">
<h1>{title}</h1>
<p class="subtitle">A comprehensive exploration</p>
</header>
<main class="magazine-body">
{sections}
{gallery}
{credits}
</main>
</body>
</html>"##,
        title = title,
        primary = primary,
        accent = accent,
        font_stack = font_stack,
        sections = sections,
        gallery = gallery_section,
        credits = credits_section,
    );

    let html_path = output_dir.join("output.html");
    std::fs::write(&html_path, &html)
        .map_err(|e| NyayaError::Config(format!("Failed to write magazine HTML: {}", e)))?;
    Ok(html_path)
}

// ---------------------------------------------------------------------------
// Blog HTML generator
// ---------------------------------------------------------------------------

fn generate_blog_html(
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    let s = style.cloned().unwrap_or_default();
    let title = html_escape(objective_desc);
    let primary = &s.primary_color;
    let accent = &s.accent_color;

    let font_stack = match s.font_family.as_str() {
        "sans-serif" => "'Helvetica Neue', Arial, sans-serif",
        "monospace" => "'JetBrains Mono', 'Fira Code', monospace",
        _ => "'Georgia', 'Palatino', serif",
    };

    // Estimate reading time (~200 words per minute)
    let total_words: usize = task_results.iter().map(|(_, t)| t.split_whitespace().count()).sum();
    let reading_minutes = (total_words / 200).max(1);

    // Build TOC
    let mut toc = String::new();
    for (i, (desc, _)) in task_results.iter().enumerate() {
        toc.push_str(&format!(
            "<li><a href=\"#section-{}\">{}</a></li>\n",
            i + 1,
            html_escape(desc),
        ));
    }

    // Build sections
    let mut sections = String::new();
    for (i, (desc, text)) in task_results.iter().enumerate() {
        let mut image_html = String::new();
        for (caption, path, attribution) in images {
            if let Some(filename) = path.file_name() {
                let cap_lower = caption.to_lowercase();
                let desc_lower = desc.to_lowercase();
                if cap_lower.contains(&desc_lower) || desc_lower.contains(&cap_lower) {
                    let attr_html = attribution.as_ref()
                        .map(|a| format!(" <span class=\"attr\">{}</span>", html_escape(a)))
                        .unwrap_or_default();
                    image_html.push_str(&format!(
                        "<figure><img src=\"{}\" alt=\"{}\">\
                         <figcaption>{}{}</figcaption></figure>\n",
                        filename.to_string_lossy(),
                        html_escape(caption),
                        html_escape(caption),
                        attr_html,
                    ));
                }
            }
        }

        sections.push_str(&format!(
            "<section id=\"section-{num}\">\n\
             <h2>{heading}</h2>\n\
             {images}\
             <div class=\"content\">{body}</div>\n\
             </section>\n",
            num = i + 1,
            heading = html_escape(desc),
            images = image_html,
            body = text_to_html(text),
        ));
    }

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<meta property="og:title" content="{title}">
<meta property="og:description" content="{og_desc}">
<meta property="og:type" content="article">
<style>
:root {{ --primary: {primary}; --accent: {accent}; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: {font_stack}; color: #333; background: #fff; line-height: 1.8; }}

.blog-container {{ display: flex; max-width: 1000px; margin: 0 auto; padding: 2rem 1rem; gap: 2rem; }}

/* Sidebar TOC */
.toc-sidebar {{
  position: sticky; top: 2rem; align-self: flex-start;
  width: 220px; flex-shrink: 0; padding: 1rem;
  border-right: 1px solid #eee; font-size: 0.85rem;
}}
.toc-sidebar h3 {{ color: var(--accent); margin-bottom: 0.8rem; font-size: 0.95rem; text-transform: uppercase; letter-spacing: 0.05em; }}
.toc-sidebar ol {{ padding-left: 1.2em; }}
.toc-sidebar li {{ margin-bottom: 0.4rem; }}
.toc-sidebar a {{ color: #555; text-decoration: none; }}
.toc-sidebar a:hover {{ color: var(--accent); }}

/* Main content */
.blog-main {{ flex: 1; max-width: 720px; }}

.blog-header {{ margin-bottom: 2.5rem; padding-bottom: 1.5rem; border-bottom: 1px solid #eee; }}
.blog-header h1 {{ font-size: 2.2rem; color: var(--primary); margin-bottom: 0.5rem; line-height: 1.3; }}
.blog-meta {{ color: #888; font-size: 0.9rem; }}

section {{ margin-bottom: 2.5rem; }}
section h2 {{ color: var(--accent); font-size: 1.4rem; margin-bottom: 0.8rem; padding-bottom: 0.3rem; border-bottom: 1px solid #f0f0f0; }}
.content {{ margin: 1em 0; }}
.content p {{ margin-bottom: 1em; }}

/* Images */
figure {{ margin: 1.5rem 0; text-align: center; }}
figure img {{ max-width: 100%; height: auto; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }}
figcaption {{ font-style: italic; color: #666; margin-top: 0.5em; font-size: 0.9rem; }}
figcaption .attr {{ font-size: 0.8em; color: #999; display: block; }}

/* Blockquote */
blockquote {{ margin: 1.5rem 0; padding: 1rem 1.5rem; border-left: 4px solid var(--accent); background: #f9f9f9; font-style: italic; color: #555; }}

/* Code blocks */
pre {{ background: #f5f5f5; padding: 1rem; border-radius: 6px; overflow-x: auto; font-size: 0.9rem; margin: 1rem 0; }}
code {{ font-family: 'JetBrains Mono', 'Fira Code', monospace; font-size: 0.9em; }}

@media (max-width: 768px) {{
  .blog-container {{ flex-direction: column; }}
  .toc-sidebar {{ position: static; width: 100%; border-right: none; border-bottom: 1px solid #eee; padding-bottom: 1rem; margin-bottom: 1rem; }}
}}
@media print {{
  .toc-sidebar {{ display: none; }}
}}
</style>
</head>
<body>
<div class="blog-container">
<nav class="toc-sidebar">
<h3>Contents</h3>
<ol>
{toc}
</ol>
</nav>
<main class="blog-main">
<header class="blog-header">
<h1>{title}</h1>
<p class="blog-meta">{reading_time} min read</p>
</header>
{sections}
</main>
</div>
</body>
</html>"##,
        title = title,
        og_desc = html_escape(&objective_desc.chars().take(160).collect::<String>()),
        primary = primary,
        accent = accent,
        font_stack = font_stack,
        toc = toc,
        reading_time = reading_minutes,
        sections = sections,
    );

    let html_path = output_dir.join("output.html");
    std::fs::write(&html_path, &html)
        .map_err(|e| NyayaError::Config(format!("Failed to write blog HTML: {}", e)))?;
    Ok(html_path)
}

// ---------------------------------------------------------------------------
// Video generator (Remotion → ffmpeg → HTML fallback)
// ---------------------------------------------------------------------------

/// Measure audio duration in seconds using ffprobe.
fn measure_audio_duration_ffprobe(path: &Path) -> Option<f32> {
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<f32>().ok()
    } else {
        None
    }
}

/// Check if a section title looks like a references/bibliography section.
// ---------------------------------------------------------------------------
// Documentary-style slide content model
// ---------------------------------------------------------------------------

/// Rich slide content for documentary-style video output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SlideContent {
    Title {
        title: String,
        subtitle: String,
        #[serde(rename = "durationFrames", default = "default_duration_title")]
        duration_frames: u32,
    },
    Content {
        title: String,
        bullets: Vec<String>,
        #[serde(default)]
        footnotes: Vec<String>,
        #[serde(rename = "durationFrames", default = "default_duration_content")]
        duration_frames: u32,
    },
    Timeline {
        title: String,
        events: Vec<TimelineEvent>,
        #[serde(rename = "durationFrames", default = "default_duration_timeline")]
        duration_frames: u32,
    },
    Stats {
        title: String,
        stats: Vec<StatEntry>,
        #[serde(rename = "durationFrames", default = "default_duration_content")]
        duration_frames: u32,
    },
    Quote {
        text: String,
        attribution: String,
        #[serde(rename = "durationFrames", default = "default_duration_quote")]
        duration_frames: u32,
    },
    Image {
        caption: String,
        filename: String,
        #[serde(default)]
        attribution: String,
        #[serde(rename = "durationFrames", default = "default_duration_quote")]
        duration_frames: u32,
    },
    Closing {
        title: String,
        subtitle: String,
        #[serde(rename = "durationFrames", default = "default_duration_content")]
        duration_frames: u32,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineEvent {
    pub date: String,
    pub desc: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatEntry {
    pub label: String,
    pub value: String,
}

fn default_duration_title() -> u32 { 210 }
fn default_duration_content() -> u32 { 180 }
fn default_duration_timeline() -> u32 { 240 }
fn default_duration_quote() -> u32 { 150 }

impl SlideContent {
    #[allow(dead_code)]
    fn duration_frames(&self) -> u32 {
        match self {
            SlideContent::Title { duration_frames, .. }
            | SlideContent::Content { duration_frames, .. }
            | SlideContent::Timeline { duration_frames, .. }
            | SlideContent::Stats { duration_frames, .. }
            | SlideContent::Quote { duration_frames, .. }
            | SlideContent::Image { duration_frames, .. }
            | SlideContent::Closing { duration_frames, .. } => *duration_frames,
        }
    }

    #[cfg(test)]
    fn kind_str(&self) -> &'static str {
        match self {
            SlideContent::Title { .. } => "title",
            SlideContent::Content { .. } => "content",
            SlideContent::Timeline { .. } => "timeline",
            SlideContent::Stats { .. } => "stats",
            SlideContent::Quote { .. } => "quote",
            SlideContent::Image { .. } => "image",
            SlideContent::Closing { .. } => "closing",
        }
    }
}

// ── PixiJS Motion Graphics Scene Types ──────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CounterEntry {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BarEntry {
    pub label: String,
    pub value: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WaypointEntry {
    pub year: String,
    pub label: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComparisonPoint {
    pub left: String,
    pub right: String,
}

fn default_opener_duration() -> u32 { 240 }
fn default_kinetic_duration() -> u32 { 180 }
fn default_counter_duration() -> u32 { 210 }
fn default_bar_race_duration() -> u32 { 210 }
fn default_particle_duration() -> u32 { 150 }
fn default_timeline_path_duration() -> u32 { 270 }
fn default_comparison_duration() -> u32 { 240 }
fn default_closing_motion_duration() -> u32 { 210 }
fn default_layout() -> String { "cascade".to_string() }
fn default_preset() -> String { "stars".to_string() }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VideoScene {
    Opener {
        title: String,
        #[serde(default)]
        subtitle: String,
        #[serde(default)]
        mood: String,
        #[serde(rename = "durationFrames", default = "default_opener_duration")]
        duration_frames: u32,
    },
    KineticText {
        text: String,
        #[serde(default = "default_layout")]
        layout: String,
        #[serde(rename = "durationFrames", default = "default_kinetic_duration")]
        duration_frames: u32,
    },
    DataCounter {
        title: String,
        counters: Vec<CounterEntry>,
        #[serde(rename = "durationFrames", default = "default_counter_duration")]
        duration_frames: u32,
    },
    BarRace {
        title: String,
        bars: Vec<BarEntry>,
        #[serde(rename = "durationFrames", default = "default_bar_race_duration")]
        duration_frames: u32,
    },
    ParticleMood {
        text: String,
        #[serde(default = "default_preset")]
        preset: String,
        #[serde(rename = "durationFrames", default = "default_particle_duration")]
        duration_frames: u32,
    },
    TimelinePath {
        title: String,
        waypoints: Vec<WaypointEntry>,
        #[serde(rename = "durationFrames", default = "default_timeline_path_duration")]
        duration_frames: u32,
    },
    ComparisonSplit {
        title: String,
        #[serde(default)]
        left_label: String,
        #[serde(default)]
        right_label: String,
        points: Vec<ComparisonPoint>,
        #[serde(rename = "durationFrames", default = "default_comparison_duration")]
        duration_frames: u32,
    },
    Closing {
        title: String,
        #[serde(default)]
        subtitle: String,
        #[serde(rename = "durationFrames", default = "default_closing_motion_duration")]
        duration_frames: u32,
    },
}

impl VideoScene {
    pub fn duration_frames(&self) -> u32 {
        match self {
            VideoScene::Opener { duration_frames, .. }
            | VideoScene::KineticText { duration_frames, .. }
            | VideoScene::DataCounter { duration_frames, .. }
            | VideoScene::BarRace { duration_frames, .. }
            | VideoScene::ParticleMood { duration_frames, .. }
            | VideoScene::TimelinePath { duration_frames, .. }
            | VideoScene::ComparisonSplit { duration_frames, .. }
            | VideoScene::Closing { duration_frames, .. } => *duration_frames,
        }
    }

    #[cfg(test)]
    fn kind_str(&self) -> &'static str {
        match self {
            VideoScene::Opener { .. } => "opener",
            VideoScene::KineticText { .. } => "kineticText",
            VideoScene::DataCounter { .. } => "dataCounter",
            VideoScene::BarRace { .. } => "barRace",
            VideoScene::ParticleMood { .. } => "particleMood",
            VideoScene::TimelinePath { .. } => "timelinePath",
            VideoScene::ComparisonSplit { .. } => "comparisonSplit",
            VideoScene::Closing { .. } => "closing",
        }
    }

    fn duration_frames_mut(&mut self) -> &mut u32 {
        match self {
            VideoScene::Opener { duration_frames, .. }
            | VideoScene::KineticText { duration_frames, .. }
            | VideoScene::DataCounter { duration_frames, .. }
            | VideoScene::BarRace { duration_frames, .. }
            | VideoScene::ParticleMood { duration_frames, .. }
            | VideoScene::TimelinePath { duration_frames, .. }
            | VideoScene::ComparisonSplit { duration_frames, .. }
            | VideoScene::Closing { duration_frames, .. } => duration_frames,
        }
    }
}

/// Validate and fix video scenes: ensure Opener first, Closing last, clamp counts/durations,
/// fix invalid layout/preset values.
pub fn validate_video_scenes(scenes: &mut Vec<VideoScene>, title: &str) {
    // Fix invalid layout values
    let valid_layouts = ["cascade", "converge", "wave", "typewriter"];
    let valid_presets = ["stars", "rain", "fireflies", "snow", "nebula"];

    for scene in scenes.iter_mut() {
        match scene {
            VideoScene::KineticText { layout, .. } => {
                if !valid_layouts.contains(&layout.as_str()) {
                    *layout = "cascade".to_string();
                }
            }
            VideoScene::ParticleMood { preset, .. } => {
                if !valid_presets.contains(&preset.as_str()) {
                    *preset = "stars".to_string();
                }
            }
            _ => {}
        }
    }

    // Ensure Opener first
    if !matches!(scenes.first(), Some(VideoScene::Opener { .. })) {
        scenes.insert(0, VideoScene::Opener {
            title: title.to_string(),
            subtitle: String::new(),
            mood: "dramatic".to_string(),
            duration_frames: 240,
        });
    }

    // Ensure Closing last
    if !matches!(scenes.last(), Some(VideoScene::Closing { .. })) {
        scenes.push(VideoScene::Closing {
            title: "Thank You".to_string(),
            subtitle: "Generated by NabaOS PEA".to_string(),
            duration_frames: 210,
        });
    }

    // Clamp to 8-20 scenes
    if scenes.len() > 20 {
        let closing = scenes.pop().unwrap();
        scenes.truncate(19);
        scenes.push(closing);
    }

    // Clamp duration_frames to 90-360
    for scene in scenes.iter_mut() {
        let df = scene.duration_frames_mut();
        *df = (*df).clamp(90, 360);
    }
}

/// Extract timeline events: scan for year patterns followed by descriptions.
fn extract_timeline_events(text: &str) -> Vec<(String, String)> {
    let mut events: Vec<(String, String)> = Vec::new();
    // Match patterns like "1979:", "In 2024,", "March 2026", "1979 -", "1979–"
    let year_re = regex::Regex::new(
        r"(?:(?:in\s+|since\s+)?(?:(?:January|February|March|April|May|June|July|August|September|October|November|December)\s+)?(\d{4}))\s*[-–:,.]?\s*(.{10,})"
    ).unwrap();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        // Skip markdown table rows, header separators, and list markers
        if line.starts_with('|') || line.starts_with("|-") || line.starts_with("| -") {
            continue;
        }
        if let Some(caps) = year_re.captures(line) {
            let year = caps.get(1).unwrap().as_str().to_string();
            let year_num: u32 = year.parse().unwrap_or(0);
            if !(1800..=2100).contains(&year_num) { continue; }
            let desc_raw = caps.get(2).unwrap().as_str().trim().to_string();
            // Strip markdown formatting from description
            let desc = strip_markdown(&desc_raw);
            // Clean up description — trim to first sentence or 120 chars
            let desc = if let Some(dot_pos) = desc.find(". ") {
                if dot_pos > 20 {
                    format!("{}.", &desc[..dot_pos])
                } else {
                    desc[..desc.len().min(120)].to_string()
                }
            } else {
                desc[..desc.len().min(120)].to_string()
            };
            // Skip descriptions that are too short or look like noise
            if desc.len() < 15 || looks_like_slide_noise(&desc) {
                continue;
            }
            // Skip descriptions that look like year ranges (e.g., "2026; assess...")
            if desc.starts_with(|c: char| c.is_ascii_digit()) && desc.len() < 30 {
                continue;
            }
            events.push((year, desc));
        }
    }
    // Deduplicate by year (keep first occurrence)
    let mut seen = std::collections::HashSet::new();
    events.retain(|(year, _)| seen.insert(year.clone()));
    events.truncate(8);
    events
}

/// Extract key statistics: scan for number patterns with context.
fn extract_key_stats(text: &str) -> Vec<(String, String)> {
    let mut stats: Vec<(String, String)> = Vec::new();
    // Match: $X billion, X%, X,XXX casualties, X million, etc.
    let stat_re = regex::Regex::new(
        r"(\$[\d,.]+\s*(?:billion|million|trillion|B|M|T)|\d[\d,.]*\s*%|\d[\d,.]+\s+(?:casualties|people|deaths|refugees|troops|soldiers|civilians|hectares|tons|kilometers|miles|units|workers|jobs|users|patients|students|billion|million|thousand))"
    ).unwrap();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || looks_like_slide_noise(line) { continue; }
        // Skip markdown table rows
        if line.starts_with('|') || line.starts_with("|-") {
            continue;
        }
        let clean_line = strip_markdown(line);
        for mat in stat_re.find_iter(&clean_line) {
            let value = mat.as_str().trim().to_string();
            // Try to extract a short label from surrounding context
            let before = &clean_line[..mat.start()];
            // Take the last clause before the number
            let label = before
                .rsplit(|c: char| c == '.' || c == ';')
                .next()
                .unwrap_or(before)
                .trim()
                .to_string();
            // Clean up the label: remove trailing commas, "with", "over", etc.
            let label = label
                .trim_end_matches(|c: char| c == ',' || c == ':')
                .trim()
                .to_string();
            let label = if label.len() > 50 {
                // Take last meaningful phrase
                label.rsplitn(2, ", ").next().unwrap_or(&label).trim().to_string()
            } else if label.is_empty() || label.len() < 3 {
                // Use text after the stat as label if before is empty
                let after = &clean_line[mat.end()..];
                let after_label = after.trim_start_matches(|c: char| c == ' ' || c == ',');
                let after_label = after_label.split(|c: char| c == '.' || c == ',' || c == ';')
                    .next()
                    .unwrap_or("")
                    .trim();
                if after_label.len() >= 3 && after_label.len() <= 40 {
                    after_label.to_string()
                } else {
                    continue; // Skip stats without meaningful labels
                }
            } else {
                label
            };
            stats.push((label, value));
        }
    }
    // Deduplicate by value
    let mut seen = std::collections::HashSet::new();
    stats.retain(|(_, v)| seen.insert(v.clone()));
    stats.truncate(6);
    stats
}

/// Extract the first meaningful quote from text.
fn extract_key_quote(text: &str) -> Option<(String, String)> {
    // Look for quoted text with attribution
    let quote_re = regex::Regex::new(
        r#"[""\u{201C}]([^""\u{201D}]{20,200})[""\u{201D}]\s*[-–—]?\s*(.{3,60})"#
    ).unwrap();

    for line in text.lines() {
        if let Some(caps) = quote_re.captures(line) {
            let quote_text = caps.get(1).unwrap().as_str().trim().to_string();
            let attribution = caps.get(2).unwrap().as_str().trim().to_string();
            // Clean attribution: strip trailing punctuation
            let attribution = attribution.trim_end_matches(|c: char| c == '.' || c == ',').to_string();
            if !quote_text.is_empty() && !attribution.is_empty() {
                return Some((quote_text, attribution));
            }
        }
    }
    None
}

/// Extract inline citations like (Author, YYYY) or Author (YYYY) or Author et al. (YYYY).
fn extract_inline_citations(text: &str) -> Vec<String> {
    let mut citations: Vec<String> = Vec::new();
    // Pattern 1: (Author, YYYY) or (Author et al., YYYY)
    let paren_re = regex::Regex::new(
        r"\(([A-Z][a-z]+(?:\s+(?:et\s+al\.?|&\s+[A-Z][a-z]+))?),?\s*((?:19|20)\d{2})\)"
    ).unwrap();
    // Pattern 2: Author (YYYY)
    let inline_re = regex::Regex::new(
        r"([A-Z][a-z]+(?:\s+(?:et\s+al\.?|&\s+[A-Z][a-z]+))?)\s+\(((?:19|20)\d{2})\)"
    ).unwrap();

    for caps in paren_re.captures_iter(text) {
        let cite = format!("{} ({})", caps.get(1).unwrap().as_str(), caps.get(2).unwrap().as_str());
        if !citations.contains(&cite) {
            citations.push(cite);
        }
    }
    for caps in inline_re.captures_iter(text) {
        let cite = format!("{} ({})", caps.get(1).unwrap().as_str(), caps.get(2).unwrap().as_str());
        if !citations.contains(&cite) {
            citations.push(cite);
        }
    }
    citations.truncate(3);
    citations
}

fn is_reference_section(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("reference") || lower.contains("bibliograph")
}

/// Return true if the string looks like slide noise (URLs, APA fragments, mostly punctuation).
fn looks_like_slide_noise(s: &str) -> bool {
    let s = s.trim();
    if s.contains("://") || s.contains("www.") {
        return true;
    }
    // APA / citation fragments
    let lower = s.to_lowercase();
    if lower.contains("doi:") || lower.contains("doi.org") || lower.contains("et al")
        || lower.contains("vol.") || lower.contains(", pp.") || lower.contains("isbn")
        || lower.contains("journal of") || lower.contains("retrieved from")
    {
        return true;
    }
    // Pattern: "Author (YYYY)" — typical citation start
    if s.contains("(20") || s.contains("(19") {
        // If it also has commas (author list), likely a citation
        if s.matches(',').count() >= 2 {
            return true;
        }
    }
    // Mostly digits / punctuation (>65% noise chars)
    if !s.is_empty() {
        let noise_chars = s.chars().filter(|c| c.is_ascii_digit() || c.is_ascii_punctuation() || c.is_whitespace()).count();
        if (noise_chars as f64 / s.len() as f64) > 0.65 {
            return true;
        }
    }
    false
}

/// Strip markdown formatting from text, returning plain content.
///
/// Handles: headers, bold, italic, code, links, images, strikethrough.
fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let trimmed = line.trim();
        // Strip header markers: "### Title" → "Title"
        let line_clean = if trimmed.starts_with('#') {
            trimmed.trim_start_matches('#').trim_start()
        } else {
            trimmed
        };
        // Process inline formatting character by character
        let chars: Vec<char> = line_clean.chars().collect();
        let len = chars.len();
        let mut i = 0;
        while i < len {
            // Image: ![alt](url) → remove entirely
            if chars[i] == '!' && i + 1 < len && chars[i + 1] == '[' {
                // Skip past ![alt](url)
                if let Some(close_bracket) = chars[i + 2..].iter().position(|&c| c == ']') {
                    let after_bracket = i + 2 + close_bracket + 1;
                    if after_bracket < len && chars[after_bracket] == '(' {
                        if let Some(close_paren) = chars[after_bracket + 1..].iter().position(|&c| c == ')') {
                            i = after_bracket + 1 + close_paren + 1;
                            continue;
                        }
                    }
                }
                out.push(chars[i]);
                i += 1;
            }
            // Link: [text](url) → text
            else if chars[i] == '[' {
                if let Some(close_bracket) = chars[i + 1..].iter().position(|&c| c == ']') {
                    let text_end = i + 1 + close_bracket;
                    let after_bracket = text_end + 1;
                    if after_bracket < len && chars[after_bracket] == '(' {
                        if let Some(close_paren) = chars[after_bracket + 1..].iter().position(|&c| c == ')') {
                            // Emit link text only
                            for &c in &chars[i + 1..text_end] {
                                out.push(c);
                            }
                            i = after_bracket + 1 + close_paren + 1;
                            continue;
                        }
                    }
                }
                out.push(chars[i]);
                i += 1;
            }
            // Bold: **text** or __text__
            else if i + 1 < len && ((chars[i] == '*' && chars[i + 1] == '*') || (chars[i] == '_' && chars[i + 1] == '_')) {
                let marker = chars[i];
                // Find closing **
                if let Some(close_pos) = chars[i + 2..].windows(2).position(|w| w[0] == marker && w[1] == marker) {
                    for &c in &chars[i + 2..i + 2 + close_pos] {
                        out.push(c);
                    }
                    i = i + 2 + close_pos + 2;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            // Strikethrough: ~~text~~
            else if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
                if let Some(close_pos) = chars[i + 2..].windows(2).position(|w| w[0] == '~' && w[1] == '~') {
                    for &c in &chars[i + 2..i + 2 + close_pos] {
                        out.push(c);
                    }
                    i = i + 2 + close_pos + 2;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            // Italic: *text* or _text_ (single, not double)
            else if (chars[i] == '*' || chars[i] == '_') && (i + 1 >= len || chars[i + 1] != chars[i]) {
                let marker = chars[i];
                if let Some(close_pos) = chars[i + 1..].iter().position(|&c| c == marker) {
                    for &c in &chars[i + 1..i + 1 + close_pos] {
                        out.push(c);
                    }
                    i = i + 1 + close_pos + 1;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            // Inline code: `code`
            else if chars[i] == '`' {
                if let Some(close_pos) = chars[i + 1..].iter().position(|&c| c == '`') {
                    for &c in &chars[i + 1..i + 1 + close_pos] {
                        out.push(c);
                    }
                    i = i + 1 + close_pos + 1;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out.push('\n');
    }
    // Trim trailing newline
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Extract clean bullet points from a block of text.
///
/// Split on newlines first, then sentence boundaries (`. [A-Z]`), filter for
/// reasonable length (15-140 chars), reject noise, strip numbered-list markers.
fn extract_slide_bullets(text: &str) -> Vec<String> {
    let text = &strip_markdown(text);
    let mut candidates: Vec<String> = Vec::new();

    // First split on newlines to get natural paragraphs/lines
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }

        // Skip markdown table rows and separators
        if line.starts_with('|') || line.contains(" | ") || line.starts_with("|-") {
            continue;
        }
        // If the whole line is noise, skip early
        if looks_like_slide_noise(line) { continue; }

        // Sentence-split on ". [A-Z]" boundaries
        let mut remaining = line;
        loop {
            match remaining.find(". ") {
                Some(pos) => {
                    let after = &remaining[pos + 2..];
                    if after.starts_with(|c: char| c.is_uppercase()) {
                        // Sentence boundary found
                        let sentence = remaining[..pos + 1].trim().to_string();
                        candidates.push(sentence);
                        remaining = after;
                    } else {
                        // Not a boundary, keep scanning past this ". "
                        if pos + 2 < remaining.len() {
                            // Look for next ". " from after this one
                            let search_from = pos + 2;
                            match remaining[search_from..].find(". ") {
                                Some(_) => {
                                    // Continue the loop — remaining still has more ". " to check
                                    // But we need to advance, so take the whole thing up to the
                                    // next actual boundary by continuing the loop
                                    // Skip this non-boundary by searching ahead
                                    let ahead = &remaining[search_from..];
                                    if let Some(next_pos) = ahead.find(". ") {
                                        let abs_pos = search_from + next_pos;
                                        let after_next = &remaining[abs_pos + 2..];
                                        if after_next.starts_with(|c: char| c.is_uppercase()) {
                                            let sentence = remaining[..abs_pos + 1].trim().to_string();
                                            candidates.push(sentence);
                                            remaining = after_next;
                                        } else {
                                            // Give up splitting this line
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        } else {
                            break;
                        }
                    }
                }
                None => break,
            }
        }
        if !remaining.trim().is_empty() {
            candidates.push(remaining.trim().to_string());
        }
    }

    candidates
        .into_iter()
        // Strip numbered-list markers like "1. ", "2) ", "- "
        .map(|s| {
            let s = s.trim().to_string();
            if let Some(rest) = s.strip_prefix("- ") {
                rest.to_string()
            } else if s.starts_with(|c: char| c.is_ascii_digit()) {
                // Strip "1. " or "1) " prefixes
                let re_stripped = s.trim_start_matches(|c: char| c.is_ascii_digit());
                if let Some(rest) = re_stripped.strip_prefix(". ").or_else(|| re_stripped.strip_prefix(") ")) {
                    rest.to_string()
                } else {
                    s
                }
            } else {
                s
            }
        })
        .filter(|s| s.len() >= 15 && s.len() <= 140)
        .filter(|s| !looks_like_slide_noise(s))
        .take(4)
        .collect()
}

/// Extract short APA-style source entries from reference text for a "Key Sources" slide.
#[allow(dead_code)]
fn extract_key_sources(ref_text: &str) -> Vec<String> {
    ref_text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| {
            // Must contain a year-like pattern
            l.contains("19") || l.contains("20")
        })
        .filter(|l| l.len() >= 15 && l.len() <= 100)
        .filter(|l| !l.starts_with("http") && !l.starts_with("doi"))
        .map(|l| if l.len() > 65 { format!("{}...", &l[..62]) } else { l })
        .take(5)
        .collect()
}

/// Build documentary-style slide data from task results.
///
/// Produces a varied sequence: Title → Content/Timeline/Stats slides → Quote → Closing.
/// Reference sections are folded into per-slide footnotes instead of a standalone slide.
pub(crate) fn build_slides(
    objective_desc: &str,
    task_results: &[(String, String)],
) -> Vec<SlideContent> {
    let mut slides: Vec<SlideContent> = Vec::new();

    // Extract first sentence of objective for subtitle
    let subtitle = objective_desc
        .find(". ")
        .map(|pos| objective_desc[..pos + 1].to_string())
        .unwrap_or_else(|| "A comprehensive exploration".to_string());

    // Title slide
    slides.push(SlideContent::Title {
        title: objective_desc.to_string(),
        subtitle,
        duration_frames: 210,
    });

    // Collect all reference text for citation extraction
    let mut all_ref_text = String::new();
    for (desc, text) in task_results.iter() {
        if is_reference_section(desc) {
            all_ref_text.push_str(text);
            all_ref_text.push('\n');
        }
    }

    // Track best quote across all sections
    let mut best_quote: Option<(String, String)> = None;

    // Content slides (one per section, skip references)
    for (desc, text) in task_results.iter().take(12) {
        if is_reference_section(desc) {
            continue;
        }

        // Extract bullets for content slide
        let bullets = extract_slide_bullets(text);
        if bullets.is_empty() {
            continue;
        }

        // Extract inline citations as footnotes
        let footnotes = extract_inline_citations(text);

        slides.push(SlideContent::Content {
            title: desc.clone(),
            bullets,
            footnotes,
            duration_frames: 180,
        });

        // After this content slide, try to add a timeline or stats slide
        let timeline_events = extract_timeline_events(text);
        if timeline_events.len() >= 3 {
            slides.push(SlideContent::Timeline {
                title: format!("{} — Timeline", desc),
                events: timeline_events
                    .into_iter()
                    .map(|(date, desc)| TimelineEvent { date, desc })
                    .collect(),
                duration_frames: 240,
            });
        }

        let stats = extract_key_stats(text);
        if stats.len() >= 2 {
            slides.push(SlideContent::Stats {
                title: format!("{} — Key Numbers", desc),
                stats: stats
                    .into_iter()
                    .map(|(label, value)| StatEntry { label, value })
                    .collect(),
                duration_frames: 180,
            });
        }

        // Capture best quote
        if best_quote.is_none() {
            best_quote = extract_key_quote(text);
        }
    }

    // Add quote slide before closing if found
    if let Some((text, attribution)) = best_quote {
        slides.push(SlideContent::Quote {
            text,
            attribution,
            duration_frames: 150,
        });
    }

    // Closing slide
    slides.push(SlideContent::Closing {
        title: "Thank You".into(),
        subtitle: "Generated by NabaOS PEA".into(),
        duration_frames: 180,
    });

    slides
}

fn generate_video(
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    let slides = build_slides(objective_desc, task_results);
    generate_video_from_slides(&slides, images, style, output_dir)
}

/// Render a pre-built slide deck to video (Remotion → ffmpeg → HTML fallback).
pub fn generate_video_from_slides(
    slides: &[SlideContent],
    images: &[ImageEntry],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    let s = style.cloned().unwrap_or_default();
    let primary = &s.primary_color;
    let accent = &s.accent_color;

    // -----------------------------------------------------------------------
    // Path 1: Remotion (highest quality — animated transitions, spring text)
    // Requires node + npm; remotion itself is installed per-project via npm install
    // -----------------------------------------------------------------------
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let has_npm = std::process::Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_node && has_npm {
        eprintln!("[pea/doc] Node.js + npm detected — generating Remotion video");
        match generate_remotion_video(&slides, images, primary, accent, output_dir) {
            Ok(path) => return Ok(path),
            Err(e) => {
                eprintln!("[pea/doc] Remotion render failed: {} — trying ffmpeg", e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Path 2: wkhtmltoimage + ffmpeg (static slides, crossfade)
    // -----------------------------------------------------------------------
    let slides_dir = output_dir.join("slides");
    std::fs::create_dir_all(&slides_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to create slides dir: {}", e)))?;

    // Write slide HTMLs — flatten SlideContent to title+bullets for static render
    for (i, slide) in slides.iter().enumerate() {
        let (slide_title, bullets) = slide_to_title_bullets(slide);
        let bullet_html: String = bullets
            .iter()
            .map(|b| format!("<li>{}</li>", html_escape(b)))
            .collect::<Vec<_>>()
            .join("\n");
        let slide_html = format!(
            r##"<!DOCTYPE html>
<html><head><meta charset="UTF-8">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ width: 1920px; height: 1080px; display: flex; flex-direction: column;
  justify-content: center; align-items: center; font-family: 'Helvetica Neue', Arial, sans-serif;
  background: linear-gradient(135deg, {primary} 0%, {accent} 100%); color: #fff; padding: 80px; }}
h1 {{ font-size: 64px; text-align: center; margin-bottom: 40px; line-height: 1.2; }}
ul {{ list-style: none; font-size: 36px; line-height: 1.8; }}
ul li::before {{ content: "→ "; color: rgba(255,255,255,0.7); }}
</style></head><body>
<h1>{title}</h1>
<ul>{bullets}</ul>
</body></html>"##,
            primary = primary, accent = accent,
            title = html_escape(&slide_title), bullets = bullet_html,
        );
        std::fs::write(slides_dir.join(format!("slide_{:03}.html", i)), &slide_html)
            .map_err(|e| NyayaError::Config(format!("Failed to write slide HTML: {}", e)))?;
    }

    // Convert to PNGs
    let has_wkhtmltoimage = std::process::Command::new("wkhtmltoimage")
        .arg("--version").output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_wkhtmltoimage {
        for i in 0..slides.len() {
            let _ = std::process::Command::new("wkhtmltoimage")
                .args(["--width", "1920", "--height", "1080", "--quality", "90"])
                .arg(slides_dir.join(format!("slide_{:03}.html", i)))
                .arg(slides_dir.join(format!("slide_{:03}.png", i)))
                .output();
        }
    }

    // ffmpeg render
    if slides_dir.join("slide_000.png").exists() {
        let has_ffmpeg = std::process::Command::new("ffmpeg").arg("-version").output()
            .map(|o| o.status.success()).unwrap_or(false);
        if has_ffmpeg {
            let output_mp4 = output_dir.join("output.mp4");
            let result = std::process::Command::new("ffmpeg")
                .args(["-y", "-framerate", "1/5"])
                .arg("-i").arg(slides_dir.join("slide_%03d.png"))
                .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-vf", "scale=1920:1080"])
                .arg(&output_mp4)
                .output();
            if let Ok(output) = result {
                if output.status.success() && output_mp4.exists() {
                    eprintln!("[pea/doc] video generated via ffmpeg: {}", output_mp4.display());
                    return Ok(output_mp4);
                }
            }
            eprintln!("[pea/doc] ffmpeg failed, falling back to slideshow HTML");
        }
    }

    // -----------------------------------------------------------------------
    // Path 3: Interactive slideshow HTML fallback
    // -----------------------------------------------------------------------
    // Extract title from first slide for fallback
    let fallback_title = match slides.first() {
        Some(SlideContent::Title { title, .. }) => title.as_str(),
        _ => "Video",
    };
    generate_slideshow_html_fallback(fallback_title, slides, primary, accent, output_dir)
}

/// Flatten a SlideContent into (title, bullets) for fallback renderers.
fn slide_to_title_bullets(slide: &SlideContent) -> (String, Vec<String>) {
    match slide {
        SlideContent::Title { title, subtitle, .. } => (title.clone(), vec![subtitle.clone()]),
        SlideContent::Content { title, bullets, .. } => (title.clone(), bullets.clone()),
        SlideContent::Timeline { title, events, .. } => {
            let bullets: Vec<String> = events.iter().map(|e| format!("{}: {}", e.date, e.desc)).collect();
            (title.clone(), bullets)
        }
        SlideContent::Stats { title, stats, .. } => {
            let bullets: Vec<String> = stats.iter().map(|s| format!("{}: {}", s.label, s.value)).collect();
            (title.clone(), bullets)
        }
        SlideContent::Quote { text, attribution, .. } => {
            (format!("\"{}\"", text), vec![format!("— {}", attribution)])
        }
        SlideContent::Image { caption, attribution, .. } => {
            (caption.clone(), vec![attribution.clone()])
        }
        SlideContent::Closing { title, subtitle, .. } => (title.clone(), vec![subtitle.clone()]),
    }
}

/// Write a full Remotion project, run `npx remotion render`, return MP4 path.
fn generate_remotion_video(
    slides: &[SlideContent],
    images: &[ImageEntry],
    primary: &str,
    accent: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let remotion_dir = output_dir.join("remotion");
    let src_dir = remotion_dir.join("src");
    let components_dir = src_dir.join("components");
    let public_dir = remotion_dir.join("public");
    std::fs::create_dir_all(&components_dir)
        .map_err(|e| NyayaError::Config(format!("create remotion dirs: {}", e)))?;
    std::fs::create_dir_all(&public_dir)
        .map_err(|e| NyayaError::Config(format!("create public dir: {}", e)))?;

    // Copy images into public/ and build image refs JSON
    // Filter out non-renderable files (.tikz is LaTeX code, not an image)
    let image_extensions = ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp"];
    let mut image_refs: Vec<serde_json::Value> = Vec::new();
    for (caption, path, attribution) in images {
        if let Some(filename) = path.file_name() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !image_extensions.contains(&ext.as_str()) {
                continue; // Skip .tikz and other non-image files
            }
            let dest = public_dir.join(filename);
            if path.exists() {
                let _ = std::fs::copy(path, &dest);
                image_refs.push(serde_json::json!({
                    "caption": caption,
                    "filename": filename.to_string_lossy(),
                    "attribution": attribution.as_deref().unwrap_or(""),
                }));
            }
        }
    }

    // Serialize slide data to JSON — SlideContent has serde(tag = "kind")
    let slides_json: Vec<serde_json::Value> = slides.iter().map(|s| {
        serde_json::to_value(s).unwrap_or_default()
    }).collect();

    // Generate TTS narration if enabled via --narrate flag or NABA_PEA_NARRATE env var
    let audio_dir = public_dir.join("audio");
    let narrate = crate::pea::tts::is_narrate_enabled();
    if narrate {
        let tts = crate::pea::tts::TtsDispatcher::detect();
        if tts.is_available() {
            std::fs::create_dir_all(&audio_dir)
                .map_err(|e| NyayaError::Config(format!("create audio dir: {}", e)))?;
            eprintln!("[pea/doc] generating narration with {} for {} slides", tts.provider(), slides.len());
            for (i, slide) in slides.iter().enumerate() {
                let mp3_path = audio_dir.join(format!("slide_{:03}.mp3", i));
                let (title, bullets) = slide_to_title_bullets(slide);
                let narration = crate::pea::tts::TtsDispatcher::slide_to_narration(&title, &bullets);
                match tts.synthesize(&narration, &mp3_path) {
                    Ok(true) => eprintln!("[pea/doc] narration slide {}: {}", i, mp3_path.display()),
                    Ok(false) => eprintln!("[pea/doc] narration slide {} skipped", i),
                    Err(e) => eprintln!("[pea/doc] narration slide {} error: {}", i, e),
                }
            }
        } else {
            eprintln!("[pea/doc] narration requested but no TTS provider available");
        }
    }

    // Check for narration audio files (populated by TTS above or pre-existing)
    let audio_entries: Vec<serde_json::Value> = if audio_dir.exists() {
        slides.iter().enumerate().filter_map(|(i, _)| {
            let mp3 = audio_dir.join(format!("slide_{:03}.mp3", i));
            if mp3.exists() {
                let duration = measure_audio_duration_ffprobe(&mp3).unwrap_or(5.0);
                Some(serde_json::json!({
                    "filename": format!("audio/slide_{:03}.mp3", i),
                    "durationSecs": duration,
                }))
            } else {
                None
            }
        }).collect()
    } else {
        Vec::new()
    };

    let props = serde_json::json!({
        "slides": slides_json,
        "primaryColor": primary,
        "accentColor": accent,
        "transitionFrames": 45,       // 1.5 second cinematic transition
        "images": image_refs,
        "audio": audio_entries,
    });

    let props_path = remotion_dir.join("slides-data.json");
    std::fs::write(&props_path, serde_json::to_string_pretty(&props).unwrap_or_default())
        .map_err(|e| NyayaError::Config(format!("write props: {}", e)))?;

    // --- package.json ---
    let package_json = r##"{
  "name": "nabaos-pea-video",
  "private": true,
  "scripts": {
    "render": "npx remotion render src/index.ts Slideshow out/video.mp4"
  },
  "dependencies": {
    "react": "19.0.0",
    "react-dom": "19.0.0",
    "remotion": "4.0.248",
    "@remotion/cli": "4.0.248",
    "@remotion/transitions": "4.0.248",
    "@remotion/noise": "4.0.248"
  },
  "devDependencies": {
    "typescript": "5.7.3",
    "@types/react": "19.0.0"
  }
}"##;
    std::fs::write(remotion_dir.join("package.json"), package_json)
        .map_err(|e| NyayaError::Config(format!("write package.json: {}", e)))?;

    // --- tsconfig.json ---
    let tsconfig = r##"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "outDir": "./dist"
  },
  "include": ["src/**/*"]
}"##;
    std::fs::write(remotion_dir.join("tsconfig.json"), tsconfig)
        .map_err(|e| NyayaError::Config(format!("write tsconfig: {}", e)))?;

    // --- src/index.ts ---
    let index_ts = r##"import { registerRoot } from "remotion";
import { RemotionRoot } from "./Root";
registerRoot(RemotionRoot);
"##;
    std::fs::write(src_dir.join("index.ts"), index_ts)
        .map_err(|e| NyayaError::Config(format!("write index.ts: {}", e)))?;

    // --- src/types.ts ---
    let types_ts = r##"export type SlideEntry =
  | { kind: "title"; title: string; subtitle: string; durationFrames: number }
  | { kind: "content"; title: string; bullets: string[]; footnotes: string[]; durationFrames: number }
  | { kind: "timeline"; title: string; events: { date: string; desc: string }[]; durationFrames: number }
  | { kind: "stats"; title: string; stats: { label: string; value: string }[]; durationFrames: number }
  | { kind: "quote"; text: string; attribution: string; durationFrames: number }
  | { kind: "image"; caption: string; filename: string; attribution: string; durationFrames: number }
  | { kind: "closing"; title: string; subtitle: string; durationFrames: number };

export interface ImageRef {
  caption: string;
  filename: string;
  attribution: string;
}

export interface AudioEntry {
  filename: string;
  durationSecs: number;
}

export interface SlideshowProps {
  slides: SlideEntry[];
  primaryColor: string;
  accentColor: string;
  transitionFrames: number;
  images: ImageRef[];
  audio: AudioEntry[];
}
"##;
    std::fs::write(src_dir.join("types.ts"), types_ts)
        .map_err(|e| NyayaError::Config(format!("write types.ts: {}", e)))?;

    // --- src/Root.tsx ---
    let root_tsx = r##"import React from "react";
import { Composition } from "remotion";
import { Slideshow } from "./Slideshow";
import type { SlideshowProps } from "./types";

export const RemotionRoot: React.FC = () => {
  const defaultProps: SlideshowProps = {
    slides: [{ kind: "title", title: "Loading...", subtitle: "", durationFrames: 210 }],
    primaryColor: "#333333",
    accentColor: "#0066CC",
    transitionFrames: 45,
    images: [],
    audio: [],
  };

  return (
    <Composition
      id="Slideshow"
      component={Slideshow}
      width={1920}
      height={1080}
      fps={30}
      durationInFrames={300}
      defaultProps={defaultProps}
      calculateMetadata={async ({ props }) => {
        const fps = 30;
        // Sum per-slide durationFrames, accounting for image slides interleaved
        let totalFrames = 0;
        const imageSlideCount = props.images.length;
        const allSlideCount = props.slides.length + imageSlideCount;
        const transitions = Math.max(0, allSlideCount - 1);

        // Sum per-slide durations from slide metadata
        for (const slide of props.slides) {
          const dur = slide.durationFrames || 150;
          totalFrames += dur;
        }
        // Add image slide durations (150 frames each)
        totalFrames += imageSlideCount * 150;

        // If audio exists, use max(audio, slide duration) per slide
        if (props.audio.length > 0) {
          let audioTotal = 0;
          for (let i = 0; i < allSlideCount; i++) {
            const audioEntry = props.audio[i] || null;
            const slideDur = i < props.slides.length ? (props.slides[i].durationFrames || 150) : 150;
            if (audioEntry) {
              audioTotal += Math.max(Math.ceil(audioEntry.durationSecs * fps) + 10, slideDur);
            } else {
              audioTotal += slideDur;
            }
          }
          totalFrames = audioTotal;
        }

        totalFrames -= transitions * props.transitionFrames;
        return { durationInFrames: Math.max(totalFrames, 30) };
      }}
    />
  );
};
"##;
    std::fs::write(src_dir.join("Root.tsx"), root_tsx)
        .map_err(|e| NyayaError::Config(format!("write Root.tsx: {}", e)))?;

    // --- src/Slideshow.tsx ---
    // Main composition using TransitionSeries with varied transitions + images + audio
    let slideshow_tsx = r##"import React from "react";
import {
  TransitionSeries,
  linearTiming,
  springTiming,
} from "@remotion/transitions";
import { Audio, staticFile } from "remotion";
import { fade } from "@remotion/transitions/fade";
import { slide } from "@remotion/transitions/slide";
import { wipe } from "@remotion/transitions/wipe";
import { Slide } from "./components/Slide";
import { TitleSlide } from "./components/TitleSlide";
import { ClosingSlide } from "./components/ClosingSlide";
import { ImageSlide } from "./components/ImageSlide";
import { TimelineSlide } from "./components/TimelineSlide";
import { StatsSlide } from "./components/StatsSlide";
import { QuoteSlide } from "./components/QuoteSlide";
import type { SlideshowProps, SlideEntry, ImageRef } from "./types";

const TRANSITIONS = [
  () => fade(),
  () => slide({ direction: "from-right" }),
  () => slide({ direction: "from-bottom" }),
  () => wipe({ direction: "from-left" }),
  () => wipe({ direction: "from-top-left" }),
  () => fade(),
  () => slide({ direction: "from-left" }),
  () => wipe({ direction: "from-right" }),
  () => slide({ direction: "from-top" }),
];

const TIMINGS = [
  (frames: number) => linearTiming({ durationInFrames: frames }),
  (frames: number) =>
    springTiming({
      config: { damping: 200 },
      durationInFrames: frames,
    }),
  (frames: number) =>
    springTiming({
      config: { damping: 15, stiffness: 120 },
      durationInFrames: frames,
    }),
];

export const Slideshow: React.FC<SlideshowProps> = ({
  slides,
  primaryColor,
  accentColor,
  transitionFrames,
  images,
  audio,
}) => {
  // Build sequence: interleave image slides after every ~3 content slides
  type SeqEntry =
    | { kind: "slide"; data: SlideEntry; idx: number }
    | { kind: "imageSlide"; data: ImageRef; idx: number };

  const sequence: SeqEntry[] = [];
  let imageIdx = 0;
  let contentCount = 0;

  for (let i = 0; i < slides.length; i++) {
    sequence.push({ kind: "slide", data: slides[i], idx: i });
    const s = slides[i];
    if (s.kind !== "title" && s.kind !== "closing") {
      contentCount++;
      if (contentCount % 3 === 0 && imageIdx < images.length) {
        sequence.push({ kind: "imageSlide", data: images[imageIdx], idx: imageIdx });
        imageIdx++;
      }
    }
  }
  // Append remaining images before closing
  while (imageIdx < images.length) {
    const lastIdx = sequence.length;
    sequence.splice(lastIdx - 1, 0, { kind: "imageSlide", data: images[imageIdx], idx: imageIdx });
    imageIdx++;
  }

  const total = sequence.length;
  const totalSlides = slides.length;
  const fps = 30;

  return (
    <TransitionSeries>
      {sequence.flatMap((entry, seqIdx) => {
        const audioEntry = audio[seqIdx] || null;
        const slideDur = entry.kind === "slide" ? (entry.data.durationFrames || 150) : 150;
        const dur = audioEntry
          ? Math.max(Math.ceil(audioEntry.durationSecs * fps) + 10, slideDur)
          : slideDur;

        const elements: React.ReactNode[] = [];

        if (entry.kind === "imageSlide") {
          elements.push(
            <TransitionSeries.Sequence key={`img-${seqIdx}`} durationInFrames={dur}>
              <ImageSlide
                image={entry.data}
                primaryColor={primaryColor}
                accentColor={accentColor}
              />
              {audioEntry && <Audio src={staticFile(audioEntry.filename)} />}
            </TransitionSeries.Sequence>
          );
        } else {
          const slideData = entry.data;
          const slideIdx = entry.idx;

          elements.push(
            <TransitionSeries.Sequence key={`slide-${seqIdx}`} durationInFrames={dur}>
              {renderSlide(slideData, slideIdx, totalSlides, primaryColor, accentColor)}
              {audioEntry && <Audio src={staticFile(audioEntry.filename)} />}
            </TransitionSeries.Sequence>
          );
        }

        if (seqIdx < total - 1) {
          const presentation = TRANSITIONS[seqIdx % TRANSITIONS.length]();
          const timing = TIMINGS[seqIdx % TIMINGS.length](transitionFrames);
          elements.push(
            <TransitionSeries.Transition
              key={`trans-${seqIdx}`}
              presentation={presentation}
              timing={timing}
            />
          );
        }

        return elements;
      })}
    </TransitionSeries>
  );
};

function renderSlide(
  slide: SlideEntry,
  slideIndex: number,
  totalSlides: number,
  primaryColor: string,
  accentColor: string,
): React.ReactNode {
  switch (slide.kind) {
    case "title":
      return (
        <TitleSlide
          title={slide.title}
          subtitle={slide.subtitle}
          primaryColor={primaryColor}
          accentColor={accentColor}
          slideIndex={slideIndex}
          totalSlides={totalSlides}
        />
      );
    case "content":
      return (
        <Slide
          title={slide.title}
          bullets={slide.bullets}
          footnotes={slide.footnotes}
          primaryColor={primaryColor}
          accentColor={accentColor}
          slideIndex={slideIndex}
          totalSlides={totalSlides}
        />
      );
    case "timeline":
      return (
        <TimelineSlide
          title={slide.title}
          events={slide.events}
          primaryColor={primaryColor}
          accentColor={accentColor}
          slideIndex={slideIndex}
          totalSlides={totalSlides}
        />
      );
    case "stats":
      return (
        <StatsSlide
          title={slide.title}
          stats={slide.stats}
          primaryColor={primaryColor}
          accentColor={accentColor}
          slideIndex={slideIndex}
          totalSlides={totalSlides}
        />
      );
    case "quote":
      return (
        <QuoteSlide
          text={slide.text}
          attribution={slide.attribution}
          primaryColor={primaryColor}
          accentColor={accentColor}
        />
      );
    case "image":
      return (
        <ImageSlide
          image={{ caption: slide.caption, filename: slide.filename, attribution: slide.attribution }}
          primaryColor={primaryColor}
          accentColor={accentColor}
        />
      );
    case "closing":
      return (
        <ClosingSlide
          title={slide.title}
          subtitle={slide.subtitle}
          primaryColor={primaryColor}
          accentColor={accentColor}
          slideIndex={slideIndex}
          totalSlides={totalSlides}
        />
      );
  }
}
"##;
    std::fs::write(src_dir.join("Slideshow.tsx"), slideshow_tsx)
        .map_err(|e| NyayaError::Config(format!("write Slideshow.tsx: {}", e)))?;

    // --- src/components/AnimatedGradient.tsx ---
    let animated_gradient = r##"import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";

export const AnimatedGradient: React.FC<{
  primaryColor: string;
  accentColor: string;
}> = ({ primaryColor, accentColor }) => {
  const frame = useCurrentFrame();
  const angle = interpolate(frame, [0, 300], [135, 155]);
  const shift = interpolate(frame, [0, 300], [0, 12]);
  // Darken the gradient for cinematic depth
  const darkBase = "rgba(8,8,16,1)";

  return (
    <AbsoluteFill
      style={{
        background: `linear-gradient(${angle}deg, ${darkBase} 0%, ${primaryColor} ${20 + shift}%, ${accentColor} 100%)`,
      }}
    />
  );
};
"##;
    std::fs::write(components_dir.join("AnimatedGradient.tsx"), animated_gradient)
        .map_err(|e| NyayaError::Config(format!("write AnimatedGradient.tsx: {}", e)))?;

    // --- src/components/Particles.tsx ---
    let particles = r##"import React, { useMemo } from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";

export const Particles: React.FC<{ count?: number }> = ({ count = 40 }) => {
  const frame = useCurrentFrame();

  const particles = useMemo(
    () =>
      Array.from({ length: count }, (_, i) => ({
        x: ((i * 1327) % 1920),
        y: ((i * 947) % 1080),
        size: (i % 4) + 1,
        speed: 0.2 + (i % 5) * 0.1,
        phase: (i * 0.7) % (Math.PI * 2),
      })),
    [count]
  );

  return (
    <AbsoluteFill style={{ pointerEvents: "none" }}>
      {particles.map((p, i) => {
        const y = ((p.y - frame * p.speed * 2) % 1080 + 1080) % 1080;
        const x = p.x + Math.sin(frame * 0.02 + p.phase) * 25;
        const opacity = interpolate(
          Math.sin(frame * 0.03 + p.phase),
          [-1, 1],
          [0.05, 0.25]
        );
        return (
          <div
            key={i}
            style={{
              position: "absolute",
              left: x,
              top: y,
              width: p.size,
              height: p.size,
              borderRadius: "50%",
              backgroundColor: "rgba(255,255,255,1)",
              opacity,
            }}
          />
        );
      })}
    </AbsoluteFill>
  );
};
"##;
    std::fs::write(components_dir.join("Particles.tsx"), particles)
        .map_err(|e| NyayaError::Config(format!("write Particles.tsx: {}", e)))?;

    // --- src/components/ProgressBar.tsx ---
    let progress_bar = r##"import React from "react";
import { spring, useCurrentFrame, useVideoConfig } from "remotion";

export const ProgressBar: React.FC<{
  slideIndex: number;
  totalSlides: number;
  accentColor: string;
}> = ({ slideIndex, totalSlides, accentColor }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const progress = (slideIndex + 1) / totalSlides;
  const anim = spring({ frame, fps, config: { damping: 20, stiffness: 80 } });

  return (
    <div
      style={{
        position: "absolute",
        bottom: 0,
        left: 0,
        right: 0,
        height: 4,
        backgroundColor: "rgba(255,255,255,0.15)",
      }}
    >
      <div
        style={{
          width: `${progress * 100 * anim}%`,
          height: "100%",
          backgroundColor: accentColor,
          borderRadius: "0 2px 2px 0",
        }}
      />
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("ProgressBar.tsx"), progress_bar)
        .map_err(|e| NyayaError::Config(format!("write ProgressBar.tsx: {}", e)))?;

    // --- src/components/SlideCounter.tsx ---
    let slide_counter = r##"import React from "react";
import { interpolate, useCurrentFrame } from "remotion";

export const SlideCounter: React.FC<{
  current: number;
  total: number;
}> = ({ current, total }) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [0, 15], [0, 0.5], {
    extrapolateRight: "clamp",
  });

  return (
    <div
      style={{
        position: "absolute",
        bottom: 20,
        right: 40,
        fontSize: 18,
        fontFamily: "'JetBrains Mono', monospace",
        color: `rgba(255,255,255,${opacity})`,
        letterSpacing: 2,
      }}
    >
      {String(current + 1).padStart(2, "0")} / {String(total).padStart(2, "0")}
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("SlideCounter.tsx"), slide_counter)
        .map_err(|e| NyayaError::Config(format!("write SlideCounter.tsx: {}", e)))?;

    // --- src/components/AnimatedTitle.tsx ---
    // Word-by-word spring reveal with scale + translateY
    let animated_title = r##"import React from "react";
import { spring, interpolate, useCurrentFrame, useVideoConfig } from "remotion";

export const AnimatedTitle: React.FC<{
  text: string;
  fontSize?: number;
  delay?: number;
  color?: string;
}> = ({ text, fontSize = 72, delay = 0, color = "#ffffff" }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const words = text.split(" ");

  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        justifyContent: "center",
        gap: 16,
        padding: "0 80px",
      }}
    >
      {words.map((word, i) => {
        const wordDelay = delay + i * 5;
        const s = spring({
          frame: frame - wordDelay,
          fps,
          config: { damping: 14, stiffness: 180, mass: 0.8 },
        });
        const opacity = interpolate(s, [0, 1], [0, 1]);
        const translateY = interpolate(s, [0, 1], [40, 0]);
        const scale = interpolate(s, [0, 1], [0.7, 1]);
        const rotation = interpolate(s, [0, 1], [-8, 0]);

        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              fontSize,
              fontWeight: 700,
              color,
              opacity,
              transform: `translateY(${translateY}px) scale(${scale}) rotate(${rotation}deg)`,
              textShadow: "0 4px 20px rgba(0,0,0,0.3)",
              lineHeight: 1.2,
            }}
          >
            {word}
          </span>
        );
      })}
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("AnimatedTitle.tsx"), animated_title)
        .map_err(|e| NyayaError::Config(format!("write AnimatedTitle.tsx: {}", e)))?;

    // --- src/components/AnimatedBullets.tsx ---
    // Staggered bullet point reveal with spring + arrow icon
    let animated_bullets = r##"import React from "react";
import { spring, interpolate, useCurrentFrame, useVideoConfig } from "remotion";

export const AnimatedBullets: React.FC<{
  bullets: string[];
  delay?: number;
  accentColor: string;
}> = ({ bullets, delay = 25, accentColor }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20, maxWidth: 1400 }}>
      {bullets.map((bullet, i) => {
        const bulletDelay = delay + i * 12;
        const s = spring({
          frame: frame - bulletDelay,
          fps,
          config: { damping: 16, stiffness: 120 },
        });
        const opacity = interpolate(s, [0, 1], [0, 1]);
        const translateX = interpolate(s, [0, 1], [-60, 0]);

        // Arrow icon grows in
        const arrowScale = spring({
          frame: frame - bulletDelay - 2,
          fps,
          config: { damping: 10, stiffness: 200 },
        });

        return (
          <div
            key={i}
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 16,
              opacity,
              transform: `translateX(${translateX}px)`,
            }}
          >
            <span
              style={{
                fontSize: 28,
                color: accentColor,
                transform: `scale(${arrowScale})`,
                display: "inline-block",
                flexShrink: 0,
                marginTop: 4,
              }}
            >
              →
            </span>
            <span
              style={{
                fontSize: 32,
                color: "rgba(255,255,255,0.92)",
                lineHeight: 1.5,
                fontWeight: 400,
              }}
            >
              {bullet}
            </span>
          </div>
        );
      })}
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("AnimatedBullets.tsx"), animated_bullets)
        .map_err(|e| NyayaError::Config(format!("write AnimatedBullets.tsx: {}", e)))?;

    // --- src/components/TitleSlide.tsx ---
    let title_slide = r##"import React from "react";
import { AbsoluteFill, spring, interpolate, useCurrentFrame, useVideoConfig } from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { AnimatedTitle } from "./AnimatedTitle";
import { Particles } from "./Particles";
import { CinematicOverlay } from "./CinematicOverlay";
import { SlideCounter } from "./SlideCounter";

export const TitleSlide: React.FC<{
  title: string;
  subtitle: string;
  primaryColor: string;
  accentColor: string;
  slideIndex: number;
  totalSlides: number;
}> = ({ title, subtitle, primaryColor, accentColor, slideIndex, totalSlides }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Decorative line expands
  const lineWidth = spring({
    frame: frame - 10,
    fps,
    config: { damping: 20, stiffness: 60 },
  });
  const lineW = interpolate(lineWidth, [0, 1], [0, 500]);

  // Subtitle fade in
  const subtitleOpacity = interpolate(frame, [50, 75], [0, 0.7], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // Subtle camera drift
  const driftX = interpolate(frame, [0, 210], [2, -2]);
  const driftY = interpolate(frame, [0, 210], [1, -1]);
  const driftScale = interpolate(frame, [0, 210], [1.02, 1.0]);

  return (
    <CinematicOverlay grainIntensity={0.04} vignetteIntensity={0.6}>
      <AbsoluteFill style={{ transform: `scale(${driftScale}) translate(${driftX}px, ${driftY}px)` }}>
        <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
        <Particles count={50} />
      </AbsoluteFill>
      <AbsoluteFill
        style={{
          justifyContent: "center",
          alignItems: "center",
          flexDirection: "column",
          gap: 30,
        }}
      >
        <AnimatedTitle text={title} fontSize={76} delay={5} />
        {/* Decorative line */}
        <div
          style={{
            width: lineW,
            height: 3,
            backgroundColor: accentColor,
            borderRadius: 2,
            opacity: 0.8,
            boxShadow: `0 0 15px ${accentColor}60`,
          }}
        />
        {/* Subtitle */}
        {subtitle && (
          <div
            style={{
              fontSize: 26,
              color: "rgba(255,255,255,0.7)",
              opacity: subtitleOpacity,
              fontWeight: 300,
              letterSpacing: 4,
              textTransform: "uppercase",
              fontFamily: "'Inter', 'Helvetica Neue', sans-serif",
            }}
          >
            {subtitle}
          </div>
        )}
      </AbsoluteFill>
      <SlideCounter current={slideIndex} total={totalSlides} />
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("TitleSlide.tsx"), title_slide)
        .map_err(|e| NyayaError::Config(format!("write TitleSlide.tsx: {}", e)))?;

    // --- src/components/Slide.tsx ---
    let slide_component = r##"import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { AnimatedTitle } from "./AnimatedTitle";
import { AnimatedBullets } from "./AnimatedBullets";
import { FootnoteBar } from "./FootnoteBar";
import { LowerThird } from "./LowerThird";
import { CinematicOverlay } from "./CinematicOverlay";
import { Particles } from "./Particles";
import { ProgressBar } from "./ProgressBar";
import { SlideCounter } from "./SlideCounter";

export const Slide: React.FC<{
  title: string;
  bullets: string[];
  footnotes?: string[];
  primaryColor: string;
  accentColor: string;
  slideIndex: number;
  totalSlides: number;
}> = ({ title, bullets, footnotes, primaryColor, accentColor, slideIndex, totalSlides }) => {
  const frame = useCurrentFrame();

  // Section number indicator
  const numOpacity = interpolate(frame, [0, 10], [0, 0.08], {
    extrapolateRight: "clamp",
  });

  // Subtle camera drift for cinematic feel
  const driftX = interpolate(frame, [0, 180], [1, -1]);
  const driftScale = interpolate(frame, [0, 180], [1.01, 1.0]);

  return (
    <CinematicOverlay>
      <AbsoluteFill style={{ transform: `scale(${driftScale}) translateX(${driftX}px)` }}>
        <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
        <Particles count={20} />
      </AbsoluteFill>

      {/* Large section number watermark */}
      <div
        style={{
          position: "absolute",
          right: 60,
          top: 40,
          fontSize: 220,
          fontWeight: 900,
          color: "white",
          opacity: numOpacity,
          fontFamily: "'JetBrains Mono', monospace",
          letterSpacing: -8,
        }}
      >
        {String(slideIndex).padStart(2, "0")}
      </div>

      <AbsoluteFill
        style={{
          justifyContent: "center",
          padding: "80px 120px",
          gap: 36,
          flexDirection: "column",
        }}
      >
        <AnimatedTitle text={title} fontSize={52} delay={0} />
        <div style={{ marginTop: 16 }}>
          <AnimatedBullets bullets={bullets} delay={20} accentColor={accentColor} />
        </div>
      </AbsoluteFill>

      {/* Chapter marker lower-third */}
      <LowerThird
        label={`Section ${String(slideIndex).padStart(2, "0")}`}
        sublabel={title.length > 40 ? title.slice(0, 40) + "..." : title}
        accentColor={accentColor}
        delay={3}
      />

      {footnotes && footnotes.length > 0 && <FootnoteBar footnotes={footnotes} />}

      <ProgressBar
        slideIndex={slideIndex}
        totalSlides={totalSlides}
        accentColor={accentColor}
      />
      <SlideCounter current={slideIndex} total={totalSlides} />
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("Slide.tsx"), slide_component)
        .map_err(|e| NyayaError::Config(format!("write Slide.tsx: {}", e)))?;

    // --- src/components/ClosingSlide.tsx ---
    let closing_slide = r##"import React from "react";
import {
  AbsoluteFill,
  spring,
  interpolate,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { CinematicOverlay } from "./CinematicOverlay";
import { Particles } from "./Particles";

export const ClosingSlide: React.FC<{
  title: string;
  subtitle: string;
  primaryColor: string;
  accentColor: string;
  slideIndex: number;
  totalSlides: number;
}> = ({ title, subtitle, primaryColor, accentColor }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Title scales up with spring
  const s = spring({
    frame: frame - 5,
    fps,
    config: { damping: 12, stiffness: 100, mass: 1.2 },
  });
  const scale = interpolate(s, [0, 1], [0.5, 1]);
  const opacity = interpolate(s, [0, 1], [0, 1]);

  // Subtitle fades in later
  const subOpacity = interpolate(frame, [30, 50], [0, 0.6], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // Ring decoration expands
  const ringScale = spring({
    frame: frame - 15,
    fps,
    config: { damping: 18, stiffness: 60 },
  });

  return (
    <CinematicOverlay grainIntensity={0.03} vignetteIntensity={0.65}>
      <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
      <Particles count={60} />

      {/* Decorative ring */}
      <AbsoluteFill style={{ justifyContent: "center", alignItems: "center" }}>
        <div
          style={{
            width: 300,
            height: 300,
            borderRadius: "50%",
            border: `2px solid rgba(255,255,255,0.08)`,
            transform: `scale(${ringScale * 2})`,
            position: "absolute",
            boxShadow: "0 0 40px rgba(255,255,255,0.03)",
          }}
        />
      </AbsoluteFill>

      <AbsoluteFill
        style={{
          justifyContent: "center",
          alignItems: "center",
          flexDirection: "column",
          gap: 24,
        }}
      >
        <div
          style={{
            fontSize: 90,
            fontWeight: 800,
            color: "white",
            opacity,
            transform: `scale(${scale})`,
            textShadow: "0 4px 30px rgba(0,0,0,0.4)",
            fontFamily: "'Inter', 'Helvetica Neue', sans-serif",
          }}
        >
          {title}
        </div>
        {subtitle && (
          <div
            style={{
              fontSize: 20,
              color: "rgba(255,255,255,0.55)",
              opacity: subOpacity,
              letterSpacing: 3,
              fontWeight: 300,
              textTransform: "uppercase",
            }}
          >
            {subtitle}
          </div>
        )}
      </AbsoluteFill>
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("ClosingSlide.tsx"), closing_slide)
        .map_err(|e| NyayaError::Config(format!("write ClosingSlide.tsx: {}", e)))?;

    // --- src/components/ImageSlide.tsx ---
    let image_slide = r##"import React from "react";
import {
  AbsoluteFill,
  Img,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { CinematicOverlay } from "./CinematicOverlay";
import type { ImageRef } from "../types";

export const ImageSlide: React.FC<{
  image: ImageRef;
  primaryColor: string;
  accentColor: string;
}> = ({ image, primaryColor, accentColor }) => {
  const frame = useCurrentFrame();

  const fadeIn = interpolate(frame, [0, 25], [0, 1], {
    extrapolateRight: "clamp",
  });

  // Ken Burns effect — slow zoom + pan over duration
  const kenBurnsScale = interpolate(frame, [0, 150], [1.0, 1.12], {
    extrapolateRight: "clamp",
  });
  const kenBurnsPanX = interpolate(frame, [0, 150], [0, -25], {
    extrapolateRight: "clamp",
  });
  const kenBurnsPanY = interpolate(frame, [0, 150], [0, -10], {
    extrapolateRight: "clamp",
  });

  const captionOpacity = interpolate(frame, [30, 50], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <CinematicOverlay grainIntensity={0.03} vignetteIntensity={0.55}>
      <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
      <AbsoluteFill
        style={{
          justifyContent: "center",
          alignItems: "center",
          padding: 50,
        }}
      >
        <div
          style={{
            opacity: fadeIn,
            borderRadius: 8,
            overflow: "hidden",
            boxShadow: "0 25px 80px rgba(0,0,0,0.5)",
            maxWidth: 1600,
            maxHeight: 820,
          }}
        >
          <Img
            src={staticFile(image.filename)}
            style={{
              width: "100%",
              height: "100%",
              objectFit: "cover",
              transform: `scale(${kenBurnsScale}) translate(${kenBurnsPanX}px, ${kenBurnsPanY}px)`,
            }}
          />
        </div>
        <div
          style={{
            position: "absolute",
            bottom: 70,
            textAlign: "center",
            opacity: captionOpacity,
          }}
        >
          <div
            style={{
              fontSize: 26,
              color: "rgba(255,255,255,0.92)",
              fontWeight: 500,
              textShadow: "0 2px 15px rgba(0,0,0,0.7)",
              padding: "8px 24px",
              backgroundColor: "rgba(0,0,0,0.35)",
              borderRadius: 4,
              backdropFilter: "blur(4px)",
            }}
          >
            {image.caption}
          </div>
          {image.attribution && (
            <div
              style={{
                fontSize: 14,
                color: "rgba(255,255,255,0.45)",
                marginTop: 8,
                fontStyle: "italic",
              }}
            >
              {image.attribution}
            </div>
          )}
        </div>
      </AbsoluteFill>
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("ImageSlide.tsx"), image_slide)
        .map_err(|e| NyayaError::Config(format!("write ImageSlide.tsx: {}", e)))?;

    // --- src/components/FootnoteBar.tsx ---
    let footnote_bar = r##"import React from "react";
import { interpolate, useCurrentFrame } from "remotion";

export const FootnoteBar: React.FC<{
  footnotes: string[];
}> = ({ footnotes }) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [80, 100], [0, 0.45], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <div
      style={{
        position: "absolute",
        bottom: 25,
        left: 40,
        right: 200,
        fontSize: 13,
        color: "rgba(255,255,255,1)",
        opacity,
        fontFamily: "'JetBrains Mono', monospace",
        letterSpacing: 0.5,
      }}
    >
      {footnotes.map((fn, i) => (
        <span key={i} style={{ marginRight: 20 }}>
          [{i + 1}] {fn}
        </span>
      ))}
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("FootnoteBar.tsx"), footnote_bar)
        .map_err(|e| NyayaError::Config(format!("write FootnoteBar.tsx: {}", e)))?;

    // --- src/components/TimelineSlide.tsx ---
    let timeline_slide = r##"import React from "react";
import {
  AbsoluteFill,
  spring,
  interpolate,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { AnimatedTitle } from "./AnimatedTitle";
import { CinematicOverlay } from "./CinematicOverlay";
import { LowerThird } from "./LowerThird";
import { Particles } from "./Particles";
import { ProgressBar } from "./ProgressBar";
import { SlideCounter } from "./SlideCounter";

export const TimelineSlide: React.FC<{
  title: string;
  events: { date: string; desc: string }[];
  primaryColor: string;
  accentColor: string;
  slideIndex: number;
  totalSlides: number;
}> = ({ title, events, primaryColor, accentColor, slideIndex, totalSlides }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Vertical line animation
  const lineProgress = interpolate(frame, [10, 80], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // Camera drift
  const driftY = interpolate(frame, [0, 240], [2, -2]);

  return (
    <CinematicOverlay>
      <AbsoluteFill style={{ transform: `translateY(${driftY}px)` }}>
        <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
        <Particles count={12} />
      </AbsoluteFill>

      <AbsoluteFill
        style={{
          padding: "60px 120px",
          flexDirection: "column",
          gap: 20,
        }}
      >
        <AnimatedTitle text={title} fontSize={42} delay={0} />

        <div style={{ position: "relative", marginTop: 30, marginLeft: 80, flex: 1 }}>
          {/* Vertical connecting line */}
          <div
            style={{
              position: "absolute",
              left: 6,
              top: 0,
              width: 2,
              height: `${lineProgress * 100}%`,
              backgroundColor: accentColor,
              opacity: 0.6,
            }}
          />

          {events.map((event, i) => {
            const delay = 15 + i * 18;
            const s = spring({
              frame: frame - delay,
              fps,
              config: { damping: 14, stiffness: 150 },
            });
            const opacity = interpolate(s, [0, 1], [0, 1]);
            const translateX = interpolate(s, [0, 1], [-40, 0]);

            return (
              <div
                key={i}
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 24,
                  marginBottom: 24,
                  opacity,
                  transform: `translateX(${translateX}px)`,
                }}
              >
                {/* Timeline marker */}
                <div
                  style={{
                    width: 14,
                    height: 14,
                    borderRadius: "50%",
                    backgroundColor: accentColor,
                    flexShrink: 0,
                    marginTop: 8,
                    boxShadow: `0 0 10px ${accentColor}`,
                  }}
                />
                {/* Date */}
                <div
                  style={{
                    fontSize: 26,
                    fontWeight: 700,
                    color: accentColor,
                    minWidth: 80,
                    fontFamily: "'JetBrains Mono', monospace",
                  }}
                >
                  {event.date}
                </div>
                {/* Description */}
                <div
                  style={{
                    fontSize: 24,
                    color: "rgba(255,255,255,0.88)",
                    lineHeight: 1.4,
                    maxWidth: 1200,
                  }}
                >
                  {event.desc}
                </div>
              </div>
            );
          })}
        </div>
      </AbsoluteFill>

      <LowerThird label="Timeline" accentColor={accentColor} delay={5} />

      <ProgressBar slideIndex={slideIndex} totalSlides={totalSlides} accentColor={accentColor} />
      <SlideCounter current={slideIndex} total={totalSlides} />
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("TimelineSlide.tsx"), timeline_slide)
        .map_err(|e| NyayaError::Config(format!("write TimelineSlide.tsx: {}", e)))?;

    // --- src/components/StatsSlide.tsx ---
    let stats_slide = r##"import React from "react";
import {
  AbsoluteFill,
  interpolate,
  useCurrentFrame,
} from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { AnimatedTitle } from "./AnimatedTitle";
import { CinematicOverlay } from "./CinematicOverlay";
import { LowerThird } from "./LowerThird";
import { Particles } from "./Particles";
import { ProgressBar } from "./ProgressBar";
import { SlideCounter } from "./SlideCounter";

export const StatsSlide: React.FC<{
  title: string;
  stats: { label: string; value: string }[];
  primaryColor: string;
  accentColor: string;
  slideIndex: number;
  totalSlides: number;
}> = ({ title, stats, primaryColor, accentColor, slideIndex, totalSlides }) => {
  const frame = useCurrentFrame();

  // Determine grid columns based on stat count
  const cols = stats.length <= 2 ? 2 : 3;

  return (
    <CinematicOverlay>
      <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
      <Particles count={15} />

      <AbsoluteFill
        style={{
          padding: "60px 120px",
          flexDirection: "column",
          justifyContent: "center",
          alignItems: "center",
          gap: 50,
        }}
      >
        <AnimatedTitle text={title} fontSize={42} delay={0} />

        <div
          style={{
            display: "grid",
            gridTemplateColumns: `repeat(${cols}, 1fr)`,
            gap: 60,
            maxWidth: 1400,
            width: "100%",
          }}
        >
          {stats.map((stat, i) => {
            const delay = 20 + i * 15;
            const progress = interpolate(frame - delay, [0, 40], [0, 1], {
              extrapolateLeft: "clamp",
              extrapolateRight: "clamp",
            });
            const scale = interpolate(progress, [0, 1], [0.5, 1]);
            const opacity = interpolate(progress, [0, 1], [0, 1]);

            return (
              <div
                key={i}
                style={{
                  textAlign: "center",
                  opacity,
                  transform: `scale(${scale})`,
                }}
              >
                <div
                  style={{
                    fontSize: 56,
                    fontWeight: 800,
                    color: accentColor,
                    fontFamily: "'JetBrains Mono', monospace",
                    textShadow: `0 0 20px ${accentColor}40`,
                    marginBottom: 12,
                  }}
                >
                  {stat.value}
                </div>
                <div
                  style={{
                    fontSize: 20,
                    color: "rgba(255,255,255,0.7)",
                    fontWeight: 400,
                    textTransform: "uppercase",
                    letterSpacing: 2,
                  }}
                >
                  {stat.label}
                </div>
              </div>
            );
          })}
        </div>
      </AbsoluteFill>

      <LowerThird label="Key Figures" accentColor={accentColor} delay={5} />

      <ProgressBar slideIndex={slideIndex} totalSlides={totalSlides} accentColor={accentColor} />
      <SlideCounter current={slideIndex} total={totalSlides} />
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("StatsSlide.tsx"), stats_slide)
        .map_err(|e| NyayaError::Config(format!("write StatsSlide.tsx: {}", e)))?;

    // --- src/components/QuoteSlide.tsx ---
    let quote_slide = r##"import React from "react";
import {
  AbsoluteFill,
  interpolate,
  useCurrentFrame,
} from "remotion";
import { AnimatedGradient } from "./AnimatedGradient";
import { CinematicOverlay } from "./CinematicOverlay";
import { Particles } from "./Particles";

export const QuoteSlide: React.FC<{
  text: string;
  attribution: string;
  primaryColor: string;
  accentColor: string;
}> = ({ text, attribution, primaryColor, accentColor }) => {
  const frame = useCurrentFrame();

  const fadeIn = interpolate(frame, [5, 30], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const attrOpacity = interpolate(frame, [35, 55], [0, 0.7], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <CinematicOverlay grainIntensity={0.04} vignetteIntensity={0.6}>
      <AnimatedGradient primaryColor={primaryColor} accentColor={accentColor} />
      <Particles count={25} />

      <AbsoluteFill
        style={{
          justifyContent: "center",
          alignItems: "center",
          flexDirection: "column",
          padding: "80px 160px",
          gap: 40,
        }}
      >
        {/* Opening quote mark */}
        <div
          style={{
            fontSize: 140,
            color: accentColor,
            opacity: fadeIn * 0.3,
            fontFamily: "Georgia, serif",
            lineHeight: 0.8,
            marginBottom: -20,
            textShadow: `0 0 30px ${accentColor}30`,
          }}
        >
          {"\u201C"}
        </div>

        {/* Quote text */}
        <div
          style={{
            fontSize: 38,
            color: "rgba(255,255,255,0.95)",
            textAlign: "center",
            lineHeight: 1.6,
            fontStyle: "italic",
            maxWidth: 1200,
            opacity: fadeIn,
            fontWeight: 300,
          }}
        >
          {text}
        </div>

        {/* Closing quote mark */}
        <div
          style={{
            fontSize: 120,
            color: accentColor,
            opacity: fadeIn * 0.4,
            fontFamily: "Georgia, serif",
            lineHeight: 0.5,
            marginTop: -20,
          }}
        >
          {"\u201D"}
        </div>

        {/* Attribution */}
        <div
          style={{
            fontSize: 24,
            color: "rgba(255,255,255,0.6)",
            opacity: attrOpacity,
            fontStyle: "italic",
            letterSpacing: 1,
          }}
        >
          — {attribution}
        </div>
      </AbsoluteFill>
    </CinematicOverlay>
  );
};
"##;
    std::fs::write(components_dir.join("QuoteSlide.tsx"), quote_slide)
        .map_err(|e| NyayaError::Config(format!("write QuoteSlide.tsx: {}", e)))?;

    // --- src/components/FilmGrain.tsx ---
    // Uses CSS background-image with a tiny inline noise pattern instead of SVG feTurbulence.
    // feTurbulence is extremely expensive in headless Chrome (~5s/frame).
    // A static noise tile scrolled with transform is near-free.
    let film_grain = r##"import React from "react";
import { AbsoluteFill, useCurrentFrame } from "remotion";

// 4x4 pixel noise tile encoded as base64 PNG — repeats seamlessly
const NOISE_TILE = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAYAAACp8Z5+AAAAMklEQVQIW2P4z8DwHwMDAwMDIwMDAwOY+J+BgYEBxGJkYGBgYGD4/5+BgYEB5AJGBgYGAH2cDhUAAAAASUVORK5CYII=";

export const FilmGrain: React.FC<{ intensity?: number }> = ({ intensity = 0.06 }) => {
  const frame = useCurrentFrame();
  // Shift the noise tile slightly each frame for flicker effect
  const offsetX = (frame * 3) % 100;
  const offsetY = (frame * 7) % 100;

  return (
    <AbsoluteFill
      style={{
        pointerEvents: "none",
        mixBlendMode: "overlay",
        opacity: intensity,
        backgroundImage: `url(${NOISE_TILE})`,
        backgroundRepeat: "repeat",
        backgroundSize: "100px 100px",
        backgroundPosition: `${offsetX}px ${offsetY}px`,
        filter: "contrast(200%) brightness(150%)",
      }}
    />
  );
};
"##;
    std::fs::write(components_dir.join("FilmGrain.tsx"), film_grain)
        .map_err(|e| NyayaError::Config(format!("write FilmGrain.tsx: {}", e)))?;

    // --- src/components/Vignette.tsx ---
    let vignette = r##"import React from "react";
import { AbsoluteFill } from "remotion";

export const Vignette: React.FC<{ intensity?: number }> = ({ intensity = 0.55 }) => {
  return (
    <AbsoluteFill
      style={{
        pointerEvents: "none",
        background: `radial-gradient(ellipse at center, transparent 50%, rgba(0,0,0,${intensity}) 100%)`,
      }}
    />
  );
};
"##;
    std::fs::write(components_dir.join("Vignette.tsx"), vignette)
        .map_err(|e| NyayaError::Config(format!("write Vignette.tsx: {}", e)))?;

    // --- src/components/CinematicOverlay.tsx ---
    // Wraps any content with film grain + vignette + subtle color grading
    let cinematic_overlay = r##"import React from "react";
import { AbsoluteFill } from "remotion";
import { FilmGrain } from "./FilmGrain";
import { Vignette } from "./Vignette";

export const CinematicOverlay: React.FC<{
  children: React.ReactNode;
  grainIntensity?: number;
  vignetteIntensity?: number;
}> = ({ children, grainIntensity = 0.05, vignetteIntensity = 0.5 }) => {
  return (
    <AbsoluteFill
      style={{
        // Subtle color grading: slight warm tint, boosted contrast
        filter: "contrast(1.08) saturate(1.12) brightness(0.97)",
      }}
    >
      {children}
      <Vignette intensity={vignetteIntensity} />
      <FilmGrain intensity={grainIntensity} />
    </AbsoluteFill>
  );
};
"##;
    std::fs::write(components_dir.join("CinematicOverlay.tsx"), cinematic_overlay)
        .map_err(|e| NyayaError::Config(format!("write CinematicOverlay.tsx: {}", e)))?;

    // --- src/components/LowerThird.tsx ---
    // Broadcast-style lower-third with animated accent bar + text reveal
    let lower_third = r##"import React from "react";
import { spring, interpolate, useCurrentFrame, useVideoConfig } from "remotion";

export const LowerThird: React.FC<{
  label: string;
  sublabel?: string;
  accentColor: string;
  delay?: number;
}> = ({ label, sublabel, accentColor, delay = 5 }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Accent bar slides in
  const barWidth = spring({
    frame: frame - delay,
    fps,
    config: { damping: 18, stiffness: 100 },
  });
  const barW = interpolate(barWidth, [0, 1], [0, 4]);

  // Text fades in after bar
  const textOpacity = interpolate(frame - delay, [8, 22], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const textX = interpolate(frame - delay, [8, 22], [20, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // Background panel
  const panelWidth = spring({
    frame: frame - delay,
    fps,
    config: { damping: 20, stiffness: 80 },
  });
  const panelW = interpolate(panelWidth, [0, 1], [0, 100]);

  return (
    <div
      style={{
        position: "absolute",
        bottom: 80,
        left: 60,
        display: "flex",
        alignItems: "stretch",
        gap: 0,
      }}
    >
      {/* Accent bar */}
      <div
        style={{
          width: barW,
          backgroundColor: accentColor,
          borderRadius: 2,
        }}
      />
      {/* Text panel */}
      <div
        style={{
          backgroundColor: "rgba(0,0,0,0.65)",
          padding: "14px 28px",
          borderRadius: "0 4px 4px 0",
          opacity: panelW / 100,
          overflow: "hidden",
          backdropFilter: "blur(8px)",
        }}
      >
        <div
          style={{
            fontSize: 22,
            fontWeight: 600,
            color: "rgba(255,255,255,0.95)",
            opacity: textOpacity,
            transform: `translateX(${textX}px)`,
            letterSpacing: 1,
            textTransform: "uppercase",
            whiteSpace: "nowrap",
          }}
        >
          {label}
        </div>
        {sublabel && (
          <div
            style={{
              fontSize: 15,
              color: "rgba(255,255,255,0.55)",
              opacity: textOpacity,
              transform: `translateX(${textX}px)`,
              marginTop: 4,
              letterSpacing: 0.5,
              whiteSpace: "nowrap",
            }}
          >
            {sublabel}
          </div>
        )}
      </div>
    </div>
  );
};
"##;
    std::fs::write(components_dir.join("LowerThird.tsx"), lower_third)
        .map_err(|e| NyayaError::Config(format!("write LowerThird.tsx: {}", e)))?;

    // --- Install dependencies and render ---
    eprintln!("[pea/doc] installing Remotion dependencies...");
    let install = std::process::Command::new("npm")
        .arg("install")
        .current_dir(&remotion_dir)
        .output()
        .map_err(|e| NyayaError::Config(format!("npm install: {}", e)))?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        return Err(NyayaError::Config(format!("npm install failed: {}", stderr)));
    }

    // Create output directory
    std::fs::create_dir_all(remotion_dir.join("out"))
        .map_err(|e| NyayaError::Config(format!("create out dir: {}", e)))?;

    let output_mp4 = output_dir.join("output.mp4");

    eprintln!("[pea/doc] rendering video with Remotion...");
    let render = std::process::Command::new("npx")
        .args([
            "remotion", "render",
            "--codec=h264",
            "--crf=17",
            "--jpeg-quality=95",
            "--color-space=bt709",
        ])
        .arg(format!("--props={}", props_path.display()))
        .args(["src/index.ts", "Slideshow"])
        .arg(&output_mp4)
        .current_dir(&remotion_dir)
        .output()
        .map_err(|e| NyayaError::Config(format!("remotion render: {}", e)))?;

    if render.status.success() && output_mp4.exists() {
        eprintln!("[pea/doc] Remotion render complete: {}", output_mp4.display());
        // Clean up the remotion project directory (keep output)
        let _ = std::fs::remove_dir_all(remotion_dir.join("node_modules"));
        return Ok(output_mp4);
    }

    let stderr = String::from_utf8_lossy(&render.stderr);
    let stdout = String::from_utf8_lossy(&render.stdout);
    Err(NyayaError::Config(format!(
        "Remotion render failed:\nstdout: {}\nstderr: {}",
        &stdout[..stdout.len().min(500)],
        &stderr[..stderr.len().min(500)],
    )))
}

// ── PixiJS Motion Graphics Video Generator ─────────────────────────────────

/// Generate a PixiJS + Remotion motion graphics video from VideoScene data.
pub fn generate_pixi_video(
    scenes: &[VideoScene],
    _images: &[ImageEntry],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
    let s = style.cloned().unwrap_or_default();
    let primary = &s.primary_color;
    let accent = &s.accent_color;

    // Check for Node.js + npm
    let has_node = std::process::Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    let has_npm = std::process::Command::new("npm").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !has_node || !has_npm {
        return Err(NyayaError::Config("PixiJS video requires Node.js and npm".to_string()));
    }

    let total_scenes = scenes.len();
    eprintln!("[pea/doc] PixiJS + Remotion: rendering {} scenes", total_scenes);

    let remotion_dir = output_dir.join("remotion");
    let src_dir = remotion_dir.join("src");
    let scenes_dir = src_dir.join("scenes");
    std::fs::create_dir_all(&scenes_dir)
        .map_err(|e| NyayaError::Config(format!("create scenes dir: {}", e)))?;

    // --- Write scenes-data.json ---
    let scenes_json = serde_json::to_string_pretty(scenes)
        .map_err(|e| NyayaError::Config(format!("serialize scenes: {}", e)))?;
    let props_path = remotion_dir.join("scenes-data.json");
    std::fs::write(&props_path, &scenes_json)
        .map_err(|e| NyayaError::Config(format!("write scenes-data.json: {}", e)))?;

    // --- package.json ---
    let package_json = format!(r#"{{
  "name": "pixi-motion-video",
  "private": true,
  "scripts": {{ "build": "remotion render" }},
  "dependencies": {{
    "@remotion/cli": "4.0.248",
    "@remotion/bundler": "4.0.248",
    "remotion": "4.0.248",
    "react": "19.0.0",
    "react-dom": "19.0.0",
    "pixi.js": "8.6.6"
  }},
  "devDependencies": {{
    "typescript": "5.7.3",
    "@types/react": "19.0.1"
  }}
}}"#);
    std::fs::write(remotion_dir.join("package.json"), &package_json)
        .map_err(|e| NyayaError::Config(format!("write package.json: {}", e)))?;

    // --- tsconfig.json ---
    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "baseUrl": "."
  },
  "include": ["src"]
}"#;
    std::fs::write(remotion_dir.join("tsconfig.json"), tsconfig)
        .map_err(|e| NyayaError::Config(format!("write tsconfig.json: {}", e)))?;

    // --- src/types.ts ---
    let types_ts = r#"export interface CounterEntry { label: string; value: string; unit?: string; }
export interface BarEntry { label: string; value: number; }
export interface WaypointEntry { year: string; label: string; }
export interface ComparisonPoint { left: string; right: string; }

export type VideoScene =
  | { kind: "opener"; title: string; subtitle?: string; mood?: string; durationFrames: number }
  | { kind: "kineticText"; text: string; layout: string; durationFrames: number }
  | { kind: "dataCounter"; title: string; counters: CounterEntry[]; durationFrames: number }
  | { kind: "barRace"; title: string; bars: BarEntry[]; durationFrames: number }
  | { kind: "particleMood"; text: string; preset: string; durationFrames: number }
  | { kind: "timelinePath"; title: string; waypoints: WaypointEntry[]; durationFrames: number }
  | { kind: "comparisonSplit"; title: string; leftLabel?: string; rightLabel?: string; points: ComparisonPoint[]; durationFrames: number }
  | { kind: "closing"; title: string; subtitle?: string; durationFrames: number };

export interface MotionVideoProps {
  scenes: VideoScene[];
  primaryColor: string;
  accentColor: string;
}
"#;
    std::fs::write(src_dir.join("types.ts"), types_ts)
        .map_err(|e| NyayaError::Config(format!("write types.ts: {}", e)))?;

    // --- src/easing.ts ---
    let easing_ts = r#"export function easeOutExpo(t: number): number {
  return t === 1 ? 1 : 1 - Math.pow(2, -10 * t);
}

export function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

export function spring(t: number, damping = 0.7, freq = 4): number {
  if (t <= 0) return 0;
  if (t >= 1) return 1;
  return 1 - Math.exp(-damping * freq * t) * Math.cos(freq * Math.PI * 2 * t * (1 - damping));
}

export function stagger(index: number, total: number, progress: number, overlap = 0.3): number {
  const slotWidth = 1 / (total + (total - 1) * overlap);
  const start = index * slotWidth * (1 - overlap);
  const end = start + slotWidth;
  return Math.max(0, Math.min(1, (progress - start) / (end - start)));
}
"#;
    std::fs::write(src_dir.join("easing.ts"), easing_ts)
        .map_err(|e| NyayaError::Config(format!("write easing.ts: {}", e)))?;

    // --- src/pixi-canvas.tsx ---
    let pixi_canvas = r#"import React, { useEffect, useRef } from "react";
import { useCurrentFrame, useVideoConfig } from "remotion";
import { Application, Container } from "pixi.js";

interface PixiCanvasProps {
  setup: (app: Application) => void;
  update: (app: Application, progress: number) => void;
  bg?: number;
}

export const PixiCanvas: React.FC<PixiCanvasProps> = ({ setup, update, bg = 0x16161e }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const appRef = useRef<Application | null>(null);
  const setupDone = useRef(false);
  const frame = useCurrentFrame();
  const { durationInFrames } = useVideoConfig();
  const progress = frame / durationInFrames;

  useEffect(() => {
    if (!canvasRef.current || setupDone.current) return;
    setupDone.current = true;
    const app = new Application();
    app.init({
      canvas: canvasRef.current,
      width: 1920,
      height: 1080,
      backgroundColor: bg,
      autoStart: false,
      antialias: true,
    }).then(() => {
      setup(app);
      appRef.current = app;
    });
    return () => { app.destroy(true); appRef.current = null; setupDone.current = false; };
  }, []);

  useEffect(() => {
    if (appRef.current) {
      update(appRef.current, progress);
      appRef.current.render();
    }
  }, [frame]);

  return <canvas ref={canvasRef} width={1920} height={1080} style={{ width: "100%", height: "100%" }} />;
};
"#;
    std::fs::write(src_dir.join("pixi-canvas.tsx"), pixi_canvas)
        .map_err(|e| NyayaError::Config(format!("write pixi-canvas.tsx: {}", e)))?;

    // --- Scene components ---
    write_pixi_scene_opener(&scenes_dir, primary, accent)?;
    write_pixi_scene_kinetic_text(&scenes_dir, primary, accent)?;
    write_pixi_scene_data_counter(&scenes_dir, primary, accent)?;
    write_pixi_scene_bar_race(&scenes_dir, primary, accent)?;
    write_pixi_scene_particle_mood(&scenes_dir, primary, accent)?;
    write_pixi_scene_timeline_path(&scenes_dir, primary, accent)?;
    write_pixi_scene_comparison_split(&scenes_dir, primary, accent)?;
    write_pixi_scene_closing(&scenes_dir, primary, accent)?;

    // --- src/MotionVideo.tsx ---
    let motion_video = format!(r##"import React from "react";
import {{ Sequence, useVideoConfig }} from "remotion";
import {{ VideoScene }} from "./types";
import {{ OpenerScene }} from "./scenes/opener";
import {{ KineticTextScene }} from "./scenes/kinetic-text";
import {{ DataCounterScene }} from "./scenes/data-counter";
import {{ BarRaceScene }} from "./scenes/bar-race";
import {{ ParticleMoodScene }} from "./scenes/particle-mood";
import {{ TimelinePathScene }} from "./scenes/timeline-path";
import {{ ComparisonSplitScene }} from "./scenes/comparison-split";
import {{ ClosingScene }} from "./scenes/closing";

const CROSSFADE = 15;

function renderScene(scene: VideoScene) {{
  switch (scene.kind) {{
    case "opener": return <OpenerScene title={{scene.title}} subtitle={{scene.subtitle || ""}} mood={{scene.mood || "dramatic"}} />;
    case "kineticText": return <KineticTextScene text={{scene.text}} layout={{scene.layout}} />;
    case "dataCounter": return <DataCounterScene title={{scene.title}} counters={{scene.counters}} />;
    case "barRace": return <BarRaceScene title={{scene.title}} bars={{scene.bars}} />;
    case "particleMood": return <ParticleMoodScene text={{scene.text}} preset={{scene.preset}} />;
    case "timelinePath": return <TimelinePathScene title={{scene.title}} waypoints={{scene.waypoints}} />;
    case "comparisonSplit": return <ComparisonSplitScene title={{scene.title}} leftLabel={{scene.leftLabel || "Before"}} rightLabel={{scene.rightLabel || "After"}} points={{scene.points}} />;
    case "closing": return <ClosingScene title={{scene.title}} subtitle={{scene.subtitle || ""}} />;
  }}
}}

export const MotionVideo: React.FC<{{ scenes: VideoScene[] }}> = ({{ scenes }}) => {{
  const {{ fps }} = useVideoConfig();
  let offset = 0;

  return (
    <div style={{{{ width: 1920, height: 1080, backgroundColor: "#16161e" }}}}>
      {{scenes.map((scene, i) => {{
        const dur = scene.durationFrames;
        const fadeIn = i > 0 ? CROSSFADE : 0;
        const fadeOut = i < scenes.length - 1 ? CROSSFADE : 0;
        const seq = (
          <Sequence key={{i}} from={{offset}} durationInFrames={{dur}}>
            <div style={{{{
              width: "100%", height: "100%",
              opacity: 1,
            }}}}>
              {{renderScene(scene)}}
            </div>
          </Sequence>
        );
        offset += dur - (i < scenes.length - 1 ? CROSSFADE : 0);
        return seq;
      }})}}
    </div>
  );
}};
"##);
    std::fs::write(src_dir.join("MotionVideo.tsx"), &motion_video)
        .map_err(|e| NyayaError::Config(format!("write MotionVideo.tsx: {}", e)))?;

    // --- src/Root.tsx ---
    let root_tsx = format!(r##"import {{ Composition }} from "remotion";
import {{ MotionVideo }} from "./MotionVideo";
import {{ VideoScene }} from "./types";
import scenesData from "../scenes-data.json";

const CROSSFADE = 15;

export const RemotionRoot: React.FC = () => {{
  const scenes = scenesData as VideoScene[];
  const totalFrames = scenes.reduce((acc, s, i) =>
    acc + s.durationFrames - (i < scenes.length - 1 ? CROSSFADE : 0), 0);

  return (
    <Composition
      id="MotionVideo"
      component={{MotionVideo}}
      durationInFrames={{totalFrames}}
      fps={{30}}
      width={{1920}}
      height={{1080}}
      defaultProps={{{{ scenes }}}}
    />
  );
}};
"##);
    std::fs::write(src_dir.join("Root.tsx"), &root_tsx)
        .map_err(|e| NyayaError::Config(format!("write Root.tsx: {}", e)))?;

    // --- src/index.ts ---
    let index_ts = r#"import { registerRoot } from "remotion";
import { RemotionRoot } from "./Root";
registerRoot(RemotionRoot);
"#;
    std::fs::write(src_dir.join("index.ts"), index_ts)
        .map_err(|e| NyayaError::Config(format!("write index.ts: {}", e)))?;

    // --- Install and render ---
    eprintln!("[pea/doc] installing PixiJS + Remotion dependencies...");
    let install = std::process::Command::new("npm")
        .arg("install")
        .current_dir(&remotion_dir)
        .output()
        .map_err(|e| NyayaError::Config(format!("npm install: {}", e)))?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        return Err(NyayaError::Config(format!("npm install failed: {}", stderr)));
    }

    std::fs::create_dir_all(remotion_dir.join("out"))
        .map_err(|e| NyayaError::Config(format!("create out dir: {}", e)))?;

    let output_mp4 = output_dir.join("output.mp4");

    eprintln!("[pea/doc] rendering PixiJS motion graphics video...");
    let render = std::process::Command::new("npx")
        .args([
            "remotion", "render",
            "--codec=h264",
            "--crf=17",
            "--jpeg-quality=95",
            "--color-space=bt709",
            "--gl=angle",
        ])
        .arg(format!("--props={}", props_path.display()))
        .args(["src/index.ts", "MotionVideo"])
        .arg(&output_mp4)
        .current_dir(&remotion_dir)
        .output()
        .map_err(|e| NyayaError::Config(format!("remotion render: {}", e)))?;

    if render.status.success() && output_mp4.exists() {
        eprintln!("[pea/doc] PixiJS render complete: {}", output_mp4.display());
        let _ = std::fs::remove_dir_all(remotion_dir.join("node_modules"));
        return Ok(output_mp4);
    }

    let stderr = String::from_utf8_lossy(&render.stderr);
    let stdout = String::from_utf8_lossy(&render.stdout);
    Err(NyayaError::Config(format!(
        "PixiJS Remotion render failed:\nstdout: {}\nstderr: {}",
        &stdout[..stdout.len().min(500)],
        &stderr[..stderr.len().min(500)],
    )))
}

// ── PixiJS Scene Component Writers ─────────────────────────────────────────

fn css_to_hex_int(css: &str) -> String {
    let hex = css.trim_start_matches('#');
    format!("0x{}", hex)
}

fn write_pixi_scene_opener(dir: &Path, primary: &str, accent: &str) -> Result<()> {
    let pc = css_to_hex_int(primary);
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";
import {{ spring }} from "../easing";

interface Props {{ title: string; subtitle: string; mood: string; }}

export const OpenerScene: React.FC<Props> = ({{ title, subtitle }}) => {{
  const particles: {{ x: number; y: number; vx: number; vy: number; r: number }}[] = [];

  const setup = useCallback((app: Application) => {{
    // Create 200 particles for radial burst
    const g = new Graphics();
    app.stage.addChild(g);
    for (let i = 0; i < 200; i++) {{
      const angle = (Math.PI * 2 * i) / 200 + Math.random() * 0.3;
      particles.push({{
        x: 960, y: 540,
        vx: Math.cos(angle) * (2 + Math.random() * 4),
        vy: Math.sin(angle) * (2 + Math.random() * 4),
        r: 1 + Math.random() * 3,
      }});
    }}
    // Title
    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 72, fontWeight: "bold", fill: {ac}, wordWrap: true, wordWrapWidth: 1600, align: "center" }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5); t.x = 960; t.y = 480; t.label = "title";
    app.stage.addChild(t);
    // Subtitle
    const subStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 36, fill: 0xc8c8d2, wordWrap: true, wordWrapWidth: 1400, align: "center" }});
    const s = new Text({{ text: subtitle, style: subStyle }});
    s.anchor.set(0.5); s.x = 960; s.y = 620; s.label = "subtitle"; s.alpha = 0;
    app.stage.addChild(s);
  }}, [title, subtitle]);

  const update = useCallback((app: Application, progress: number) => {{
    // Particles: radial burst with friction
    const g = app.stage.children[0] as Graphics;
    g.clear();
    for (const p of particles) {{
      p.x += p.vx; p.y += p.vy;
      p.vx *= 0.97; p.vy *= 0.97;
      g.circle(p.x, p.y, p.r * (1 - progress * 0.5));
      g.fill({{ color: {pc}, alpha: 0.6 * (1 - progress * 0.7) }});
    }}
    // Title spring scale
    const titleEl = app.stage.children.find((c: any) => c.label === "title");
    if (titleEl) {{
      const s = 0.3 + 0.7 * spring(progress * 2);
      titleEl.scale.set(Math.min(s, 1));
    }}
    // Subtitle fade in at 40%
    const subEl = app.stage.children.find((c: any) => c.label === "subtitle");
    if (subEl) {{
      subEl.alpha = Math.max(0, Math.min(1, (progress - 0.4) * 3));
    }}
  }}, []);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("opener.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write opener.tsx: {}", e)))
}

fn write_pixi_scene_kinetic_text(dir: &Path, _primary: &str, accent: &str) -> Result<()> {
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback, useRef }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Text, TextStyle }} from "pixi.js";
import {{ spring, stagger }} from "../easing";

interface Props {{ text: string; layout: string; }}

export const KineticTextScene: React.FC<Props> = ({{ text, layout }}) => {{
  const wordsRef = useRef<string[]>([]);

  const setup = useCallback((app: Application) => {{
    // Parse words, detecting **bold** markers
    const raw = text.split(/\s+/);
    wordsRef.current = raw;
    const cols = Math.ceil(Math.sqrt(raw.length * 1.5));
    raw.forEach((word, i) => {{
      const isBold = word.startsWith("**") && word.endsWith("**");
      const clean = isBold ? word.slice(2, -2) : word;
      const style = new TextStyle({{
        fontFamily: "Arial",
        fontSize: isBold ? 56 : 40,
        fontWeight: isBold ? "bold" : "normal",
        fill: isBold ? {ac} : 0xc8c8d2,
      }});
      const t = new Text({{ text: clean, style }});
      t.anchor.set(0.5);
      // Target grid position
      const col = i % cols;
      const row = Math.floor(i / cols);
      const targetX = 200 + col * (1520 / cols);
      const targetY = 200 + row * 80;
      t.x = targetX; t.y = targetY;
      (t as any).targetX = targetX;
      (t as any).targetY = targetY;
      (t as any).wordIndex = i;

      // Initial position based on layout
      if (layout === "cascade") {{ t.y = -100; }}
      else if (layout === "converge") {{
        const edge = i % 4;
        t.x = edge === 0 ? -200 : edge === 1 ? 2120 : targetX;
        t.y = edge === 2 ? -200 : edge === 3 ? 1280 : targetY;
      }}
      else if (layout === "typewriter") {{ t.alpha = 0; }}

      app.stage.addChild(t);
    }});
  }}, [text, layout]);

  const update = useCallback((app: Application, progress: number) => {{
    const total = wordsRef.current.length;
    app.stage.children.forEach((child: any) => {{
      if (child.wordIndex === undefined) return;
      const i = child.wordIndex;
      const p = stagger(i, total, progress, 0.4);
      const s = spring(p);

      if (layout === "cascade") {{
        child.y = -100 + ((child as any).targetY + 100) * s;
      }} else if (layout === "converge") {{
        child.x = child.x + ((child as any).targetX - child.x) * s * 0.3;
        child.y = child.y + ((child as any).targetY - child.y) * s * 0.3;
      }} else if (layout === "wave") {{
        child.scale.set(0.8 + 0.4 * Math.sin(progress * Math.PI * 4 + i * 0.5));
      }} else if (layout === "typewriter") {{
        child.alpha = p > 0.1 ? 1 : 0;
      }}
    }});
  }}, [layout]);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("kinetic-text.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write kinetic-text.tsx: {}", e)))
}

fn write_pixi_scene_data_counter(dir: &Path, _primary: &str, accent: &str) -> Result<()> {
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Text, TextStyle, Graphics }} from "pixi.js";
import {{ easeOutExpo }} from "../easing";
import {{ CounterEntry }} from "../types";

interface Props {{ title: string; counters: CounterEntry[]; }}

export const DataCounterScene: React.FC<Props> = ({{ title, counters }}) => {{
  const setup = useCallback((app: Application) => {{
    // Title
    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 48, fontWeight: "bold", fill: 0xc8c8d2 }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5, 0); t.x = 960; t.y = 60;
    app.stage.addChild(t);

    const spacing = 1920 / (counters.length + 1);
    counters.forEach((c, i) => {{
      const x = spacing * (i + 1);
      // Arc background
      const g = new Graphics(); g.label = `arc_${{i}}`;
      app.stage.addChild(g);
      (g as any).cx = x; (g as any).cy = 450; (g as any).radius = 120;
      // Value text
      const valStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 64, fontWeight: "bold", fill: {ac} }});
      const vt = new Text({{ text: "0", style: valStyle }});
      vt.anchor.set(0.5); vt.x = x; vt.y = 450; vt.label = `val_${{i}}`;
      (vt as any).target = parseFloat(c.value.replace(/[^0-9.]/g, "")) || 0;
      (vt as any).unit = c.unit || "";
      (vt as any).rawValue = c.value;
      app.stage.addChild(vt);
      // Label
      const lblStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 28, fill: 0xc8c8d2, wordWrap: true, wordWrapWidth: 300, align: "center" }});
      const lt = new Text({{ text: c.label, style: lblStyle }});
      lt.anchor.set(0.5); lt.x = x; lt.y = 600;
      app.stage.addChild(lt);
    }});
  }}, [title, counters]);

  const update = useCallback((app: Application, progress: number) => {{
    const ease = easeOutExpo(Math.min(progress * 1.5, 1));
    app.stage.children.forEach((child: any) => {{
      // Update arcs
      if (child.label?.startsWith("arc_")) {{
        const g = child as Graphics;
        g.clear();
        g.arc(child.cx, child.cy, child.radius, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * ease);
        g.stroke({{ color: {ac}, width: 6, alpha: 0.4 }});
      }}
      // Update counter values
      if (child.label?.startsWith("val_")) {{
        const target = child.target;
        const current = Math.floor(ease * target);
        child.text = `${{current.toLocaleString()}}${{child.unit}}`;
      }}
    }});
  }}, []);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("data-counter.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write data-counter.tsx: {}", e)))
}

fn write_pixi_scene_bar_race(dir: &Path, _primary: &str, accent: &str) -> Result<()> {
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";
import {{ easeOutCubic, stagger }} from "../easing";
import {{ BarEntry }} from "../types";

interface Props {{ title: string; bars: BarEntry[]; }}

export const BarRaceScene: React.FC<Props> = ({{ title, bars }}) => {{
  const setup = useCallback((app: Application) => {{
    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 44, fontWeight: "bold", fill: 0xc8c8d2 }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5, 0); t.x = 960; t.y = 40;
    app.stage.addChild(t);

    const maxVal = Math.max(...bars.map(b => b.value), 1);
    const barHeight = Math.min(60, 800 / bars.length);
    const gap = 10;
    const startY = 140;

    bars.forEach((b, i) => {{
      const y = startY + i * (barHeight + gap);
      // Label
      const lblStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 22, fill: 0xc8c8d2 }});
      const lbl = new Text({{ text: b.label, style: lblStyle }});
      lbl.anchor.set(1, 0.5); lbl.x = 280; lbl.y = y + barHeight / 2;
      app.stage.addChild(lbl);
      // Bar graphic
      const g = new Graphics(); g.label = `bar_${{i}}`;
      (g as any).barY = y; (g as any).barH = barHeight;
      (g as any).maxWidth = 1500 * (b.value / maxVal);
      (g as any).barIndex = i;
      app.stage.addChild(g);
      // Value label
      const valStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 20, fontWeight: "bold", fill: 0xc8c8d2 }});
      const vt = new Text({{ text: b.value.toLocaleString(), style: valStyle }});
      vt.anchor.set(0, 0.5); vt.y = y + barHeight / 2;
      vt.label = `bval_${{i}}`; (vt as any).barIndex = i;
      app.stage.addChild(vt);
    }});
  }}, [title, bars]);

  const update = useCallback((app: Application, progress: number) => {{
    const total = bars.length;
    app.stage.children.forEach((child: any) => {{
      if (child.label?.startsWith("bar_")) {{
        const g = child as Graphics;
        const p = easeOutCubic(stagger(child.barIndex, total, progress));
        const w = child.maxWidth * p;
        g.clear();
        g.roundRect(300, child.barY, w, child.barH, 4);
        g.fill({{ color: {ac}, alpha: 0.85 }});
      }}
      if (child.label?.startsWith("bval_")) {{
        const p = easeOutCubic(stagger(child.barIndex, total, progress));
        const barChild = app.stage.children.find((c: any) => c.label === `bar_${{child.barIndex}}`);
        if (barChild) child.x = 310 + (barChild as any).maxWidth * p;
      }}
    }});
  }}, [bars]);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("bar-race.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write bar-race.tsx: {}", e)))
}

fn write_pixi_scene_particle_mood(dir: &Path, primary: &str, _accent: &str) -> Result<()> {
    let pc = css_to_hex_int(primary);
    let code = format!(r##"import React, {{ useCallback, useRef }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";

interface Props {{ text: string; preset: string; }}

interface Particle {{ x: number; y: number; vx: number; vy: number; r: number; alpha: number; }}

export const ParticleMoodScene: React.FC<Props> = ({{ text, preset }}) => {{
  const particlesRef = useRef<Particle[]>([]);

  const setup = useCallback((app: Application) => {{
    const count = preset === "nebula" ? 300 : preset === "fireflies" ? 80 : 150;
    const parts: Particle[] = [];
    for (let i = 0; i < count; i++) {{
      parts.push({{
        x: Math.random() * 1920,
        y: Math.random() * 1080,
        vx: (Math.random() - 0.5) * (preset === "rain" ? 0.5 : 2),
        vy: preset === "rain" ? 2 + Math.random() * 3 : preset === "snow" ? 0.5 + Math.random() : (Math.random() - 0.5) * 2,
        r: preset === "nebula" ? 1 + Math.random() * 6 : 1 + Math.random() * 3,
        alpha: 0.3 + Math.random() * 0.7,
      }});
    }}
    particlesRef.current = parts;

    const g = new Graphics(); g.label = "particles";
    app.stage.addChild(g);

    const textStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 52, fontWeight: "bold", fill: 0xffffff, wordWrap: true, wordWrapWidth: 1400, align: "center" }});
    const t = new Text({{ text, style: textStyle }});
    t.anchor.set(0.5); t.x = 960; t.y = 540; t.label = "text";
    app.stage.addChild(t);
  }}, [text, preset]);

  const update = useCallback((app: Application, progress: number) => {{
    const g = app.stage.children.find((c: any) => c.label === "particles") as Graphics;
    if (!g) return;
    g.clear();
    for (const p of particlesRef.current) {{
      p.x += p.vx; p.y += p.vy;
      if (preset === "fireflies") {{
        p.x += Math.sin(progress * 20 + p.y * 0.01) * 0.5;
        p.alpha = 0.3 + 0.7 * Math.abs(Math.sin(progress * 8 + p.x * 0.01));
      }}
      // Wrap around
      if (p.x < -10) p.x = 1930; if (p.x > 1930) p.x = -10;
      if (p.y < -10) p.y = 1090; if (p.y > 1090) p.y = -10;
      g.circle(p.x, p.y, p.r);
      g.fill({{ color: {pc}, alpha: p.alpha * (preset === "nebula" ? 0.4 : 0.7) }});
    }}
    // Text pulse
    const t = app.stage.children.find((c: any) => c.label === "text");
    if (t) {{ t.alpha = 0.7 + 0.3 * Math.sin(progress * Math.PI * 2); }}
  }}, [preset]);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("particle-mood.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write particle-mood.tsx: {}", e)))
}

fn write_pixi_scene_timeline_path(dir: &Path, _primary: &str, accent: &str) -> Result<()> {
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";
import {{ spring }} from "../easing";
import {{ WaypointEntry }} from "../types";

interface Props {{ title: string; waypoints: WaypointEntry[]; }}

export const TimelinePathScene: React.FC<Props> = ({{ title, waypoints }}) => {{
  const setup = useCallback((app: Application) => {{
    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 44, fontWeight: "bold", fill: 0xc8c8d2 }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5, 0); t.x = 960; t.y = 40;
    app.stage.addChild(t);

    // Timeline line
    const line = new Graphics(); line.label = "line";
    app.stage.addChild(line);

    // Waypoints
    const spacing = 1600 / (waypoints.length + 1);
    waypoints.forEach((wp, i) => {{
      const x = 160 + spacing * (i + 1);
      // Dot
      const dot = new Graphics(); dot.label = `dot_${{i}}`;
      (dot as any).wpX = x; (dot as any).wpIndex = i;
      app.stage.addChild(dot);
      // Year label
      const yearStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 24, fontWeight: "bold", fill: {ac} }});
      const yt = new Text({{ text: wp.year, style: yearStyle }});
      yt.anchor.set(0.5); yt.x = x; yt.y = 510; yt.alpha = 0;
      yt.label = `year_${{i}}`; (yt as any).wpIndex = i;
      app.stage.addChild(yt);
      // Description
      const descStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 20, fill: 0xc8c8d2, wordWrap: true, wordWrapWidth: spacing - 20, align: "center" }});
      const dt = new Text({{ text: wp.label, style: descStyle }});
      dt.anchor.set(0.5, 0); dt.x = x; dt.y = 560; dt.alpha = 0;
      dt.label = `desc_${{i}}`; (dt as any).wpIndex = i;
      app.stage.addChild(dt);
    }});
  }}, [title, waypoints]);

  const update = useCallback((app: Application, progress: number) => {{
    const total = waypoints.length;
    // Draw progressive line
    const line = app.stage.children.find((c: any) => c.label === "line") as Graphics;
    if (line) {{
      line.clear();
      const lineProgress = Math.min(progress * 1.3, 1);
      const endX = 160 + 1600 * lineProgress;
      line.moveTo(160, 480); line.lineTo(endX, 480);
      line.stroke({{ color: 0x555577, width: 3 }});
    }}

    app.stage.children.forEach((child: any) => {{
      if (child.label?.startsWith("dot_")) {{
        const g = child as Graphics;
        const i = child.wpIndex;
        const wpProgress = Math.min(progress * 1.3, 1);
        const threshold = (i + 1) / (total + 1);
        const visible = wpProgress >= threshold;
        g.clear();
        if (visible) {{
          const s = spring(Math.min((wpProgress - threshold) * (total + 1) * 2, 1));
          g.circle(child.wpX, 480, 8 * s);
          g.fill({{ color: {ac} }});
        }}
      }}
      if (child.label?.startsWith("year_") || child.label?.startsWith("desc_")) {{
        const i = child.wpIndex;
        const wpProgress = Math.min(progress * 1.3, 1);
        const threshold = (i + 1) / (total + 1);
        child.alpha = wpProgress >= threshold ? spring(Math.min((wpProgress - threshold) * (total + 1) * 2, 1)) : 0;
      }}
    }});
  }}, [waypoints]);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("timeline-path.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write timeline-path.tsx: {}", e)))
}

fn write_pixi_scene_comparison_split(dir: &Path, _primary: &str, accent: &str) -> Result<()> {
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";
import {{ spring, stagger }} from "../easing";
import {{ ComparisonPoint }} from "../types";

interface Props {{ title: string; leftLabel: string; rightLabel: string; points: ComparisonPoint[]; }}

export const ComparisonSplitScene: React.FC<Props> = ({{ title, leftLabel, rightLabel, points }}) => {{
  const setup = useCallback((app: Application) => {{
    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 44, fontWeight: "bold", fill: 0xc8c8d2 }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5, 0); t.x = 960; t.y = 40;
    app.stage.addChild(t);

    // Divider
    const div = new Graphics(); div.label = "divider";
    app.stage.addChild(div);

    // Column headers
    const headerStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 32, fontWeight: "bold", fill: {ac} }});
    const lh = new Text({{ text: leftLabel, style: headerStyle }});
    lh.anchor.set(0.5); lh.x = 480; lh.y = 140; lh.label = "leftHeader";
    app.stage.addChild(lh);
    const rh = new Text({{ text: rightLabel, style: headerStyle }});
    rh.anchor.set(0.5); rh.x = 1440; rh.y = 140; rh.label = "rightHeader";
    app.stage.addChild(rh);

    // Points
    points.forEach((p, i) => {{
      const y = 220 + i * 80;
      const leftStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 24, fill: 0xc8c8d2, wordWrap: true, wordWrapWidth: 400, align: "center" }});
      const lt = new Text({{ text: p.left, style: leftStyle }});
      lt.anchor.set(0.5, 0); lt.x = 480; lt.y = y; lt.alpha = 0;
      lt.label = `left_${{i}}`; (lt as any).pointIndex = i; (lt as any).side = "left";
      app.stage.addChild(lt);
      const rt = new Text({{ text: p.right, style: leftStyle }});
      rt.anchor.set(0.5, 0); rt.x = 1440; rt.y = y; rt.alpha = 0;
      rt.label = `right_${{i}}`; (rt as any).pointIndex = i; (rt as any).side = "right";
      app.stage.addChild(rt);
    }});
  }}, [title, leftLabel, rightLabel, points]);

  const update = useCallback((app: Application, progress: number) => {{
    // Divider slide in
    const div = app.stage.children.find((c: any) => c.label === "divider") as Graphics;
    if (div) {{
      div.clear();
      const h = 900 * spring(Math.min(progress * 3, 1));
      div.moveTo(960, 120); div.lineTo(960, 120 + h);
      div.stroke({{ color: {ac}, width: 2, alpha: 0.5 }});
    }}

    const total = points.length;
    app.stage.children.forEach((child: any) => {{
      if (child.pointIndex !== undefined) {{
        const i = child.pointIndex;
        const p = stagger(i, total, Math.max(0, progress - 0.15), 0.3);
        const s = spring(p);
        child.alpha = s;
        const offset = child.side === "left" ? -100 * (1 - s) : 100 * (1 - s);
        child.x = (child.side === "left" ? 480 : 1440) + offset;
      }}
    }});
  }}, [points]);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("comparison-split.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write comparison-split.tsx: {}", e)))
}

fn write_pixi_scene_closing(dir: &Path, primary: &str, accent: &str) -> Result<()> {
    let pc = css_to_hex_int(primary);
    let ac = css_to_hex_int(accent);
    let code = format!(r##"import React, {{ useCallback, useRef }} from "react";
import {{ PixiCanvas }} from "../pixi-canvas";
import {{ Application, Graphics, Text, TextStyle }} from "pixi.js";
import {{ spring }} from "../easing";

interface Props {{ title: string; subtitle: string; }}

export const ClosingScene: React.FC<Props> = ({{ title, subtitle }}) => {{
  const particlesRef = useRef<{{ x: number; y: number; vx: number; vy: number; r: number }}[]>([]);

  const setup = useCallback((app: Application) => {{
    // Particles that drift toward center (reverse of opener)
    const parts: {{ x: number; y: number; vx: number; vy: number; r: number }}[] = [];
    for (let i = 0; i < 150; i++) {{
      parts.push({{
        x: Math.random() * 1920, y: Math.random() * 1080,
        vx: (960 - Math.random() * 1920) * 0.002,
        vy: (540 - Math.random() * 1080) * 0.002,
        r: 1 + Math.random() * 3,
      }});
    }}
    particlesRef.current = parts;

    const g = new Graphics(); g.label = "particles";
    app.stage.addChild(g);

    const titleStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 64, fontWeight: "bold", fill: {ac}, wordWrap: true, wordWrapWidth: 1600, align: "center" }});
    const t = new Text({{ text: title, style: titleStyle }});
    t.anchor.set(0.5); t.x = 960; t.y = 460; t.label = "title";
    app.stage.addChild(t);

    const subStyle = new TextStyle({{ fontFamily: "Arial", fontSize: 32, fill: 0xc8c8d2, wordWrap: true, wordWrapWidth: 1400, align: "center" }});
    const s = new Text({{ text: subtitle, style: subStyle }});
    s.anchor.set(0.5); s.x = 960; s.y = 600; s.label = "subtitle"; s.alpha = 0;
    app.stage.addChild(s);
  }}, [title, subtitle]);

  const update = useCallback((app: Application, progress: number) => {{
    const g = app.stage.children.find((c: any) => c.label === "particles") as Graphics;
    if (g) {{
      g.clear();
      for (const p of particlesRef.current) {{
        p.x += p.vx; p.y += p.vy;
        g.circle(p.x, p.y, p.r);
        g.fill({{ color: {pc}, alpha: 0.4 + 0.3 * progress }});
      }}
    }}
    // Title spring up
    const t = app.stage.children.find((c: any) => c.label === "title");
    if (t) {{
      const s = 0.5 + 0.5 * spring(Math.min(progress * 2, 1));
      t.scale.set(s);
    }}
    // Subtitle fade
    const sub = app.stage.children.find((c: any) => c.label === "subtitle");
    if (sub) {{ sub.alpha = Math.max(0, Math.min(1, (progress - 0.3) * 3)); }}
  }}, []);

  return <PixiCanvas setup={{setup}} update={{update}} />;
}};
"##);
    std::fs::write(dir.join("closing.tsx"), &code)
        .map_err(|e| NyayaError::Config(format!("write closing.tsx: {}", e)))
}

/// Interactive slideshow HTML with keyboard navigation + auto-play.
fn generate_slideshow_html_fallback(
    objective_desc: &str,
    slides: &[SlideContent],
    primary: &str,
    accent: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    eprintln!("[pea/doc] generating slideshow HTML fallback");
    let mut slide_divs = String::new();
    for (i, slide) in slides.iter().enumerate() {
        let (slide_title, bullets) = slide_to_title_bullets(slide);
        let bullet_html: String = bullets
            .iter()
            .map(|b| format!("<li>{}</li>", html_escape(b)))
            .collect::<Vec<_>>()
            .join("\n");
        slide_divs.push_str(&format!(
            "<div class=\"slide\" id=\"slide-{i}\">\
             <h2>{title}</h2><ul>{bullets}</ul></div>\n",
            i = i,
            title = html_escape(&slide_title),
            bullets = bullet_html,
        ));
    }

    let total_slides = slides.len();
    let slideshow_html = format!(
        r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Slideshow</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: 'Helvetica Neue', Arial, sans-serif; background: #111; color: #fff; overflow: hidden; }}
.slide {{
  display: none; width: 100vw; height: 100vh;
  flex-direction: column; justify-content: center; align-items: center;
  background: linear-gradient(135deg, {primary} 0%, {accent} 100%);
  padding: 60px; text-align: center;
  animation: fadeIn 0.5s ease-in;
}}
.slide.active {{ display: flex; }}
.slide h2 {{ font-size: 3rem; margin-bottom: 2rem; }}
.slide ul {{ list-style: none; font-size: 1.5rem; line-height: 2.2; }}
.slide ul li::before {{ content: "→ "; opacity: 0.7; }}
.controls {{
  position: fixed; bottom: 30px; left: 50%; transform: translateX(-50%);
  display: flex; gap: 1rem; z-index: 10;
}}
.controls button {{
  padding: 0.5rem 1.5rem; font-size: 1rem; cursor: pointer;
  border: 2px solid rgba(255,255,255,0.5); background: rgba(0,0,0,0.3);
  color: #fff; border-radius: 6px;
}}
.controls button:hover {{ background: rgba(255,255,255,0.2); }}
.counter {{ position: fixed; bottom: 30px; right: 30px; color: rgba(255,255,255,0.5); font-size: 0.9rem; }}
@keyframes fadeIn {{ from {{ opacity: 0; }} to {{ opacity: 1; }} }}
</style>
</head>
<body>
{slides}
<div class="controls">
<button onclick="prev()">← Prev</button>
<button onclick="toggleAuto()">Auto</button>
<button onclick="next()">Next →</button>
</div>
<div class="counter" id="counter">1 / {total}</div>
<script>
let current = 0;
const total = {total};
let autoTimer = null;
function show(n) {{
  document.querySelectorAll('.slide').forEach(s => s.classList.remove('active'));
  current = ((n % total) + total) % total;
  document.getElementById('slide-' + current).classList.add('active');
  document.getElementById('counter').textContent = (current + 1) + ' / ' + total;
}}
function next() {{ show(current + 1); }}
function prev() {{ show(current - 1); }}
function toggleAuto() {{
  if (autoTimer) {{ clearInterval(autoTimer); autoTimer = null; }}
  else {{ autoTimer = setInterval(next, 5000); }}
}}
document.addEventListener('keydown', e => {{
  if (e.key === 'ArrowRight' || e.key === ' ') next();
  if (e.key === 'ArrowLeft') prev();
}});
show(0);
</script>
</body></html>"##,
        title = html_escape(objective_desc),
        primary = primary,
        accent = accent,
        slides = slide_divs,
        total = total_slides,
    );

    let html_path = output_dir.join("output.html");
    std::fs::write(&html_path, &slideshow_html)
        .map_err(|e| NyayaError::Config(format!("Failed to write slideshow HTML: {}", e)))?;
    Ok(html_path)
}

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
    fn test_sanitize_preserve_structure_keeps_documentclass() {
        let input = "\\documentclass{article}\n\\usepackage{graphicx}\n\\usepackage{pgfornament}\n\\begin{document}\nHello\n\\end{document}";
        let result = sanitize_latex_preserve_structure(input);
        assert!(result.contains("\\documentclass"));
        assert!(result.contains("\\begin{document}"));
        assert!(result.contains("\\end{document}"));
        assert!(result.contains("Hello"));
        assert!(!result.contains("pgfornament"));
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

    // --- LaTeX lint tests ---

    #[test]
    fn test_lint_unresolved_refs() {
        let tex = "See Figure ?? for details.\nAlso Table ?? shows data.";
        let errors = lint_latex(tex);
        assert_eq!(errors.iter().filter(|e| e.kind == "unresolved_ref").count(), 2);
    }

    #[test]
    fn test_lint_duplicate_chapters() {
        let tex = "\\chapter{Introduction}\nSome text.\n\\chapter{Introduction}\nMore text.";
        let errors = lint_latex(tex);
        assert!(errors.iter().any(|e| e.kind == "duplicate_chapter"));
    }

    #[test]
    fn test_lint_bare_url() {
        let tex = "Visit https://example.com for more info.";
        let errors = lint_latex(tex);
        assert!(errors.iter().any(|e| e.kind == "bare_url"));
    }

    #[test]
    fn test_lint_bare_url_not_in_href() {
        let tex = "Visit \\url{https://example.com} for more info.";
        let errors = lint_latex(tex);
        assert!(!errors.iter().any(|e| e.kind == "bare_url"));
    }

    #[test]
    fn test_auto_fix_wraps_urls() {
        let tex = "Visit https://example.com for more info.";
        let errors = lint_latex(tex);
        let fixed = auto_fix_lint(tex, &errors);
        assert!(fixed.contains("\\url{https://example.com}"));
        assert!(!fixed.contains(" https://example.com "));
    }

    #[test]
    fn test_lint_wide_tabular() {
        let tex = "\\begin{tabular}{l c c c c c c c}\ndata\n\\end{tabular}";
        let errors = lint_latex(tex);
        assert!(errors.iter().any(|e| e.kind == "wide_tabular"));
    }

    // --- Compile log QA tests ---

    #[test]
    fn test_analyse_log_ref_warnings() {
        let dir = std::env::temp_dir().join("nabaos_test_log_refs");
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("output.log");
        std::fs::write(&log_path, "LaTeX Warning: Reference `fig1' on page 3 undefined.\nLaTeX Warning: Reference `tab2' on page 5 undefined.\n").unwrap();
        let toc_path = dir.join("output.toc");
        std::fs::write(&toc_path, "\\contentsline {chapter}{Introduction}{1}").unwrap();

        let (warnings, critical) = analyse_compile_log(&log_path, &toc_path);
        assert!(warnings.iter().any(|w| w.contains("2 unresolved")));
        assert!(!critical); // only 2, threshold is >3
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_analyse_log_empty_toc() {
        let dir = std::env::temp_dir().join("nabaos_test_log_toc_empty");
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("output.log");
        std::fs::write(&log_path, "").unwrap();
        let toc_path = dir.join("output.toc");
        std::fs::write(&toc_path, "  \n").unwrap();

        let (warnings, critical) = analyse_compile_log(&log_path, &toc_path);
        assert!(warnings.iter().any(|w| w.contains("empty")));
        assert!(critical);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_analyse_log_present_toc() {
        let dir = std::env::temp_dir().join("nabaos_test_log_toc_ok");
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("output.log");
        std::fs::write(&log_path, "Output written on output.pdf").unwrap();
        let toc_path = dir.join("output.toc");
        std::fs::write(&toc_path, "\\contentsline {chapter}{Introduction}{1}\n\\contentsline {chapter}{Analysis}{5}").unwrap();

        let (warnings, _critical) = analyse_compile_log(&log_path, &toc_path);
        assert!(!warnings.iter().any(|w| w.contains("empty")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- PRISMA linkage + figure wrapping tests ---

    #[test]
    fn test_wrap_bare_includegraphics() {
        let tex = "Some text\n\\includegraphics[width=0.8\\textwidth]{prisma_flow.png}\nMore text";
        let images: Vec<ImageEntry> = vec![
            ("PRISMA Flow Diagram".into(), PathBuf::from("prisma_flow.png"), None),
        ];
        let result = wrap_bare_includegraphics(tex, &images);
        assert!(result.contains("\\begin{figure}"));
        assert!(result.contains("\\label{fig:auto1}"));
        assert!(result.contains("\\caption{PRISMA Flow Diagram}"));
        assert!(result.contains("\\end{figure}"));
    }

    #[test]
    fn test_wrap_skips_existing_figure_env() {
        let tex = "\\begin{figure}\n\\includegraphics{chart.png}\n\\end{figure}";
        let images: Vec<ImageEntry> = vec![];
        let result = wrap_bare_includegraphics(tex, &images);
        // Should NOT double-wrap
        assert_eq!(result.matches("\\begin{figure}").count(), 1);
    }

    #[test]
    fn test_auto_fix_resolves_figure_qq() {
        let tex = "See Figure ?? for the methodology.\nAlso Table ?? shows results.";
        let errors = lint_latex(tex);
        let fixed = auto_fix_lint(tex, &errors);
        assert!(!fixed.contains("Figure ??"));
        assert!(!fixed.contains("Table ??"));
        assert!(fixed.contains("the figure"));
        assert!(fixed.contains("the table"));
    }

    #[test]
    fn test_lint_duplicate_captions() {
        let tex = "\\caption{Solar Energy Growth}\nsome text\n\\caption{Solar Energy Growth}\n";
        let errors = lint_latex(tex);
        let dup_errors: Vec<_> = errors.iter().filter(|e| e.kind == "duplicate_caption").collect();
        assert_eq!(dup_errors.len(), 1);

        // Auto-fix should add sequential letters
        let fixed = auto_fix_lint(tex, &errors);
        assert!(fixed.contains("Solar Energy Growth (b)"));
    }

    #[test]
    fn test_lint_prompt_residue() {
        let tex = "\\caption{Photovoltaic power station Kenya rural electrification solar panels community development Africa 2022}\n";
        let errors = lint_latex(tex);
        let residue: Vec<_> = errors.iter().filter(|e| e.kind == "prompt_residue_caption").collect();
        assert_eq!(residue.len(), 1);
    }

    // --- Video slide extraction tests ---

    #[test]
    fn test_extract_slide_bullets_filters_urls() {
        let text = "Climate change is a major global challenge. \
            See https://www.ipcc.ch/report/ar6 for details. \
            Temperatures have risen by 1.1 degrees Celsius since pre-industrial times. \
            Visit www.nasa.gov/climate for more data.";
        let bullets = extract_slide_bullets(text);
        for b in &bullets {
            assert!(!b.contains("://"), "Bullet should not contain URL: {}", b);
            assert!(!b.contains("www."), "Bullet should not contain www: {}", b);
        }
    }

    #[test]
    fn test_extract_slide_bullets_filters_apa() {
        // Typical reference block with multiple APA entries
        let text = "Smith, J., Brown, A., & Lee, C. (2023). Climate impacts on agriculture. Journal of Science, Vol. 42, pp. 100-115.\n\
            Jones, R. et al. (2022). Global warming trends. Nature Reviews, Vol. 8, pp. 50-75.\n\
            Retrieved from https://doi.org/10.1234/example";
        let bullets = extract_slide_bullets(text);
        assert!(bullets.is_empty(), "APA references should produce no bullets, got: {:?}", bullets);
    }

    #[test]
    fn test_build_slides_skips_references_section() {
        let task_results = vec![
            ("Introduction".into(), "Renewable energy sources are becoming increasingly important for global sustainability efforts.".into()),
            ("References".into(), "Smith (2023). Solar Energy Review. Journal of Renewables, Vol. 15, pp. 1-20.".into()),
            ("Bibliography".into(), "Jones et al. (2022). Wind Power Analysis. Energy Reports, Vol. 8, pp. 50-75.".into()),
        ];
        let slides = build_slides("Test Objective", &task_results);
        // Extract titles from SlideContent
        let titles: Vec<String> = slides.iter().map(|s| match s {
            SlideContent::Title { title, .. } => title.clone(),
            SlideContent::Content { title, .. } => title.clone(),
            SlideContent::Timeline { title, .. } => title.clone(),
            SlideContent::Stats { title, .. } => title.clone(),
            SlideContent::Quote { text, .. } => text.clone(),
            SlideContent::Image { caption, .. } => caption.clone(),
            SlideContent::Closing { title, .. } => title.clone(),
        }).collect();
        // References and Bibliography should not appear as content slide titles
        assert!(!titles.contains(&"References".to_string()), "References section should be filtered: {:?}", titles);
        assert!(!titles.contains(&"Bibliography".to_string()), "Bibliography section should be filtered: {:?}", titles);
        // Title and closing should always be present
        assert_eq!(titles[0], "Test Objective");
        assert_eq!(titles.last().unwrap(), "Thank You");
        // First slide should be Title kind, last should be Closing
        assert!(matches!(slides[0], SlideContent::Title { .. }));
        assert!(matches!(slides.last().unwrap(), SlideContent::Closing { .. }));
    }

    #[test]
    fn test_strip_markdown() {
        assert_eq!(strip_markdown("### Header Text"), "Header Text");
        assert_eq!(strip_markdown("**bold text**"), "bold text");
        assert_eq!(strip_markdown("*italic text*"), "italic text");
        assert_eq!(strip_markdown("`code`"), "code");
        assert_eq!(strip_markdown("[link text](http://example.com)"), "link text");
        assert_eq!(strip_markdown("![alt](image.png)"), "");
        assert_eq!(strip_markdown("~~struck~~"), "struck");
        assert_eq!(strip_markdown("__also bold__"), "also bold");
        assert_eq!(strip_markdown("Normal text stays"), "Normal text stays");
        // Mixed formatting
        assert_eq!(strip_markdown("### **Bold Header**"), "Bold Header");
    }

    #[test]
    fn test_extract_timeline_events() {
        let text = "In 1979, the Iranian Revolution overthrew the Shah.\n\
                     2003: The US invaded Iraq, destabilizing the region.\n\
                     By 2024, tensions had escalated significantly between the two nations.";
        let events = extract_timeline_events(text);
        assert!(events.len() >= 2, "Should extract at least 2 events, got: {:?}", events);
        assert_eq!(events[0].0, "1979");
        assert_eq!(events[1].0, "2003");
    }

    #[test]
    fn test_extract_key_stats() {
        let text = "The war caused over 500,000 casualties on both sides.\n\
                     Economic damage estimated at $2.5 billion in direct costs.\n\
                     Approximately 45% of the population was displaced.";
        let stats = extract_key_stats(text);
        assert!(stats.len() >= 2, "Should extract at least 2 stats, got: {:?}", stats);
    }

    #[test]
    fn test_extract_inline_citations() {
        let text = "According to Smith (2023), the effect was significant. \
                     Other research (Jones, 2022) confirmed these findings. \
                     Brown et al. (2021) provided additional evidence.";
        let cites = extract_inline_citations(text);
        assert!(cites.len() >= 2, "Should extract at least 2 citations, got: {:?}", cites);
        assert!(cites.iter().any(|c| c.contains("Smith")));
        assert!(cites.iter().any(|c| c.contains("2023")));
    }

    #[test]
    fn test_build_slides_documentary_structure() {
        let task_results = vec![
            ("Historical Background".into(),
             "In 1979, the Iranian Revolution changed the political landscape. \
              By 2003, regional tensions had escalated dramatically. \
              In 2024, new conflicts emerged between the nations.".into()),
            ("Economic Impact".into(),
             "The conflict caused over 500,000 casualties across the region. \
              Economic damage estimated at $12 billion in infrastructure losses. \
              Approximately 35% of GDP was affected by sanctions.".into()),
        ];
        let slides = build_slides("Iran Israel War Analysis", &task_results);

        // Should have Title + Content + possibly Timeline/Stats + Closing
        assert!(slides.len() >= 4, "Should have at least 4 slides, got {}", slides.len());
        assert!(matches!(slides[0], SlideContent::Title { .. }));
        assert!(matches!(slides.last().unwrap(), SlideContent::Closing { .. }));

        // Check for variety — should have more than just Title/Content/Closing
        let kinds: Vec<&str> = slides.iter().map(|s| s.kind_str()).collect();
        assert!(kinds.contains(&"content"), "Should have content slides: {:?}", kinds);
    }

    #[test]
    fn test_slide_content_deserialize_roundtrip() {
        let slides = vec![
            SlideContent::Title {
                title: "Test".into(),
                subtitle: "Sub".into(),
                duration_frames: 210,
            },
            SlideContent::Content {
                title: "Key Points".into(),
                bullets: vec!["Point 1".into(), "Point 2".into()],
                footnotes: vec![],
                duration_frames: 180,
            },
            SlideContent::Timeline {
                title: "History".into(),
                events: vec![TimelineEvent { date: "2020".into(), desc: "Event".into() }],
                duration_frames: 240,
            },
            SlideContent::Stats {
                title: "Numbers".into(),
                stats: vec![StatEntry { label: "Users".into(), value: "1M".into() }],
                duration_frames: 180,
            },
            SlideContent::Quote {
                text: "Quote text".into(),
                attribution: "Author".into(),
                duration_frames: 150,
            },
            SlideContent::Image {
                caption: "Photo".into(),
                filename: "photo.jpg".into(),
                attribution: "CC".into(),
                duration_frames: 150,
            },
            SlideContent::Closing {
                title: "Thanks".into(),
                subtitle: "End".into(),
                duration_frames: 180,
            },
        ];

        let json = serde_json::to_string(&slides).unwrap();
        let parsed: Vec<SlideContent> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 7);
        assert_eq!(parsed[0].kind_str(), "title");
        assert_eq!(parsed[1].kind_str(), "content");
        assert_eq!(parsed[2].kind_str(), "timeline");
        assert_eq!(parsed[3].kind_str(), "stats");
        assert_eq!(parsed[4].kind_str(), "quote");
        assert_eq!(parsed[5].kind_str(), "image");
        assert_eq!(parsed[6].kind_str(), "closing");
    }

    #[test]
    fn test_slide_content_deserialize_without_duration() {
        // LLM might omit durationFrames — defaults should kick in
        let json = r#"[{"kind":"title","title":"Test","subtitle":"Sub"}]"#;
        let parsed: Vec<SlideContent> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].duration_frames(), 210); // default_duration_title
    }

    #[test]
    fn test_looks_like_slide_noise() {
        assert!(looks_like_slide_noise("https://example.com/page"));
        assert!(looks_like_slide_noise("See www.example.org for details"));
        assert!(looks_like_slide_noise("doi:10.1234/test.5678"));
        assert!(looks_like_slide_noise("Smith et al. (2023)"));
        assert!(looks_like_slide_noise("Vol. 42, pp. 100-115"));
        assert!(!looks_like_slide_noise("Climate change affects global temperatures significantly"));
    }

    // ── VideoScene Tests ───────────────────────────────────────────────────

    #[test]
    fn test_video_scene_deserialize_roundtrip() {
        let json = r#"[
            {"kind":"opener","title":"Rise of EVs","subtitle":"A visual journey","mood":"dramatic","durationFrames":240},
            {"kind":"kineticText","text":"Electric vehicles are **transforming** the **automotive** industry","layout":"cascade","durationFrames":180},
            {"kind":"dataCounter","title":"Key Numbers","counters":[{"label":"Global EV Sales","value":"14000000","unit":" units"}],"durationFrames":210},
            {"kind":"barRace","title":"Top Markets","bars":[{"label":"China","value":8100000},{"label":"Europe","value":3200000}],"durationFrames":210},
            {"kind":"particleMood","text":"A cleaner future awaits","preset":"nebula","durationFrames":150},
            {"kind":"timelinePath","title":"EV Timeline","waypoints":[{"year":"1996","label":"GM EV1"},{"year":"2008","label":"Tesla Roadster"}],"durationFrames":270},
            {"kind":"comparisonSplit","title":"ICE vs EV","leftLabel":"Gasoline","rightLabel":"Electric","points":[{"left":"$3000/yr fuel","right":"$500/yr charging"}],"durationFrames":240},
            {"kind":"closing","title":"The Future is Electric","subtitle":"Generated by NabaOS","durationFrames":210}
        ]"#;
        let scenes: Vec<VideoScene> = serde_json::from_str(json).unwrap();
        assert_eq!(scenes.len(), 8);
        assert_eq!(scenes[0].kind_str(), "opener");
        assert_eq!(scenes[1].kind_str(), "kineticText");
        assert_eq!(scenes[2].kind_str(), "dataCounter");
        assert_eq!(scenes[3].kind_str(), "barRace");
        assert_eq!(scenes[4].kind_str(), "particleMood");
        assert_eq!(scenes[5].kind_str(), "timelinePath");
        assert_eq!(scenes[6].kind_str(), "comparisonSplit");
        assert_eq!(scenes[7].kind_str(), "closing");

        // Roundtrip
        let serialized = serde_json::to_string(&scenes).unwrap();
        let roundtrip: Vec<VideoScene> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(roundtrip.len(), 8);
    }

    #[test]
    fn test_video_scene_defaults() {
        // Omitting durationFrames should use defaults
        let json = r#"[
            {"kind":"opener","title":"Test","subtitle":"Sub"},
            {"kind":"kineticText","text":"Hello world"},
            {"kind":"dataCounter","title":"Stats","counters":[]},
            {"kind":"closing","title":"End"}
        ]"#;
        let scenes: Vec<VideoScene> = serde_json::from_str(json).unwrap();
        assert_eq!(scenes[0].duration_frames(), 240); // default_opener_duration
        assert_eq!(scenes[1].duration_frames(), 180); // default_kinetic_duration
        assert_eq!(scenes[2].duration_frames(), 210); // default_counter_duration
        assert_eq!(scenes[3].duration_frames(), 210); // default_closing_motion_duration

        // Default layout and preset
        if let VideoScene::KineticText { layout, .. } = &scenes[1] {
            assert_eq!(layout, "cascade");
        }
    }

    #[test]
    fn test_validate_video_scenes_opener_closing() {
        // Missing Opener and Closing — should be inserted
        let mut scenes = vec![
            VideoScene::KineticText {
                text: "Hello".into(), layout: "wave".into(), duration_frames: 180,
            },
        ];
        validate_video_scenes(&mut scenes, "Test Topic");
        assert!(matches!(scenes.first(), Some(VideoScene::Opener { .. })));
        assert!(matches!(scenes.last(), Some(VideoScene::Closing { .. })));
        assert_eq!(scenes.len(), 3);
    }

    #[test]
    fn test_validate_video_scenes_clamps_count() {
        let mut scenes: Vec<VideoScene> = (0..25).map(|i| {
            VideoScene::KineticText {
                text: format!("Scene {}", i), layout: "cascade".into(), duration_frames: 180,
            }
        }).collect();
        validate_video_scenes(&mut scenes, "Test");
        assert!(scenes.len() <= 20);
        assert!(matches!(scenes.first(), Some(VideoScene::Opener { .. })));
        assert!(matches!(scenes.last(), Some(VideoScene::Closing { .. })));
    }

    #[test]
    fn test_validate_video_scenes_fixes_invalid_presets() {
        let mut scenes = vec![
            VideoScene::Opener { title: "T".into(), subtitle: "S".into(), mood: "epic".into(), duration_frames: 240 },
            VideoScene::KineticText { text: "Hello".into(), layout: "invalid_layout".into(), duration_frames: 180 },
            VideoScene::ParticleMood { text: "Mood".into(), preset: "invalid_preset".into(), duration_frames: 150 },
            VideoScene::Closing { title: "End".into(), subtitle: "Done".into(), duration_frames: 210 },
        ];
        validate_video_scenes(&mut scenes, "Test");

        if let VideoScene::KineticText { layout, .. } = &scenes[1] {
            assert_eq!(layout, "cascade"); // defaulted from invalid
        }
        if let VideoScene::ParticleMood { preset, .. } = &scenes[2] {
            assert_eq!(preset, "stars"); // defaulted from invalid
        }
    }

    #[test]
    fn test_validate_video_scenes_clamps_duration() {
        let mut scenes = vec![
            VideoScene::Opener { title: "T".into(), subtitle: "S".into(), mood: "".into(), duration_frames: 30 }, // too low
            VideoScene::KineticText { text: "X".into(), layout: "cascade".into(), duration_frames: 600 }, // too high
            VideoScene::Closing { title: "End".into(), subtitle: "".into(), duration_frames: 210 },
        ];
        validate_video_scenes(&mut scenes, "Test");

        assert_eq!(scenes[0].duration_frames(), 90);  // clamped up
        assert_eq!(scenes[1].duration_frames(), 360); // clamped down
    }
}
