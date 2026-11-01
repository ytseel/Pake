//! Brave's adblock-rust integration used by Pake's Windows WebView2 host.
mod engine;
mod filters;

pub use engine::{AdblockState, BlockStats};
pub use filters::refresh_filter_cache;
