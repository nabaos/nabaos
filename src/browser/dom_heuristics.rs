//! DOM Heuristics — Layer 0 of the navigation cascade.
//!
//! Pure Rust pattern matching on DOM structure. No ML models.
//! Detects common page patterns (cookie banners, login forms, search boxes,
//! article content, pagination, popups) and returns appropriate navigation actions.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Direction for scroll actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScrollDirection {
    Down,
    Up,
}

/// A navigation action the browser agent can take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NavAction {
    Click { selector: String },
    Type { selector: String, value: String },
    Scroll { direction: ScrollDirection },
    Wait { ms: u64 },
    GoBack,
    Skip { reason: String },
    ExtractContent { selector: String },
    Download { url: String },
}

/// A decision produced by a heuristic layer, including confidence and provenance.
#[derive(Debug, Clone)]
pub struct ActionDecision {
    pub action: NavAction,
    pub confidence: f32,
    pub source: &'static str,
}

/// High-level context describing the current task and page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub goal: String,
    pub target_data: Option<String>,
    pub page_url: String,
    pub page_title: String,
}

/// A simplified representation of a DOM element for heuristic inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomElement {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub role: Option<String>,
    pub aria_label: Option<String>,
    pub text: String,
    pub href: Option<String>,
    pub input_type: Option<String>,
    pub name: Option<String>,
    pub placeholder: Option<String>,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run all Layer-0 DOM heuristics against the given page elements.
///
/// Priority order:
/// 1. Cookie banner (always dismiss first)
/// 2. Task-specific heuristics (login, search, extract, paginate, dismiss popup)
///
/// Returns `None` when no heuristic matches.
pub fn dom_heuristic_action(task: &TaskContext, elements: &[DomElement]) -> Option<ActionDecision> {
    // Always handle cookie banners first — they block interaction.
    if let Some(decision) = detect_cookie_banner(elements) {
        return Some(decision);
    }

    let goal_lower = task.goal.to_lowercase();

    // Task-specific heuristics in priority order.
    if goal_lower.contains("login") || goal_lower.contains("sign in") {
        if let Some(d) = detect_login_form(elements) {
            return Some(d);
        }
    }

    if goal_lower.contains("search") || task.target_data.is_some() {
        if let Some(d) = detect_search_form(elements, task) {
            return Some(d);
        }
    }

    if goal_lower.contains("extract")
        || goal_lower.contains("article")
        || goal_lower.contains("read")
    {
        if let Some(d) = detect_article_content(elements) {
            return Some(d);
        }
    }

    if goal_lower.contains("next") || goal_lower.contains("paginate") || goal_lower.contains("page")
    {
        if let Some(d) = detect_pagination(elements) {
            return Some(d);
        }
    }

    if let Some(d) = detect_popup_dismiss(elements) {
        return Some(d);
    }

    None
}

// ---------------------------------------------------------------------------
// Heuristic detectors
// ---------------------------------------------------------------------------

/// Detect cookie consent banners and return a Click action on the accept button.
pub fn detect_cookie_banner(elements: &[DomElement]) -> Option<ActionDecision> {
    let accept_keywords = [
        "accept",
        "agree",
        "allow",
        "got it",
        "ok",
        "okay",
        "consent",
        "i understand",
        "acknowledge",
        "continue",
    ];

    for el in elements {
        if el.tag != "button" && el.tag != "a" {
            continue;
        }

        let text_lower = el.text.to_lowercase();
        let label_lower = el.aria_label.as_deref().unwrap_or("").to_lowercase();

        let has_cookie_class = el.classes.iter().any(|c| {
            let cl = c.to_lowercase();
            cl.contains("cookie") || cl.contains("consent") || cl.contains("gdpr")
        });

        let text_matches = accept_keywords
            .iter()
            .any(|kw| text_lower.contains(kw) || label_lower.contains(kw));

        if text_matches
            && (has_cookie_class || text_lower.contains("cookie") || label_lower.contains("cookie"))
        {
            return Some(ActionDecision {
                action: NavAction::Click {
                    selector: element_to_selector(el),
                },
                confidence: 0.90,
                source: "dom_heuristic:cookie_banner",
            });
        }
    }
    None
}

