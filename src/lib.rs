pub mod game;

// Dummy macros for old profiling code - will be removed in cleanup pass
#[macro_export]
macro_rules! profile_start {
    () => { std::time::Instant::now() };
}

#[macro_export]
macro_rules! profile_end {
    ($start:expr, $name:expr) => {};
}
