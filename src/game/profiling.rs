//! Performance profiling utilities
//! 
//! These are only compiled when the `perf_stats` feature is enabled.
//! Zero overhead when disabled.

// Re-export the profile macro
pub use peregrine_macros::profile;