/// Detect a login form. Finds the password field first, then looks for a
/// username/email input nearby.
pub fn detect_login_form(elements: &[DomElement]) -> Option<ActionDecision> {
    // First, find a password input.
    let has_password = elements.iter().any(|el| {
        el.tag == "input"
            && el
                .input_type
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case("password"))
                .unwrap_or(false)
    });

    if !has_password {
        return None;
    }

    // Look for the username / email field.
    for el in elements {
        if el.tag != "input" {
            continue;
        }
        let itype = el.input_type.as_deref().unwrap_or("");
        let name = el.name.as_deref().unwrap_or("").to_lowercase();
        let placeholder = el.placeholder.as_deref().unwrap_or("").to_lowercase();
        let label = el.aria_label.as_deref().unwrap_or("").to_lowercase();

        let is_username = itype.eq_ignore_ascii_case("email")
            || itype.eq_ignore_ascii_case("text")
            || name.contains("user")
            || name.contains("email")
            || name.contains("login")
            || placeholder.contains("email")
            || placeholder.contains("user")
            || label.contains("email")
            || label.contains("user");

        if is_username {
            return Some(ActionDecision {
                action: NavAction::Type {
                    selector: element_to_selector(el),
                    value: String::new(), // caller fills in credentials
                },
                confidence: 0.85,
                source: "dom_heuristic:login_form",
            });
        }
    }
    None
}

/// Detect a search form and return a Type action targeting the search input.
pub fn detect_search_form(elements: &[DomElement], task: &TaskContext) -> Option<ActionDecision> {
    let query = task.target_data.as_deref().unwrap_or(&task.goal);

    for el in elements {
        if el.tag != "input" && el.tag != "textarea" {
            continue;
        }

        let itype = el.input_type.as_deref().unwrap_or("");
        let name = el.name.as_deref().unwrap_or("").to_lowercase();
        let placeholder = el.placeholder.as_deref().unwrap_or("").to_lowercase();
        let label = el.aria_label.as_deref().unwrap_or("").to_lowercase();

        let is_search = itype.eq_ignore_ascii_case("search")
            || name == "q"
            || name == "query"
            || name == "search"
            || name.contains("search")
            || placeholder.contains("search")
            || label.contains("search");

        if is_search {
            return Some(ActionDecision {
                action: NavAction::Type {
                    selector: element_to_selector(el),
                    value: query.to_string(),
                },
                confidence: 0.88,
                source: "dom_heuristic:search_form",
            });
        }
    }
    None
}

/// Detect article / main content regions and return an ExtractContent action.
pub fn detect_article_content(elements: &[DomElement]) -> Option<ActionDecision> {
    for el in elements {
        let is_article = el.tag == "article"
            || el.tag == "main"
            || el.role.as_deref() == Some("main")
            || el.role.as_deref() == Some("article");

        if is_article {
            return Some(ActionDecision {
                action: NavAction::ExtractContent {
                    selector: element_to_selector(el),
                },
                confidence: 0.80,
                source: "dom_heuristic:article_content",
            });
        }
    }
    None
}

/// Detect pagination links (e.g. "Next", "Next Page", ">", ">>").
pub fn detect_pagination(elements: &[DomElement]) -> Option<ActionDecision> {
    let next_keywords = ["next", "next page", "older", ">", "\u{203a}", "\u{00bb}"];

    for el in elements {
        if el.tag != "a" && el.tag != "button" {
            continue;
        }

        let text_lower = el.text.to_lowercase().trim().to_string();
        let label_lower = el.aria_label.as_deref().unwrap_or("").to_lowercase();
        let class_str: String = el.classes.join(" ").to_lowercase();

        let is_next = next_keywords.iter().any(|kw| text_lower.contains(kw))
            || next_keywords.iter().any(|kw| label_lower.contains(kw))
            || class_str.contains("next");

        if is_next {
            return Some(ActionDecision {
                action: NavAction::Click {
                    selector: element_to_selector(el),
                },
                confidence: 0.82,
                source: "dom_heuristic:pagination",
            });
        }
    }
    None
}

