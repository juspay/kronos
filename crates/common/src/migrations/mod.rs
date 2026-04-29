//! Embedded migration templates plus the renderer.
//!
//! Plan 1 only ships the renderer; the embedded migration list and the
//! `apply()` entry point are added in this same plan as a later task.

pub mod render;

pub use render::{render, RenderError};
