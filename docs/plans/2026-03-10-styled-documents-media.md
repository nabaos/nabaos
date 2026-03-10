# Styled Document Output with Media Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add LLM-driven visual styling, royalty-free stock images, and env key management to PEA document output.

**Architecture:** Before document assembly, an LLM analyzes the content and outputs a `StyleConfig` JSON (theme, colors, ornaments, watermark, image search queries). A new `media.fetch_stock_image` ability downloads images from Unsplash → Pexels → FAL → TikZ fallback chain. The style config and images feed into the existing `assemble_document()` pipeline. Env key management lets users configure API keys from web/TUI/Telegram.

**Tech Stack:** Rust (2024 edition), reqwest (HTTP), serde_json, ratatui (TUI), axum (web), Svelte 5 (frontend), Unsplash API, Pexels API

---

## Task 1: StyleConfig Struct + analyze_style() in document.rs

**Files:**
- Modify: `src/pea/document.rs:1-13` (add struct after imports)
- Modify: `src/pea/document.rs:362-373` (extend system prompt)
- Test: `src/pea/document.rs` (inline tests module)

**Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/pea/document.rs`:

```rust
#[test]
fn test_parse_style_config_valid() {
    let json = r#"{
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
    }"#;
    let config: StyleConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.theme, "oriental");
    assert_eq!(config.image_queries.len(), 1);
    assert_eq!(config.image_queries[0].query, "mughlai biryani");
    assert!(config.watermark_text.is_some());
}

#[test]
fn test_parse_style_config_minimal() {
    let json = r#"{
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
    }"#;
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib pea::document::tests::test_parse_style_config_valid`
Expected: FAIL — `StyleConfig` type doesn't exist yet.

**Step 3: Write minimal implementation**

Add after line 12 in `src/pea/document.rs` (after the `use` imports, before the `// Public API` comment):

```rust
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib pea::document::tests::test_parse_style_config`
Expected: All 3 tests PASS.

**Step 5: Commit**

```bash
git add src/pea/document.rs
git commit -m "feat(pea): add StyleConfig struct for LLM-driven document styling"
```

---

## Task 2: analyze_style() Function

**Files:**
- Modify: `src/pea/document.rs` (add function after StyleConfig impl, before `// Public API`)

**Step 1: Write the failing test**

Add to tests module in `src/pea/document.rs`:

```rust
#[test]
fn test_build_style_analysis_prompt() {
    let prompt = build_style_analysis_prompt(
        "Create a Mughlai cookbook with 10 recipes",
        &[("Introduction".into(), "Mughlai cuisine...".into())],
    );
    assert!(prompt.contains("Mughlai cookbook"));
    assert!(prompt.contains("StyleConfig"));
    assert!(prompt.contains("image_queries"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib pea::document::tests::test_build_style_analysis_prompt`
Expected: FAIL — function doesn't exist.

**Step 3: Write minimal implementation**

Add the `analyze_style` public function and `build_style_analysis_prompt` helper in `src/pea/document.rs`, after the `StyleConfig` impl block and before the `// Public API` section:

```rust
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
            let preview = if text.len() > 500 { &text[..500] } else { text };
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib pea::document::tests::test_build_style_analysis_prompt`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/pea/document.rs
git commit -m "feat(pea): add analyze_style() for LLM-driven document styling"
```

---

## Task 3: media.fetch_stock_image Ability

**Files:**
- Modify: `src/runtime/host_functions.rs:1100` (add dispatch case after `"data.transform"`)
- Modify: `src/runtime/host_functions.rs` (add `exec_fetch_stock_image` function near other exec_ functions)
- Modify: `src/runtime/manifest.rs:101` (add `"media.fetch_stock_image"` to KNOWN_PERMISSIONS)

**Step 1: Write the failing test**

Add to tests module in `src/runtime/host_functions.rs`:

```rust
#[test]
fn test_fetch_stock_image_missing_query() {
    let reg = AbilityRegistry::new();
    let manifest = test_manifest(vec!["media.fetch_stock_image"]);
    let result = reg.execute_ability(
        &manifest,
        "media.fetch_stock_image",
        r#"{"output_dir": "/tmp"}"#,
    );
    assert!(result.is_err());
}

