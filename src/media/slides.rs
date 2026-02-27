//! Slide generator — renders reveal.js HTML presentations with pandoc export.

use crate::core::error::{NyayaError, Result};
use crate::media::reveal_template;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideTheme {
    Light,
    Dark,
    Corporate,
}

impl SlideTheme {
    pub fn css_name(&self) -> &str {
        match self {
            Self::Light => "white",
            Self::Dark => "black",
            Self::Corporate => "simple",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Html,
    Pptx,
    Odp,
    Pdf,
}

impl ExportFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::Html => "html",
            Self::Pptx => "pptx",
            Self::Odp => "odp",
            Self::Pdf => "pdf",
        }
    }

    pub fn pandoc_format(&self) -> Option<&str> {
        match self {
            Self::Html => None,
            Self::Pptx => Some("pptx"),
            Self::Odp => Some("odp"),
            Self::Pdf => Some("pdf"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideDeck {
    pub title: String,
    pub theme: SlideTheme,
    pub slides: Vec<Slide>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub content: SlideContent,
    pub speaker_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SlideContent {
    Title {
        title: String,
        subtitle: Option<String>,
    },
    Bullets {
        heading: String,
        items: Vec<String>,
    },
    Image {
        #[serde(skip)]
        image_bytes: Vec<u8>,
        caption: String,
        mime_type: String,
    },
    Chart {
        #[serde(skip)]
        svg_bytes: Vec<u8>,
        caption: String,
    },
    TwoColumn {
        left: Box<SlideContent>,
        right: Box<SlideContent>,
    },
    Quote {
        text: String,
        attribution: String,
    },
    Code {
        language: String,
        code: String,
    },
}

pub struct SlideGenerator {
    pandoc_path: Option<PathBuf>,
}

impl SlideGenerator {
    pub fn new(pandoc_path: Option<PathBuf>) -> Self {
        Self { pandoc_path }
    }

    /// Render a SlideDeck to reveal.js HTML.
    pub fn render_html(&self, deck: &SlideDeck) -> Result<String> {
        reveal_template::render(deck)
    }

    /// Render and export to a specific format.
    pub fn export(
        &self,
        deck: &SlideDeck,
        format: ExportFormat,
        output_path: &Path,
    ) -> Result<PathBuf> {
        let html = self.render_html(deck)?;

        if matches!(format, ExportFormat::Html) {
            let path = output_path.with_extension("html");
            std::fs::write(&path, &html)
                .map_err(|e| NyayaError::Config(format!("Failed to write HTML: {e}")))?;
            return Ok(path);
        }

        let pandoc = self.pandoc_path.as_ref().ok_or_else(|| {
            NyayaError::Config(
                "pandoc not found. Install with: sudo apt install pandoc (Linux) or brew install pandoc (macOS)".to_string(),
            )
        })?;

        let pandoc_fmt = format.pandoc_format().ok_or_else(|| {
            NyayaError::Config(format!("Unsupported export format: {}", format.extension()))
        })?;

        let temp_html = output_path.with_extension("tmp.html");
        std::fs::write(&temp_html, &html)
            .map_err(|e| NyayaError::Config(format!("Failed to write temp HTML: {e}")))?;

        let output_file = output_path.with_extension(format.extension());
        let status = Command::new(pandoc)
            .arg(&temp_html)
            .arg("-t")
            .arg(pandoc_fmt)
            .arg("-o")
            .arg(&output_file)
            .status()
            .map_err(|e| NyayaError::Config(format!("pandoc execution error: {e}")))?;

        let _ = std::fs::remove_file(&temp_html);

        if !status.success() {
            return Err(NyayaError::Config(format!(
                "pandoc exited with status {}",
                status
            )));
        }

        Ok(output_file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slide_theme_css_names() {
        assert_eq!(SlideTheme::Light.css_name(), "white");
        assert_eq!(SlideTheme::Dark.css_name(), "black");
        assert_eq!(SlideTheme::Corporate.css_name(), "simple");
    }

    #[test]
    fn test_export_format_extensions() {
        assert_eq!(ExportFormat::Html.extension(), "html");
        assert_eq!(ExportFormat::Pptx.extension(), "pptx");
        assert_eq!(ExportFormat::Odp.extension(), "odp");
        assert_eq!(ExportFormat::Pdf.extension(), "pdf");
    }

    #[test]
    fn test_slide_deck_renders_html() {
        let deck = SlideDeck {
            title: "Test Deck".to_string(),
            theme: SlideTheme::Dark,
            slides: vec![
                Slide {
                    content: SlideContent::Title {
                        title: "Hello".to_string(),
                        subtitle: Some("World".to_string()),
                    },
                    speaker_notes: None,
                },
                Slide {
                    content: SlideContent::Bullets {
                        heading: "Key Points".to_string(),
                        items: vec!["Point 1".to_string(), "Point 2".to_string()],
                    },
                    speaker_notes: None,
                },
            ],
        };
        let gen = SlideGenerator::new(None);
        let html = gen.render_html(&deck).unwrap();
        assert!(html.contains("reveal"));
        assert!(html.contains("Hello"));
        assert!(html.contains("Point 1"));
    }

    #[test]
    fn test_export_pptx_without_pandoc_returns_error() {
        let deck = SlideDeck {
            title: "Test".to_string(),
            theme: SlideTheme::Light,
            slides: vec![],
        };
        let gen = SlideGenerator::new(None);
        let result = gen.export(&deck, ExportFormat::Pptx, Path::new("/tmp/test"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("pandoc"));
    }
}
