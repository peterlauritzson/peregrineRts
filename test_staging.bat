@echo off
REM Quick script to run 10K test and grep for stage statistics
cargo test --release test_10k_units_moderate -- --nocapture 2>&1 | findstr /C:"Stage"
