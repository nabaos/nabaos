//! Terminal formatting toolkit — ANSI colors, box drawing, status indicators.
//!
//! Respects `NO_COLOR`, `TERM=dumb`, and non-TTY stdout automatically.

use std::io::IsTerminal;

// ---------------------------------------------------------------------------
// Environment detection
// ---------------------------------------------------------------------------

/// Returns `true` if ANSI color output is allowed.
pub fn color_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    std::io::stdout().is_terminal()
}

// ---------------------------------------------------------------------------
// ANSI escape constants
// ---------------------------------------------------------------------------

pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";
pub const MAGENTA: &str = "\x1b[35m";
pub const WHITE: &str = "\x1b[37m";

/// Return the escape code only if colors are enabled, otherwise "".
fn c(code: &str) -> &str {
    if color_enabled() {
        code
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Terminal width
// ---------------------------------------------------------------------------

fn term_width() -> usize {
    // Try crossterm first (when tui feature is active)
    #[cfg(feature = "tui")]
    {
        if let Ok((w, _)) = crossterm::terminal::size() {
            if w > 20 {
                return w as usize;
            }
        }
    }
    // Fallback to COLUMNS env
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80)
}

// ---------------------------------------------------------------------------
// Box drawing
// ---------------------------------------------------------------------------

/// `╭─── Title ──────────────────────╮`
pub fn header_line(title: &str) -> String {
    let w = term_width();
    let label = format!(" {} ", title);
    let inner = w.saturating_sub(2);
    let left = 3;
    let right = inner.saturating_sub(left + label.len());
    format!(
        "{}╭{}{}{}{}{}{}╮{}",
        c(DIM),
        "─".repeat(left),
        c(RESET),
        c(BOLD),
        label,
        c(RESET),
        c(DIM).to_string() + &"─".repeat(right),
        c(RESET),
    )
}

/// Alias for backward compatibility.
pub fn header(title: &str) -> String {
    header_line(title)
}

/// `╰──────────────────────────────────────────╯`
pub fn footer() -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    format!("{}╰{}╯{}", c(DIM), "─".repeat(inner), c(RESET))
}

/// `├──────────────────────────────────────────┤`
pub fn separator() -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    format!("{}├{}┤{}", c(DIM), "─".repeat(inner), c(RESET))
}

/// `├─── Title ────────────────────────────────┤`
pub fn section(title: &str) -> String {
    let w = term_width();
    let label = format!(" {} ", title);
    let inner = w.saturating_sub(2);
    let left = 3;
    let right = inner.saturating_sub(left + label.len());
    format!(
        "{}├{}{}{}{}{}{}┤{}",
        c(DIM),
        "─".repeat(left),
        c(RESET),
        c(BOLD),
        label,
        c(RESET),
        c(DIM).to_string() + &"─".repeat(right),
        c(RESET),
    )
}

/// `│  Key        Value                        │`
pub fn row(key: &str, val: &str) -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    let raw_content = format!("  {:<12} {}", key, val);
    let content = truncate_visible(&raw_content, inner);
    let pad = inner.saturating_sub(visible_len(&content));
    format!(
        "{}│{}{}{}{}│{}",
        c(DIM),
        c(RESET),
        content,
        " ".repeat(pad),
        c(DIM),
        c(RESET),
    )
}

/// `│  K1  V1     K2  V2                       │`
pub fn row_pair(k1: &str, v1: &str, k2: &str, v2: &str) -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    let raw_content = format!("  {:<10} {:<10}{:<10} {}", k1, v1, k2, v2);
    let content = truncate_visible(&raw_content, inner);
    let pad = inner.saturating_sub(visible_len(&content));
    format!(
        "{}│{}{}{}{}│{}",
        c(DIM),
        c(RESET),
        content,
        " ".repeat(pad),
        c(DIM),
        c(RESET),
    )
}

/// Blank padded row with arbitrary content.
pub fn row_raw(content: &str) -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    let content = truncate_visible(content, inner);
    let vlen = visible_len(&content);
    let pad = inner.saturating_sub(vlen);
    format!(
        "{}│{}{}{}{}│{}",
        c(DIM),
        c(RESET),
        content,
        " ".repeat(pad),
        c(DIM),
        c(RESET),
    )
}

/// Empty row inside a box.
pub fn row_empty() -> String {
    let w = term_width();
    let inner = w.saturating_sub(2);
    format!(
        "{}│{}{}{}│{}",
        c(DIM),
        c(RESET),
        " ".repeat(inner),
        c(DIM),
        c(RESET),
    )
}