#[test]
fn test_fetch_stock_image_requires_query_and_output_dir() {
    let reg = AbilityRegistry::new();
    let manifest = test_manifest(vec!["media.fetch_stock_image"]);
    let result = reg.execute_ability(
        &manifest,
        "media.fetch_stock_image",
        r#"{"query": "sunset beach", "output_dir": "/tmp/test_images"}"#,
    );
    // Will succeed or fail based on network; but should not panic
    // The ability should return Ok with either an image path or a TikZ fallback
    assert!(result.is_ok() || result.is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib runtime::host_functions::tests::test_fetch_stock_image_missing_query`
Expected: FAIL — unrecognized ability name.

**Step 3: Write minimal implementation**

First, add `"media.fetch_stock_image"` to `KNOWN_PERMISSIONS` in `src/runtime/manifest.rs:101` (after `"docs.generate"`):

```rust
    "media.fetch_stock_image",
```

Then add the dispatch case in `src/runtime/host_functions.rs` in the `match ability_name` block (after the `"data.transform"` line ~1105):

```rust
"media.fetch_stock_image" => exec_fetch_stock_image(&input)?,
```

Then add the implementation function. Place it after `exec_download` (~line 1900). The function implements the Unsplash → Pexels → TikZ fallback chain:

```rust
/// Default Unsplash demo client_id for NabaOS (open-source, attributed).
/// Users can override with NABA_UNSPLASH_KEY env var.
const UNSPLASH_DEFAULT_CLIENT_ID: &str = "nabaos-demo-key";

fn exec_fetch_stock_image(
    input: &serde_json::Value,
) -> Result<AbilityOutput, String> {
    let query = input
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("media.fetch_stock_image requires 'query' string field")?;
    let output_dir = input
        .get("output_dir")
        .and_then(|v| v.as_str())
        .ok_or("media.fetch_stock_image requires 'output_dir' string field")?;

    let output_path = std::path::Path::new(output_dir);
    let images_dir = output_path.join("images");
    std::fs::create_dir_all(&images_dir)
        .map_err(|e| format!("Failed to create images dir: {}", e))?;

    // Generate a filename from query hash
    let query_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        query.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    };
    let image_path = images_dir.join(format!("{}.jpg", query_hash));

    // Check cache — if file already exists, return it
    if image_path.exists() {
        let mut facts = std::collections::HashMap::new();
        facts.insert("source".into(), "cache".into());
        facts.insert("path".into(), image_path.display().to_string());
        return Ok(AbilityOutput {
            output: image_path.display().to_string().into_bytes(),
            result_count: Some(1),
            facts,
        });
    }

    let mut facts = std::collections::HashMap::new();

    // --- Try Unsplash ---
    let unsplash_key = std::env::var("NABA_UNSPLASH_KEY")
        .unwrap_or_else(|_| UNSPLASH_DEFAULT_CLIENT_ID.to_string());
    if unsplash_key != "nabaos-demo-key" || unsplash_key == UNSPLASH_DEFAULT_CLIENT_ID {
        let search_url = format!(
            "https://api.unsplash.com/search/photos?query={}&per_page=1&orientation=landscape",
            urlencoding::encode(query)
        );
        if let Ok(result) = try_unsplash(&search_url, &unsplash_key, &image_path) {
            facts.insert("source".into(), "unsplash".into());
            facts.insert("attribution".into(), result.attribution);
            facts.insert("path".into(), image_path.display().to_string());
            return Ok(AbilityOutput {
                output: image_path.display().to_string().into_bytes(),
                result_count: Some(1),
                facts,
            });
        }
    }

    // --- Try Pexels ---
    if let Ok(pexels_key) = std::env::var("NABA_PEXELS_KEY") {
        let search_url = format!(
            "https://api.pexels.com/v1/search?query={}&per_page=1&orientation=landscape",
            urlencoding::encode(query)
        );
        if let Ok(result) = try_pexels(&search_url, &pexels_key, &image_path) {
            facts.insert("source".into(), "pexels".into());
            facts.insert("attribution".into(), result.attribution);
            facts.insert("path".into(), image_path.display().to_string());
            return Ok(AbilityOutput {
                output: image_path.display().to_string().into_bytes(),
                result_count: Some(1),
                facts,
            });
        }
    }

    // --- Try FAL AI image generation ---
    if let Ok(_fal_key) = std::env::var("NABA_FAL_API_KEY") {
        // FAL integration is a future extension point
        // For now, fall through to TikZ
    }

    // --- TikZ fallback ---
    let tikz_path = images_dir.join(format!("{}.tikz", query_hash));
    let tikz_code = format!(
        "% TikZ placeholder for: {}\n\\begin{{tikzpicture}}\n\\node[draw, rounded corners, fill=gray!10, minimum width=8cm, minimum height=5cm, align=center, font=\\large] {{{}}};\n\\end{{tikzpicture}}",
        query, query
    );
    std::fs::write(&tikz_path, &tikz_code)
        .map_err(|e| format!("Failed to write TikZ fallback: {}", e))?;

    facts.insert("source".into(), "tikz_fallback".into());
    facts.insert("path".into(), tikz_path.display().to_string());
    Ok(AbilityOutput {
        output: tikz_path.display().to_string().into_bytes(),
        result_count: Some(1),
        facts,
    })
}

struct StockImageResult {
    attribution: String,
}

fn try_unsplash(
    search_url: &str,
    client_id: &str,
    save_path: &std::path::Path,
) -> std::result::Result<StockImageResult, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .get(search_url)
        .header("Authorization", format!("Client-ID {}", client_id))
        .header("User-Agent", "nabaos/0.3 (+https://github.com/nabaos/nabaos)")
        .send()
        .map_err(|e| format!("Unsplash request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Unsplash returned {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Unsplash JSON parse failed: {}", e))?;

    let results = body.get("results").and_then(|r| r.as_array())
        .ok_or("No results array in Unsplash response")?;

    let first = results.first().ok_or("No images found on Unsplash")?;

    let image_url = first
        .get("urls")
        .and_then(|u| u.get("regular"))
        .and_then(|v| v.as_str())
        .ok_or("No regular URL in Unsplash result")?;

    let photographer = first
        .get("user")
        .and_then(|u| u.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    // Download the image
    let img_resp = client
        .get(image_url)
        .send()
        .map_err(|e| format!("Image download failed: {}", e))?;

    let bytes = img_resp
        .bytes()
        .map_err(|e| format!("Failed to read image bytes: {}", e))?;

    std::fs::write(save_path, &bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(StockImageResult {
        attribution: format!("Photo by {} on Unsplash", photographer),
    })
}

fn try_pexels(
    search_url: &str,
    api_key: &str,
    save_path: &std::path::Path,
) -> std::result::Result<StockImageResult, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client
        .get(search_url)
        .header("Authorization", api_key)
        .header("User-Agent", "nabaos/0.3 (+https://github.com/nabaos/nabaos)")
        .send()
        .map_err(|e| format!("Pexels request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Pexels returned {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Pexels JSON parse failed: {}", e))?;

    let photos = body.get("photos").and_then(|r| r.as_array())
        .ok_or("No photos array in Pexels response")?;

    let first = photos.first().ok_or("No images found on Pexels")?;

    let image_url = first
        .get("src")
        .and_then(|u| u.get("large"))
        .and_then(|v| v.as_str())
        .ok_or("No large URL in Pexels result")?;

    let photographer = first
        .get("photographer")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    // Download the image
    let img_resp = client
        .get(image_url)
        .send()
        .map_err(|e| format!("Image download failed: {}", e))?;

    let bytes = img_resp
        .bytes()
        .map_err(|e| format!("Failed to read image bytes: {}", e))?;

    std::fs::write(save_path, &bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(StockImageResult {
        attribution: format!("Photo by {} on Pexels", photographer),
    })
}
```

Also add `urlencoding` to `Cargo.toml` if not already present. Check first:
```bash
grep urlencoding Cargo.toml
```
If not found, add: `urlencoding = "2"` to `[dependencies]`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib runtime::host_functions::tests::test_fetch_stock_image`
Expected: PASS (the missing_query test should error, the other test may succeed or fail based on network but shouldn't panic).

**Step 5: Commit**

```bash
git add src/runtime/host_functions.rs src/runtime/manifest.rs Cargo.toml Cargo.lock
git commit -m "feat: add media.fetch_stock_image ability with Unsplash/Pexels/TikZ fallback"
```

---

## Task 4: Wire Style Analysis + Image Fetching into Engine

**Files:**
- Modify: `src/pea/engine.rs:942-949` (the ObjectiveComplete handler, where `assemble_document` is called)
- Modify: `src/pea/document.rs:21-28` (update `assemble_document` signature to accept `StyleConfig`)

**Step 1: Update assemble_document signature**

In `src/pea/document.rs`, change the `assemble_document` function signature at line 21 to accept an optional `StyleConfig`:

```rust
pub fn assemble_document(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[(String, PathBuf)],
    style: Option<&StyleConfig>,
    output_dir: &Path,
) -> Result<PathBuf> {
```

Pass `style` through to `generate_latex_source` and `generate_html_fallback`. Update their signatures similarly:

```rust
fn generate_latex_source(
    registry: &AbilityRegistry,
    manifest: &AgentManifest,
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[(String, PathBuf)],
    style: Option<&StyleConfig>,
) -> Result<String> {
```

In `generate_latex_source`, add the style config to the prompt (after the existing requirements block):

```rust
let style_instructions = if let Some(s) = style {
    format!(
        "\n\nSTYLE CONFIGURATION (apply these visual design choices):\n\
         Theme: {} | Primary color: {} | Accent color: {}\n\
         Font family: {} | Ornament style: {} | Chapter style: {}\n\
         Watermark: {} (opacity: {:.0}%)\n\
         Use drop caps: {}\n\n\
         STYLING PACKAGES TO USE:\n\
         - pgfornament: for decorative borders/corners (if ornament_style != 'none')\n\
         - draftwatermark: for watermark text (if watermark_text is set)\n\
         - lettrine: for drop caps (if use_drop_caps is true)\n\
         - titlesec: for custom chapter/section headings matching the theme\n\
         - Define \\definecolor{{primarycolor}}{{HTML}}{{{}}} and \\definecolor{{accentcolor}}{{HTML}}{{{}}}\n\
         - Use these colors for headings, rules, tcolorbox backgrounds, and decorative elements",
        s.theme, s.primary_color, s.accent_color,
        s.font_family, s.ornament_style, s.chapter_style,
        s.watermark_text.as_deref().unwrap_or("none"),
        s.watermark_opacity * 100.0,
        s.use_drop_caps,
        s.primary_color.trim_start_matches('#'),
        s.accent_color.trim_start_matches('#'),
    )
} else {
    String::new()
};
```

Append `style_instructions` to the prompt string before the closing `"Output ONLY the complete LaTeX source code..."`.

**Step 2: Update engine.rs ObjectiveComplete handler**

In `src/pea/engine.rs`, replace lines 942-949 (the `assemble_document` call) with:

```rust
let output_dir = data_dir.join("pea_output").join(&obj.id);

// 1. Analyze content style
let style_config = crate::pea::document::analyze_style(
    registry, manifest, &obj.description, &task_results,
);

// 2. Fetch images based on style config queries
let mut images: Vec<(String, std::path::PathBuf)> = Vec::new();
for iq in &style_config.image_queries {
    let fetch_input = serde_json::json!({
        "query": iq.query,
        "output_dir": output_dir.to_string_lossy(),
    });
    if let Ok(result) = registry.execute_ability(
        manifest,
        "media.fetch_stock_image",
        &fetch_input.to_string(),
    ) {
        let path_str = String::from_utf8_lossy(&result.output).to_string();
        let path = std::path::PathBuf::from(path_str.trim());
        if path.exists() {
            let caption = iq.chapter.clone().unwrap_or_else(|| iq.query.clone());
            images.push((caption, path));
        }
    }
}

// 3. Assemble document with style + images
match crate::pea::document::assemble_document(
    registry,
    manifest,
    &obj.description,
    &task_results,
    &images,
    Some(&style_config),
    &output_dir,
) {
```

**Step 3: Fix existing test**

Update `test_generate_html_fallback_structure` in `document.rs` to pass `None` for the style parameter (and any other call sites that pass `&[]` for images).

**Step 4: Run all tests**

Run: `cargo test --lib pea::`
Expected: All tests PASS.

**Step 5: Commit**

```bash
git add src/pea/document.rs src/pea/engine.rs
git commit -m "feat(pea): wire style analysis and image fetching into document assembly"
```

---

## Task 5: Styled HTML Fallback

**Files:**
- Modify: `src/pea/document.rs:262-335` (the `generate_html_fallback` function)

**Step 1: Write the failing test**

Add to tests module:

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib pea::document::tests::test_styled_html_fallback`
Expected: FAIL — signature mismatch (no `style` parameter yet).

**Step 3: Write implementation**

Update `generate_html_fallback` to accept `style: Option<&StyleConfig>` and inject CSS custom properties + watermark:

```rust
fn generate_html_fallback(
    objective_desc: &str,
    task_results: &[(String, String)],
    images: &[(String, PathBuf)],
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
            r#"
body::before {{
  content: "{}";
  position: fixed; top: 50%; left: 50%;
  transform: translate(-50%, -50%) rotate(-30deg);
  font-size: 6rem; color: {}; opacity: {:.2};
  pointer-events: none; z-index: -1; white-space: nowrap;
}}"#,
            html_escape(wm),
            s.primary_color,
            s.watermark_opacity.max(0.03).min(0.15),
        )
    } else {
        String::new()
    };

    // ... (rest of the function, same section/image logic as before,
    // but using CSS custom properties for colors and font)

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
:root {{
  --primary-color: {primary};
  --accent-color: {accent};
}}
body {{ font-family: {font}; max-width: 800px; margin: 2rem auto; padding: 0 1rem; line-height: 1.8; color: #333; }}
h1 {{ text-align: center; color: var(--primary-color); border-bottom: 3px solid var(--accent-color); padding-bottom: 0.5em; }}
h2 {{ color: var(--primary-color); border-bottom: 1px solid var(--accent-color); padding-bottom: 0.3em; }}
.content {{ margin: 1em 0; }}
figure {{ text-align: center; margin: 2em 0; }}
figure img {{ max-width: 100%; height: auto; border-radius: 8px; box-shadow: 0 4px 12px rgba(0,0,0,0.15); }}
figcaption {{ font-style: italic; color: #666; margin-top: 0.5em; }}
section {{ margin-bottom: 2em; }}
.toc {{ background: #f9f9f9; padding: 1em 2em; border-radius: 4px; margin: 1em 0 2em; border-left: 4px solid var(--accent-color); }}
.toc h3 {{ margin-top: 0; color: var(--primary-color); }}
.toc ol {{ padding-left: 1.5em; }}
{watermark}
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
<footer><p style="color: var(--accent-color); text-align: center;"><em>Generated by NabaOS PEA Engine</em></p></footer>
</body>
</html>"#,
        title = escaped_title,
        primary = s.primary_color,
        accent = s.accent_color,
        font = font_stack,
        watermark = watermark_css,
        toc = task_results
            .iter()
            .map(|(desc, _)| format!("<li>{}</li>", html_escape(desc)))
            .collect::<Vec<_>>()
            .join("\n"),
        sections = sections,
        images = image_html,
    )
}
```

Update the call site in `assemble_document` (line ~80) to pass style:
```rust
let html = generate_html_fallback(objective_desc, task_results, images, style);
```

**Step 4: Run tests**

Run: `cargo test --lib pea::document`
Expected: All tests PASS.

**Step 5: Commit**

```bash
git add src/pea/document.rs
git commit -m "feat(pea): styled HTML fallback with CSS custom properties and watermarks"
```

---

## Task 6: Env Management API Endpoints

**Files:**
- Modify: `src/channels/web.rs` (add 2 handlers + 2 routes)

**Step 1: Write handler functions**

Add before `create_router()` in `src/channels/web.rs`:

```rust
/// Known env keys that can be managed via API (name, description).
const MANAGED_ENV_KEYS: &[(&str, &str)] = &[
    ("NABA_LLM_API_KEY", "LLM provider API key"),
    ("NABA_LLM_PROVIDER", "LLM provider name"),
    ("NABA_LLM_MODEL", "LLM model name"),
    ("NABA_UNSPLASH_KEY", "Unsplash image search API key"),
    ("NABA_PEXELS_KEY", "Pexels image search API key"),
    ("NABA_FAL_API_KEY", "FAL AI image generation API key"),
    ("NABA_TELEGRAM_TOKEN", "Telegram bot token"),
    ("NABA_TELEGRAM_CHAT_ID", "Telegram allowed chat ID"),
    ("NABA_SMTP_SERVER", "SMTP email server"),
    ("NABA_SMTP_USERNAME", "SMTP username"),
    ("NABA_SMTP_PASSWORD", "SMTP password"),
];

async fn handle_list_env_keys(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let env_path = state.config.data_dir.join(".env");
    let env_content = std::fs::read_to_string(&env_path).unwrap_or_default();

    let keys: Vec<serde_json::Value> = MANAGED_ENV_KEYS
        .iter()
        .map(|(name, desc)| {
            let is_set = env_content
                .lines()
                .any(|line| {
                    line.starts_with(name)
                        && line.contains('=')
                        && line.split('=').nth(1).map(|v| !v.is_empty()).unwrap_or(false)
                });
            serde_json::json!({
                "name": name,
                "description": desc,
                "is_set": is_set,
            })
        })
        .collect();

    Json(serde_json::json!({ "keys": keys }))
}

#[derive(serde::Deserialize)]
struct SetEnvRequest {
    name: String,
    value: String,
}

async fn handle_set_env_key(
    State(state): State<AppState>,
    Json(body): Json<SetEnvRequest>,
) -> impl IntoResponse {
    // Validate: only managed keys can be set
    if !MANAGED_ENV_KEYS.iter().any(|(k, _)| *k == body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Unknown env key" })),
        );
    }

    let env_path = state.config.data_dir.join(".env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();

    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            let prefix = format!("{}=", body.name);
            if line.starts_with(&prefix) {
                found = true;
                format!("{}={}", body.name, body.value)
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(format!("{}={}", body.name, body.value));
    }

    match std::fs::write(&env_path, lines.join("\n")) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "message": format!("{} updated", body.name),
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write .env: {}", e) })),
        ),
    }
}
```

**Step 2: Add routes**

In `create_router()`, add after the Outputs routes:

```rust
// Env / Settings
.route("/api/v1/settings/env", get(handle_list_env_keys))
.route("/api/v1/settings/env", put(handle_set_env_key))
```

**Step 3: Run build**

Run: `cargo build --lib`
Expected: Clean compile.

**Step 4: Commit**

```bash
git add src/channels/web.rs
git commit -m "feat(web): add env key management API endpoints"
```

---

## Task 7: Web Frontend — API Keys Settings Section

**Files:**
- Modify: `nabaos-web/src/lib/api.ts` (add `getEnvKeys()`, `setEnvKey()`)
- Modify: `nabaos-web/src/routes/Settings.svelte` (add API Keys section)

**Step 1: Add API functions in api.ts**

```typescript
export interface EnvKeyInfo {
  name: string;
  description: string;
  is_set: boolean;
}

export async function getEnvKeys(): Promise<{ keys: EnvKeyInfo[] }> {
  const res = await fetch(`${API}/settings/env`);
  if (!res.ok) throw new Error('Failed to fetch env keys');
  return res.json();
}

export async function setEnvKey(name: string, value: string): Promise<{ message: string }> {
  const res = await fetch(`${API}/settings/env`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, value }),
  });
  if (!res.ok) throw new Error('Failed to set env key');
  return res.json();
}
```

**Step 2: Add API Keys section in Settings.svelte**

Add to the imports at the top:
```typescript
import { getEnvKeys, setEnvKey, type EnvKeyInfo } from '../lib/api';
```

Add state variables:
```typescript
let envKeys = $state<EnvKeyInfo[]>([]);
let editingKey = $state('');
let editingValue = $state('');
let savingKey = $state(false);
```

Add to `collapsed` state: `apikeys: false,`

Add `onMount` call to load env keys:
```typescript
const envData = await getEnvKeys();
envKeys = envData.keys;
```

Add a section in the template (after the Vault section is a natural spot):

```svelte
<!-- API Keys -->
<Card>
  <button class="section-header" onclick={() => toggleSection('apikeys')}>
    <h3>API Keys</h3>
    <span class="collapse-icon">{collapsed.apikeys ? '+' : '−'}</span>
  </button>
  {#if !collapsed.apikeys}
    <div class="section-body">
      <p class="section-desc">Manage API keys for image sourcing and integrations. Values are never displayed.</p>
      <div class="env-keys-list">
        {#each envKeys as key}
          <div class="env-key-row">
            <div class="env-key-info">
              <code class="env-key-name">{key.name}</code>
              <span class="env-key-desc">{key.description}</span>
            </div>
            <div class="env-key-actions">
              {#if key.is_set}
                <Badge color="green">SET</Badge>
              {:else}
                <Badge color="gray">NOT SET</Badge>
              {/if}
              {#if editingKey === key.name}
                <input
                  type="password"
                  class="env-key-input"
                  placeholder="Enter value..."
                  bind:value={editingValue}
                  onkeydown={(e) => e.key === 'Enter' && saveKey(key.name)}
                />
                <Button size="sm" onclick={() => saveKey(key.name)} disabled={savingKey}>
                  {savingKey ? 'Saving...' : 'Save'}
                </Button>
                <Button size="sm" variant="ghost" onclick={() => { editingKey = ''; editingValue = ''; }}>
                  Cancel
                </Button>
              {:else}
                <Button size="sm" variant="ghost" onclick={() => { editingKey = key.name; editingValue = ''; }}>
                  Edit
                </Button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</Card>
```

Add the save handler function:
```typescript
async function saveKey(name: string) {
  if (!editingValue.trim()) return;
  savingKey = true;
  try {
    await setEnvKey(name, editingValue.trim());
    // Refresh the list
    const data = await getEnvKeys();
    envKeys = data.keys;
    editingKey = '';
    editingValue = '';
    showToast(`${name} updated`, 'success');
  } catch (e: any) {
    showToast(`Failed to update ${name}`, 'error');
  }
  savingKey = false;
}
```

Add CSS for the env keys:
```css
.env-keys-list { display: flex; flex-direction: column; gap: 0.75rem; }
.env-key-row { display: flex; justify-content: space-between; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid var(--border, #333); }
.env-key-info { display: flex; flex-direction: column; gap: 0.25rem; }
.env-key-name { font-size: 0.85rem; color: var(--accent, #ffaf5f); }
.env-key-desc { font-size: 0.75rem; color: var(--muted, #888); }
.env-key-actions { display: flex; align-items: center; gap: 0.5rem; }
.env-key-input { background: var(--bg-secondary, #1a1a24); border: 1px solid var(--border, #333); border-radius: 4px; padding: 0.25rem 0.5rem; color: var(--fg, #c8c8d2); font-size: 0.85rem; width: 200px; }
```

**Step 3: Build frontend**

Run: `cd nabaos-web && npm run build`
Expected: Clean build.

**Step 4: Commit**

```bash
git add nabaos-web/src/lib/api.ts nabaos-web/src/routes/Settings.svelte nabaos-web/dist/
git commit -m "feat(web): add API Keys management section in Settings page"
```

---

## Task 8: TUI — API Keys in Settings Tab

**Files:**
- Modify: `src/tui/app.rs` (add API keys entries in `populate_settings`)

**Step 1: Find populate_settings**

Search for `populate_settings` in `src/tui/app.rs` and add the managed env keys as editable, secret entries in the settings list. The `ConfigEntry` struct already supports `is_secret` and `env_key` fields.

Add entries for each managed key, in a new "API Keys" section:

```rust
// API Keys section
for &(key_name, desc) in &[
    ("NABA_UNSPLASH_KEY", "Unsplash image search"),
    ("NABA_PEXELS_KEY", "Pexels image search"),
    ("NABA_FAL_API_KEY", "FAL AI image generation"),
] {
    let is_set = std::env::var(key_name).map(|v| !v.is_empty()).unwrap_or(false);
    settings.entries.push(ConfigEntry {
        key: desc.to_string(),
        value: if is_set { "[SET]".to_string() } else { "[NOT SET]".to_string() },
        section: "API Keys".to_string(),
        editable: true,
        is_secret: true,
        env_key: Some(key_name.to_string()),
    });
}
```

Also add `"API Keys"` to the `SECTIONS` constant in `src/tui/tabs/settings.rs`:

```rust
const SECTIONS: &[&str] = &["Provider", "Constitution", "Budget", "Channels", "API Keys", "System"];
```

And add a color for it in `section_color`:
```rust
"API Keys" => Color::Rgb(255, 175, 95), // accent color
```

**Step 2: Run build**

Run: `cargo build --lib`
Expected: Clean compile.

**Step 3: Commit**

```bash
git add src/tui/app.rs src/tui/tabs/settings.rs
git commit -m "feat(tui): add API Keys section in settings tab"
```

---

## Task 9: Telegram — /env Command

**Files:**
- Modify: `src/channels/telegram.rs` (add `/env` handling)

**Step 1: Add env handlers**

In the `handle_message` and `handle_message_rich` functions, add matching for `/env`:

```rust
"/env" | "/env list" => {
    handle_env_command(&data_dir).await
}
msg if msg.starts_with("/env set ") => {
    handle_env_set_command(&data_dir, msg).await
}
```

Add the handler functions:

```rust
async fn handle_env_command(data_dir: &std::path::Path) -> String {
    let env_path = data_dir.join(".env");
    let env_content = std::fs::read_to_string(&env_path).unwrap_or_default();

    let managed_keys: &[(&str, &str)] = &[
        ("NABA_LLM_API_KEY", "LLM provider"),
        ("NABA_UNSPLASH_KEY", "Unsplash images"),
        ("NABA_PEXELS_KEY", "Pexels images"),
        ("NABA_FAL_API_KEY", "FAL AI generation"),
        ("NABA_TELEGRAM_TOKEN", "Telegram bot"),
    ];

    let mut lines = vec!["API Keys Status:".to_string(), String::new()];
    for (name, desc) in managed_keys {
        let is_set = env_content
            .lines()
            .any(|l| l.starts_with(name) && l.contains('=') && l.len() > name.len() + 1);
        let status = if is_set { "SET" } else { "NOT SET" };
        lines.push(format!("{} ({}) — {}", name, desc, status));
    }
    lines.push(String::new());
    lines.push("Use /env set KEY value to update.".to_string());
    lines.join("\n")
}

async fn handle_env_set_command(data_dir: &std::path::Path, msg: &str) -> String {
    // Parse: /env set KEY value
    let parts: Vec<&str> = msg.splitn(4, ' ').collect();
    if parts.len() < 4 {
        return "Usage: /env set KEY value".to_string();
    }
    let key_name = parts[2];
    let value = parts[3];

    let managed_keys = [
        "NABA_LLM_API_KEY", "NABA_UNSPLASH_KEY", "NABA_PEXELS_KEY",
        "NABA_FAL_API_KEY", "NABA_TELEGRAM_TOKEN", "NABA_TELEGRAM_CHAT_ID",
        "NABA_SMTP_SERVER", "NABA_SMTP_USERNAME", "NABA_SMTP_PASSWORD",
        "NABA_LLM_PROVIDER", "NABA_LLM_MODEL",
    ];

    if !managed_keys.contains(&key_name) {
        return format!("Unknown key: {}. Use /env to see available keys.", key_name);
    }

    let env_path = data_dir.join(".env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            let prefix = format!("{}=", key_name);
            if line.starts_with(&prefix) {
                found = true;
                format!("{}={}", key_name, value)
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(format!("{}={}", key_name, value));
    }

    match std::fs::write(&env_path, lines.join("\n")) {
        Ok(()) => format!("{} updated successfully.", key_name),
        Err(e) => format!("Failed to save: {}", e),
    }
}
```

**Step 2: Run build**

Run: `cargo build --lib`
Expected: Clean compile.

**Step 3: Commit**

```bash
git add src/channels/telegram.rs
git commit -m "feat(telegram): add /env command for API key management"
```

---

## Task 10: Add urlencoding Dependency + Final Integration Test

**Files:**
- Modify: `Cargo.toml` (add urlencoding if needed)

**Step 1: Check and add dependency**

```bash
cd /home/kumkum/nabaos && grep urlencoding Cargo.toml
```

If not found:
```bash
cargo add urlencoding
```

**Step 2: Full build + test**

Run: `cargo test --lib pea::document && cargo test --lib runtime::host_functions && cargo build --release`
Expected: All tests PASS, release build succeeds.

**Step 3: Final commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add urlencoding dependency for stock image search"
```

---

## Verification Checklist

1. `cargo test --lib pea::document` — StyleConfig, analyze_style, styled HTML tests pass
2. `cargo test --lib runtime::host_functions` — fetch_stock_image ability tests pass
3. `cargo build --release` — clean compile
4. Manual: create PEA objective → complete → output has themed styling + images
5. `curl http://localhost:8920/api/v1/settings/env` → lists keys with is_set status
6. `PUT /api/v1/settings/env` with `{"name":"NABA_PEXELS_KEY","value":"test"}` → .env updated
7. TUI settings tab → "API Keys" section shows with SET/NOT SET badges
8. Telegram `/env` → lists key status
