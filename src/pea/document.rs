// PEA Document Generator — LLM-driven LaTeX/HTML document assembly.
//
// The LLM generates the full LaTeX source from task results, choosing
// the appropriate document structure, packages, and formatting for
// the content type (cookbook, survey paper, report, etc.).

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
}

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
        }
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
           ]\n\
         }}\n\n\
         Generate 3-8 image_queries that would enhance this document with relevant royalty-free photos. \
         Queries should be specific and descriptive (e.g., \"mughlai biryani dish overhead photography\" \
         not just \"food\"). Only include image_queries if images would genuinely enhance the content — \
         for pure technical docs or code references, use an empty array.",
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

/// Assemble all task results into a final document (PDF or HTML).
///
/// Returns the path to the generated output file.
pub fn assemble_document(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)], // (task_description, result_text)
    images: &[(String, PathBuf)],      // (caption, image_path)
    output_dir: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)
        .map_err(|e| NyayaError::Config(format!("Failed to create output dir: {}", e)))?;

    // 1. Generate LaTeX source via LLM
    let tex_source = generate_latex_source(registry, manifest, objective_desc, task_results, images)?;

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
    let html = generate_html_fallback(objective_desc, task_results, images);
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
// LaTeX generation via LLM
// ---------------------------------------------------------------------------

fn generate_latex_source(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[(String, PathBuf)],
) -> Result<String> {
    let sections = task_results
        .iter()
        .enumerate()
        .map(|(i, (desc, text))| format!("=== Section {} — {} ===\n{}", i + 1, desc, text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let image_list = if images.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = images
            .iter()
            .enumerate()
            .map(|(i, (caption, path))| {
                format!("Image {}: {} (file: {})", i + 1, caption, path.display())
            })
            .collect();
        format!("\n\nAvailable images:\n{}", items.join("\n"))
    };

    let prompt = format!(
        "Compile the following sections into a complete, professionally typeset LaTeX document.\n\n\
         Objective: {}\n\n\
         Content sections:\n{}\n{}\n\n\
         CRITICAL DEDUPLICATION RULES:\n\
         - Sections labeled \"[QA FIX — supersedes earlier content]\" are corrections from quality review. \
         When a QA FIX section covers the same topic as an earlier section, use ONLY the QA FIX version \
         and discard the original.\n\
         - Do NOT repeat the same information across multiple chapters. If multiple sections cover \
         overlapping ground, merge them into a single cohesive section.\n\
         - Remove duplicate executive summaries, introductions, and conclusions — the final document \
         should have exactly one of each.\n\n\
         Requirements:\n\
         - Use appropriate packages: geometry, fancyhdr, graphicx, xcolor, booktabs, tcolorbox, tikz, hyperref\n\
         - Include a title page with the objective as the title\n\
         - Include table of contents\n\
         - Organize content into well-structured chapters/sections\n\
         - Use professional formatting: headers, footers, proper margins\n\
         - Include any tables, lists, or structured data in proper LaTeX format\n\
         - If content mentions data suitable for visualization, include TikZ/pgfplots diagrams\n\
         - For images, use \\includegraphics with the image filenames\n\
         - Output ONLY the complete LaTeX source code, starting with \\documentclass and ending with \\end{{document}}",
        objective_desc, sections, image_list
    );

    let input = serde_json::json!({
        "system": LATEX_SYSTEM_PROMPT,
        "prompt": prompt,
    });

    let result = registry
        .execute_ability(manifest, "llm.chat", &input.to_string())
        .map_err(|e| NyayaError::Config(format!("LLM call for document assembly failed: {}", e)))?;

    let raw_output = String::from_utf8_lossy(&result.output).to_string();

    // Extract LaTeX source (strip markdown fences if present)
    Ok(extract_latex_source(&raw_output))
}

// ---------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------

fn postprocess_latex(tex: &str, images: &[(String, PathBuf)], output_dir: &Path) -> String {
    let mut result = tex.to_string();

    // Fix image paths: replace any absolute/relative paths with just filenames
    // since we'll copy images to output_dir
    for (_, path) in images {
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

/// Ask the LLM to fix LaTeX compilation errors.
fn diagnose_and_fix_latex(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    tex_source: &str,
    error_log: &str,
) -> Result<String> {
    let prompt = format!(
        "The following LaTeX document failed to compile. Fix the errors and return the COMPLETE corrected LaTeX source.\n\n\
         COMPILATION ERRORS:\n{}\n\n\
         ORIGINAL LATEX SOURCE:\n{}\n\n\
         Output ONLY the corrected LaTeX source, starting with \\documentclass and ending with \\end{{document}}. \
         Do not include any explanation.",
        error_log, tex_source
    );

    let input = serde_json::json!({
        "system": LATEX_SYSTEM_PROMPT,
        "prompt": prompt,
    });

    let result = registry
        .execute_ability(manifest, "llm.chat", &input.to_string())
        .map_err(|e| NyayaError::Config(format!("LLM call for LaTeX fix failed: {}", e)))?;

    let raw_output = String::from_utf8_lossy(&result.output).to_string();
    Ok(extract_latex_source(&raw_output))
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
    images: &[(String, PathBuf)],
) -> String {
    let escaped_title = html_escape(objective_desc);

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
    for (caption, path) in images {
        if let Some(filename) = path.file_name() {
            image_html.push_str(&format!(
                "<figure><img src=\"{}\" alt=\"{}\"><figcaption>{}</figcaption></figure>\n",
                filename.to_string_lossy(),
                html_escape(caption),
                html_escape(caption)
            ));
        }
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
body {{ font-family: 'Georgia', serif; max-width: 800px; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: #333; }}
h1 {{ text-align: center; border-bottom: 2px solid #333; padding-bottom: 0.5em; }}
h2 {{ color: #444; border-bottom: 1px solid #ddd; padding-bottom: 0.3em; }}
.content {{ margin: 1em 0; }}
figure {{ text-align: center; margin: 2em 0; }}
figure img {{ max-width: 100%; height: auto; }}
figcaption {{ font-style: italic; color: #666; margin-top: 0.5em; }}
section {{ margin-bottom: 2em; }}
.toc {{ background: #f9f9f9; padding: 1em 2em; border-radius: 4px; margin: 1em 0 2em; }}
.toc h3 {{ margin-top: 0; }}
.toc ol {{ padding-left: 1.5em; }}
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
<footer><p><em>Generated by NabaOS PEA Engine</em></p></footer>
</body>
</html>"#,
        title = escaped_title,
        toc = task_results
            .iter()
            .enumerate()
            .map(|(_, (desc, _))| format!("<li>{}</li>", html_escape(desc)))
            .collect::<Vec<_>>()
            .join("\n"),
        sections = sections,
        images = image_html,
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
with professional formatting. You use modern packages (geometry, fancyhdr, graphicx, \
xcolor, booktabs, tcolorbox, tikz, multicol, hyperref) and produce clean, well-structured \
documents. Always output ONLY the LaTeX source code. Never include explanation text outside \
the LaTeX document.";

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
        let html = generate_html_fallback("Test Document", &results, &[]);
        assert!(html.contains("<title>Test Document</title>"));
        assert!(html.contains("<h2>1. Introduction</h2>"));
        assert!(html.contains("<h2>2. Chapter 1</h2>"));
        assert!(html.contains("NabaOS PEA Engine"));
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
        let images = vec![
            ("A photo".to_string(), PathBuf::from("/tmp/images/photo.jpg")),
        ];
        // Use a temp dir that may not exist — postprocess handles copy failure gracefully
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
}
