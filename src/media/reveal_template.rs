//! reveal.js HTML template renderer.

use crate::core::error::Result;
use crate::media::slides::{SlideContent, SlideDeck};

/// Render a SlideDeck to a self-contained reveal.js HTML string.
pub fn render(deck: &SlideDeck) -> Result<String> {
    let mut slides_html = String::new();

    for slide in &deck.slides {
        slides_html.push_str("        <section>\n");
        render_content(&slide.content, &mut slides_html);
        if let Some(ref notes) = slide.speaker_notes {
            slides_html.push_str(&format!(
                "          <aside class=\"notes\">{}</aside>\n",
                html_escape(notes)
            ));
        }
        slides_html.push_str("        </section>\n");
    }

    let theme = deck.theme.css_name();
    let title = html_escape(&deck.title);

    Ok(format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>{title}</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/reveal.js@5/dist/reveal.css">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/reveal.js@5/dist/theme/{theme}.css">
  <style>
    .reveal h1 {{ font-size: 2.5em; }}
    .reveal h2 {{ font-size: 1.8em; }}
    .reveal img {{ max-height: 60vh; }}
    .reveal pre code {{ font-size: 0.8em; max-height: 500px; }}
    .two-column {{ display: flex; gap: 2em; }}
    .two-column > div {{ flex: 1; }}
    .quote {{ font-style: italic; font-size: 1.4em; }}
    .attribution {{ text-align: right; font-size: 0.8em; margin-top: 1em; }}
  </style>
</head>
<body>
  <div class="reveal">
    <div class="slides">
{slides_html}
    </div>
  </div>
  <script src="https://cdn.jsdelivr.net/npm/reveal.js@5/dist/reveal.js"></script>
  <script>Reveal.initialize({{ hash: true }});</script>
</body>
</html>"#
    ))
}

fn render_content(content: &SlideContent, out: &mut String) {
    match content {
        SlideContent::Title { title, subtitle } => {
            out.push_str(&format!("          <h1>{}</h1>\n", html_escape(title)));
            if let Some(sub) = subtitle {
                out.push_str(&format!("          <h3>{}</h3>\n", html_escape(sub)));
            }
        }
        SlideContent::Bullets { heading, items } => {
            out.push_str(&format!(
                "          <h2>{}</h2>\n          <ul>\n",
                html_escape(heading)
            ));
            for item in items {
                out.push_str(&format!("            <li>{}</li>\n", html_escape(item)));
            }
            out.push_str("          </ul>\n");
        }
        SlideContent::Image {
            image_bytes,
            caption,
            mime_type,
        } => {
            if !image_bytes.is_empty() {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
                out.push_str(&format!(
                    "          <img src=\"data:{};base64,{}\" alt=\"{}\">\n",
                    html_escape(mime_type),
                    b64,
                    html_escape(caption)
                ));
            }
            out.push_str(&format!(
                "          <p><em>{}</em></p>\n",
                html_escape(caption)
            ));
        }
        SlideContent::Chart { svg_bytes, caption } => {
            if !svg_bytes.is_empty() {
                let svg = String::from_utf8_lossy(svg_bytes);
                // Basic SVG sanitization: strip script tags and event handlers
                let sanitized = svg
                    .replace("<script", "&lt;script")
                    .replace("</script", "&lt;/script")
                    .replace("onload=", "data-removed=")
                    .replace("onerror=", "data-removed=")
                    .replace("onclick=", "data-removed=")
                    .replace("onmouseover=", "data-removed=");
                out.push_str(&format!("          {}\n", sanitized));
            }
            out.push_str(&format!(
                "          <p><em>{}</em></p>\n",
                html_escape(caption)
            ));
        }
        SlideContent::TwoColumn { left, right } => {
            out.push_str("          <div class=\"two-column\">\n");
            out.push_str("            <div>\n");
            render_content(left, out);
            out.push_str("            </div>\n");
            out.push_str("            <div>\n");
            render_content(right, out);
            out.push_str("            </div>\n");
            out.push_str("          </div>\n");
        }
        SlideContent::Quote { text, attribution } => {
            out.push_str(&format!(
                "          <blockquote class=\"quote\">\"{}</blockquote>\n",
                html_escape(text)
            ));
            out.push_str(&format!(
                "          <p class=\"attribution\">— {}</p>\n",
                html_escape(attribution)
            ));
        }
        SlideContent::Code { language, code } => {
            out.push_str(&format!(
                "          <pre><code class=\"language-{}\">{}</code></pre>\n",
                html_escape(language),
                html_escape(code)
            ));
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::slides::{Slide, SlideDeck, SlideTheme};

    #[test]
    fn test_render_contains_reveal_structure() {
        let deck = SlideDeck {
            title: "Test".to_string(),
            theme: SlideTheme::Light,
            slides: vec![Slide {
                content: SlideContent::Title {
                    title: "Hello".to_string(),
                    subtitle: None,
                },
                speaker_notes: None,
            }],
        };
        let html = render(&deck).unwrap();
        assert!(html.contains("<div class=\"reveal\">"));
        assert!(html.contains("reveal.js"));
        assert!(html.contains("<h1>Hello</h1>"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn test_render_code_slide() {
        let deck = SlideDeck {
            title: "Code".to_string(),
            theme: SlideTheme::Dark,
            slides: vec![Slide {
                content: SlideContent::Code {
                    language: "rust".to_string(),
                    code: "fn main() {}".to_string(),
                },
                speaker_notes: None,
            }],
        };
        let html = render(&deck).unwrap();
        assert!(html.contains("language-rust"));
        assert!(html.contains("fn main()"));
    }
}