// ---------------------------------------------------------------------------
// Status indicators
// ---------------------------------------------------------------------------

/// `│  ✓ msg`
pub fn ok(msg: &str) -> String {
    row_raw(&format!("  {}✓{} {}", c(GREEN), c(RESET), msg))
}

/// `│  ✗ msg`
pub fn fail(msg: &str) -> String {
    row_raw(&format!("  {}✗{} {}", c(RED), c(RESET), msg))
}

/// `│  ▲ msg`
pub fn warn(msg: &str) -> String {
    row_raw(&format!("  {}▲{} {}", c(YELLOW), c(RESET), msg))
}

/// `│  ○ msg`
pub fn skip(msg: &str) -> String {
    row_raw(&format!("  {}○{} {}", c(DIM), c(RESET), msg))
}

/// `│  ● msg`
pub fn active(msg: &str) -> String {
    row_raw(&format!("  {}●{} {}", c(CYAN), c(RESET), msg))
}

// ---------------------------------------------------------------------------
// Data visualization
// ---------------------------------------------------------------------------

/// `████░░░░ 67%`
pub fn progress_bar(frac: f64, w: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * w as f64).round() as usize;
    let empty = w.saturating_sub(filled);
    format!(
        "{}{}{}{}{}",
        c(GREEN),
        "█".repeat(filled),
        c(DIM),
        "░".repeat(empty),
        c(RESET),
    )
}

/// `[CACHE]` with color
pub fn badge(label: &str, color: &str) -> String {
    format!("{}[{}]{}", c(color), label, c(RESET))
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

/// Format USD amount: `$0.0017`
pub fn money(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else if usd < 1.0 {
        format!("${:.3}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

/// Format latency: `47.0ms`
pub fn latency(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.2}ms", ms)
    } else if ms < 100.0 {
        format!("{:.1}ms", ms)
    } else {
        format!("{:.0}ms", ms)
    }
}

/// Format percentage: `51.5%`
pub fn pct(val: f64) -> String {
    format!("{:.1}%", val)
}

/// Format token count in human-readable form: `85K`
pub fn tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{}K", count / 1_000)
    } else {
        format!("{}", count)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute the visible length of a string, stripping ANSI escape codes.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0usize;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

/// Truncate a string to at most `max_visible` visible characters,
/// preserving ANSI escape sequences. Adds "…" if truncated.
fn truncate_visible(s: &str, max_visible: usize) -> String {
    let vlen = visible_len(s);
    if vlen <= max_visible {
        return s.to_string();
    }
    let target = max_visible.saturating_sub(1); // leave room for "…"
    let mut result = String::new();
    let mut vis_count = 0;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            result.push(ch);
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            result.push(ch);
            in_escape = true;
        } else {
            if vis_count >= target {
                break;
            }
            result.push(ch);
            vis_count += 1;
        }
    }
    // Reset ANSI state and add ellipsis
    if result.contains('\x1b') {
        result.push_str(c(RESET));
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_money_formatting() {
        assert_eq!(money(0.0017), "$0.0017");
        assert_eq!(money(0.05), "$0.050");
        assert_eq!(money(2.016), "$2.02");
    }

    #[test]
    fn test_latency_formatting() {
        assert_eq!(latency(0.5), "0.50ms");
        assert_eq!(latency(23.4), "23.4ms");
        assert_eq!(latency(150.0), "150ms");
    }

    #[test]
    fn test_pct_formatting() {
        assert_eq!(pct(51.5), "51.5%");
        assert_eq!(pct(100.0), "100.0%");
    }

    #[test]
    fn test_tokens_formatting() {
        assert_eq!(tokens(500), "500");
        assert_eq!(tokens(85_000), "85K");
        assert_eq!(tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_visible_len_strips_ansi() {
        assert_eq!(visible_len("hello"), 5);
        assert_eq!(visible_len("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_len("\x1b[1m\x1b[32m✓\x1b[0m ok"), 4);
    }

    #[test]
    fn test_progress_bar() {
        std::env::set_var("NO_COLOR", "1");
        let bar = progress_bar(0.5, 10);
        assert!(bar.contains("█████"));
        assert!(bar.contains("░░░░░"));
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn test_truncate_visible() {
        std::env::set_var("NO_COLOR", "1");
        let short = "hello";
        assert_eq!(truncate_visible(short, 10), "hello");
        let long = "hello world this is long";
        let truncated = truncate_visible(long, 10);
        assert_eq!(visible_len(&truncated), 10);
        assert!(truncated.ends_with('…'));
        std::env::remove_var("NO_COLOR");
    }
}
