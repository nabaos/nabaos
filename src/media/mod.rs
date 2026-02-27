//! NyayaStudio — unified multimodal creative engine.
//!
//! Provides image, video, audio, and slide generation through
//! trait-based provider abstraction. Users describe the output;
//! NabaOS picks the best available provider.

pub mod cost_estimator;
pub mod engine;
pub mod providers;
pub mod reveal_template;
pub mod shot_planner;
pub mod slides;
pub mod tools;
pub mod traits;
pub mod video_looper;
