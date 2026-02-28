// NyayaBrowser — browser automation with stealth, vision, and cascade detection.
//
// This module builds on top of `modules::browser` (CDP transport, config, cookies)
// and adds pooled tab management, DOM heuristics, YOLO-based element detection,
// WebBERT page classification, and a cascade combiner for robust detection.

pub mod captcha;
pub mod captcha_solver;
pub mod cascade;
pub mod chrome_pool;
pub mod dom_heuristics;
#[cfg(feature = "bert")]
pub mod element_detector;
pub mod extension_bridge;
pub mod session_store;
pub mod stealth;
#[cfg(feature = "bert")]
pub mod web_bert;
