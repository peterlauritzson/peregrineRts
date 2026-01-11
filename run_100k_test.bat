@echo off
cargo test --release test_100k_units_stress -- --nocapture --test-threads=1 --ignored > test_batch_result.txt 2>&1
echo Test completed.
