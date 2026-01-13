pub mod game;

// ============================================================================
// Profiling Macros
// ============================================================================

/// Conditionally log messages based on tick interval when perf_stats feature is enabled.
/// 
/// This macro logs a message every 100 ticks. When the perf_stats feature is disabled,
/// this macro compiles to nothing - zero runtime cost.
/// 
/// # Example
/// ```
/// profile_log!(tick, "Processed {} entities", query.iter().len());
/// ```
/// 
/// # Zero-Cost Abstraction
/// When compiled without the `perf_stats` feature, this expands to an empty block.
/// Even the arguments (e.g., `query.iter().len()`) are not evaluated.
#[macro_export]
#[cfg(feature = "perf_stats")]
macro_rules! profile_log {
    ($tick:expr, $($arg:tt)*) => {
        if $tick.0 % 100 == 0 {
            bevy::prelude::info!($($arg)*);
        }
    };
}

#[macro_export]
#[cfg(not(feature = "perf_stats"))]
macro_rules! profile_log {
    ($tick:expr, $($arg:tt)*) => {};
}
