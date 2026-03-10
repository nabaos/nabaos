# Styled Document Output with Media & Env Management

**Date:** 2026-03-10
**Status:** Approved

## Problem

PEA outputs are plain text assembled into basic LaTeX/HTML with no visual design, no images, and no content-appropriate styling. A cookbook looks the same as a research report. No stock images are sourced despite the `images` parameter existing in `assemble_document()`. Users also cannot manage API keys from the UI.

## Design

### 1. StyleConfig — LLM-Driven Style Analysis

Before document assembly, an LLM call analyzes the objective and content type, outputting a structured JSON config:

```rust
pub struct StyleConfig {
    pub theme: String,                   // "oriental", "academic", "corporate", "creative"
    pub primary_color: String,           // hex, e.g. "#8B4513"
    pub accent_color: String,            // hex
    pub font_family: String,             // "serif", "sans-serif", "monospace"
    pub ornament_style: String,          // "floral", "geometric", "minimal", "none"
    pub watermark_text: Option<String>,  // e.g. "Mughlai Cuisine", null for reports
    pub watermark_opacity: f64,          // 0.0–0.15
    pub chapter_style: String,           // "ornate", "clean", "academic"
    pub use_drop_caps: bool,
    pub image_queries: Vec<ImageQuery>,
}

pub struct ImageQuery {
    pub query: String,        // "mughlai biryani dish closeup"
    pub placement: String,    // "chapter_header", "section_illustration", "watermark_bg"
    pub chapter: Option<String>,
}
```

The LLM decides everything — ornaments, watermarks, image search terms — based purely on content. No hardcoded content-type logic.

### 2. Image Sourcing — Multi-Source Fallback Chain

New `media.fetch_stock_image` ability with fallback chain:

1. **Unsplash** (embedded NabaOS demo `client_id`, 50 req/hr) — `GET https://api.unsplash.com/search/photos?query=...&client_id=...`
2. **Pexels** (if `NABA_PEXELS_KEY` set) — `GET https://api.pexels.com/v1/search?query=...`
3. **FAL AI** (if `NABA_FAL_API_KEY` set) — image generation API
4. **TikZ fallback** — LLM generates TikZ illustration code

Each step:
- Downloads image to `output_dir/images/{query_hash}.jpg`
- Returns local path + attribution metadata in `facts`
- Uses existing `validate_url_ssrf()` for security
- Caches by query hash (skip re-download)

Unsplash zero-config: Register a free NabaOS developer app. Embed `client_id` as compile-time constant. User's `NABA_UNSPLASH_KEY` overrides it. Standard practice for open-source.

### 3. Document Pipeline Integration

```
ObjectiveComplete
  ├─ 1. analyze_style(objective, task_results) → StyleConfig
  ├─ 2. fetch_document_images(style_config.image_queries) → Vec<(caption, path)>
  ├─ 3. assemble_document(task_results, images, style_config)
  │     LaTeX prompt includes StyleConfig JSON + available images
  │     LLM generates themed LaTeX with ornaments, watermarks, colors
  └─ 4. compile → PDF (or styled HTML fallback)
```

### 4. LaTeX Styling

Extended `LATEX_SYSTEM_PROMPT` instructs LLM to use:
- `pgfornament` — decorative borders and ornaments
- `draftwatermark` or `background` — custom watermark text/opacity
- `titlesec` — themed chapter/section headings
- `lettrine` — drop caps when `use_drop_caps: true`
- `xcolor` — primary/accent colors from StyleConfig
- `graphicx` — `\includegraphics` for downloaded images with figure environments

HTML fallback: StyleConfig drives CSS custom properties for colors, fonts. Downloaded images inlined as base64.

### 5. Env Variable Management

**Known keys** (static array in code):
```rust
const MANAGED_ENV_KEYS: &[(&str, &str)] = &[
    ("NABA_LLM_API_KEY", "LLM provider API key"),
    ("NABA_UNSPLASH_KEY", "Unsplash image search"),
    ("NABA_PEXELS_KEY", "Pexels image search"),
    ("NABA_FAL_API_KEY", "FAL AI image generation"),
    ("NABA_TELEGRAM_TOKEN", "Telegram bot token"),
];
```

**API:**
```
GET  /api/v1/settings/env   → [{ name, description, is_set: bool }]
PUT  /api/v1/settings/env   → { name, value } (writes to .env, never returns values)
```

**Web UI:** Settings page → "API Keys" section. Shows key names with SET/NOT SET badges. Edit button → masked input. Values never sent to frontend.

**TUI:** Settings tab → "API Keys" section. `e` to edit, masked input prompt.

**Telegram:** `/env` lists status, `/env set KEY value` sets (allowed chats only).

## Files Modified

| File | Change | ~Lines |
|------|--------|--------|
| `src/pea/document.rs` | `StyleConfig`, `ImageQuery`, `analyze_style()`, extend assembly prompt, themed HTML | ~200 |
| `src/pea/engine.rs` | Call `analyze_style()` + image fetching before `assemble_document()` | ~40 |
| `src/runtime/host_functions.rs` | `media.fetch_stock_image` ability with fallback chain | ~200 |
| `src/runtime/manifest.rs` | Add `media.fetch_stock_image` to default permissions | ~2 |
| `src/pea/bridge.rs` | Update `execute_media()` to use new stock image ability | ~20 |
| `src/channels/web.rs` | 2 env management endpoints | ~60 |
| `src/tui/tabs/settings.rs` | API Keys section | ~50 |
| `src/channels/telegram.rs` | `/env` command | ~40 |
| `nabaos-web/src/routes/Settings.svelte` | API Keys section in settings page | ~80 |
| `nabaos-web/src/lib/api.ts` | `getEnvKeys()`, `setEnvKey()` functions | ~20 |

## Implementation Order

1. `document.rs` — StyleConfig struct + `analyze_style()` + extend assembly prompt
2. `host_functions.rs` — `media.fetch_stock_image` ability (Unsplash → Pexels → FAL → TikZ)
3. `manifest.rs` — add to default permissions
4. `engine.rs` — wire style analysis + image fetching into ObjectiveComplete
5. `document.rs` — themed HTML fallback
6. `web.rs` — env management endpoints
7. `nabaos-web/` — Settings page API Keys section
8. `tui/tabs/settings.rs` — API Keys section
9. `telegram.rs` — `/env` command
10. `bridge.rs` — update execute_media() to use new ability

## Verification

1. `cargo test --lib pea::document` — StyleConfig parsing, analyze_style mock
2. `cargo test --lib runtime::host_functions` — fetch_stock_image ability
3. `cargo build --release` — clean compile
4. Manual: create PEA objective → complete → check output has themed styling + images
5. `curl /api/v1/settings/env` → lists keys with is_set status
6. `PUT /api/v1/settings/env` → sets a key, verify .env updated
7. TUI settings → API Keys visible with badges
8. Telegram `/env` → lists key status
