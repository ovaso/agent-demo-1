//! Bounded, incremental Markdown rendering for ANSI terminals.
//!
//! Environment detection and terminal ownership belong to the caller. See the
//! crate README for streaming limits and upstream attribution.

mod ansi;
mod code;
mod config;
mod markdown;
mod math;
mod stream;
mod upstream;

pub use config::{Options, WidthMode};
pub use stream::Renderer;