/// Detect popup dismiss buttons (close / X buttons, modals).
pub fn detect_popup_dismiss(elements: &[DomElement]) -> Option<ActionDecision> {
    let dismiss_keywords = [
        "close",
        "dismiss",
        "no thanks",
        "not now",
        "maybe later",
        "skip",
    ];

    for el in elements {
        if el.tag != "button" && el.tag != "a" {
            continue;
        }

        let text_lower = el.text.to_lowercase();
        let label_lower = el.aria_label.as_deref().unwrap_or("").to_lowercase();
        let class_str: String = el.classes.join(" ").to_lowercase();

        let is_dismiss = dismiss_keywords.iter().any(|kw| text_lower.contains(kw))
            || dismiss_keywords.iter().any(|kw| label_lower.contains(kw))
            || class_str.contains("close")
            || class_str.contains("dismiss")
            || class_str.contains("modal-close");

        if is_dismiss {
            return Some(ActionDecision {
                action: NavAction::Click {
                    selector: element_to_selector(el),
                },
                confidence: 0.75,
                source: "dom_heuristic:popup_dismiss",
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a CSS selector for the given element, preferring the most specific
/// attribute available:  #id > [aria-label] > [name] > .classes > tag
pub fn element_to_selector(el: &DomElement) -> String {
    if let Some(ref id) = el.id {
        if !id.is_empty() {
            return format!("#{}", css_escape_ident(id));
        }
    }

    if let Some(ref label) = el.aria_label {
        if !label.is_empty() {
            return format!("{}[aria-label=\"{}\"]", el.tag, css_escape_attr(label));
        }
    }

    if let Some(ref name) = el.name {
        if !name.is_empty() {
            return format!("{}[name=\"{}\"]", el.tag, css_escape_attr(name));
        }
    }

    if !el.classes.is_empty() {
        let class_selector: String = el
            .classes
            .iter()
            .map(|c| format!(".{}", css_escape_ident(c)))
            .collect::<Vec<_>>()
            .join("");
        return format!("{}{}", el.tag, class_selector);
    }

    el.tag.clone()
}

/// Escape a string for use as a CSS identifier (IDs, class names).
fn css_escape_ident(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '.' | ':' | '[' | ']' | '>' | '+' | '~' | '#' | '(' | ')' | ',' | ' ' | '!' => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    result
}

/// Escape a string for use in a CSS attribute value (within double quotes).
fn css_escape_attr(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_element(tag: &str) -> DomElement {
        DomElement {
            tag: tag.into(),
            id: None,
            classes: vec![],
            role: None,
            aria_label: None,
            text: String::new(),
            href: None,
            input_type: None,
            name: None,
            placeholder: None,
        }
    }

    #[test]
    fn test_detect_cookie_banner() {
        let mut btn = make_element("button");
        btn.text = "Accept All".into();
        btn.classes = vec!["cookie-banner-btn".into()];

        let result = detect_cookie_banner(&[btn]);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::Click { selector } => {
                assert!(selector.contains("cookie-banner-btn"));
            }
            other => panic!("Expected Click, got {:?}", other),
        }
        assert!(decision.confidence > 0.8);
    }

    #[test]
    fn test_detect_login_form() {
        let mut email_input = make_element("input");
        email_input.input_type = Some("email".into());
        email_input.name = Some("email".into());

        let mut password_input = make_element("input");
        password_input.input_type = Some("password".into());
        password_input.name = Some("password".into());

        let result = detect_login_form(&[email_input, password_input]);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::Type { selector, .. } => {
                assert!(selector.contains("email"));
            }
            other => panic!("Expected Type, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_search_form() {
        let mut search_input = make_element("input");
        search_input.input_type = Some("search".into());
        search_input.name = Some("q".into());

        let task = TaskContext {
            goal: "search for rust tutorials".into(),
            target_data: Some("rust tutorials".into()),
            page_url: "https://example.com".into(),
            page_title: "Example".into(),
        };

        let result = detect_search_form(&[search_input], &task);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::Type { value, .. } => {
                assert_eq!(value, "rust tutorials");
            }
            other => panic!("Expected Type, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_article_content() {
        let article = make_element("article");

        let result = detect_article_content(&[article]);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::ExtractContent { selector } => {
                assert_eq!(selector, "article");
            }
            other => panic!("Expected ExtractContent, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_pagination() {
        let mut next_link = make_element("a");
        next_link.text = "Next".into();
        next_link.href = Some("/page/2".into());

        let result = detect_pagination(&[next_link]);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::Click { .. } => {}
            other => panic!("Expected Click, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_popup_dismiss() {
        let mut close_btn = make_element("button");
        close_btn.aria_label = Some("close".into());

        let result = detect_popup_dismiss(&[close_btn]);
        assert!(result.is_some());
        let decision = result.unwrap();
        match &decision.action {
            NavAction::Click { selector } => {
                assert!(selector.contains("close"));
            }
            other => panic!("Expected Click, got {:?}", other),
        }
    }

    #[test]
    fn test_element_to_selector_id() {
        let mut el = make_element("div");
        el.id = Some("myid".into());

        assert_eq!(element_to_selector(&el), "#myid");
    }

    #[test]
    fn test_no_match_returns_none() {
        let div = make_element("div");

        let task = TaskContext {
            goal: "do something".into(),
            target_data: None,
            page_url: "https://example.com".into(),
            page_title: "Example".into(),
        };

        let result = dom_heuristic_action(&task, &[div]);
        assert!(result.is_none());
    }
}
