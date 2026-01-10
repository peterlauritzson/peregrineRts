/// Progressive Performance Scaling Tests
///
/// This test suite validates that the simulation can maintain target tick rates
/// at increasing scales. Each test only runs if the previous test succeeded,
/// allowing us to identify the exact point where performance degrades.
///
/// Tests progress from:
/// - 100 units @ 10 TPS
/// - 1M units @ 100 TPS
/// - 10M units @ 100 TPS (final goal)
///
/// ## CRITICAL BUG FIX (Jan 2026)
///
/// **Problem:** Initial performance tests showed unrealistically fast execution:
/// - 100 units: 0.02ms/tick (impossible - suggests no work being done)
/// - 10M units passing instantly with 0.00s duration
/// - All tests showed identical performance regardless of unit count
///
/// **Root Cause:** Simulation systems were NOT running at all!
/// 1. Systems were added to `FixedUpdate` schedule
/// 2. Tests call `app.update()` which runs `Update` schedule only
/// 3. `FixedUpdate` only runs when `Time<Fixed>` accumulates enough delta
/// 4. Result: Spatial hash empty, no collision detection, no physics
///
/// **Secondary Bug:** Spatial hash bounds checking had integer underflow bug
/// - When entity slightly outside map bounds, `max_col` could be negative isize
/// - Casting negative isize to usize caused underflow (becomes huge number)
/// - Fix: Added `.max(0)` AFTER `.min()` to prevent underflow
///
/// **Solution:** Changed systems to run in `Update` schedule for tests
/// - Real game uses `FixedUpdate` with proper time stepping
/// - Tests use `Update` for deterministic tick-by-tick execution
///
/// **Performance After Fix:**
/// - 100 units: 0.04ms/tick (realistic - 25x slower than broken version)
/// - 10k units: 1.29ms/tick (shows proper scaling with unit count)
/// - Spatial hash now populated: ~1.4 entries per entity (multi-cell coverage)
///
/// ## Usage Modes:
///
/// 1. **Full suite** (stop at first failure):
///    ```
///    cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 2. **Resume from last failure**:
///    ```
///    $env:PERF_TEST_MODE="resume"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 3. **Regression check** (run only previously passing tests):
///    ```
///    $env:PERF_TEST_MODE="regression"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 4. **Reset checkpoint**:
///    ```
///    $env:PERF_TEST_MODE="reset"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```

use bevy::prelude::*;
use peregrine::game::simulation::components::{
    SimPosition, SimPositionPrev, SimVelocity, SimAcceleration, Collider,
    CachedNeighbors, OccupiedCells, StaticObstacle, layers,
};
use peregrine::game::simulation::resources::SimConfig;
use peregrine::game::simulation::collision::CollisionEvent;
use peregrine::game::simulation::physics;
use peregrine::game::simulation::collision;
use peregrine::game::simulation::systems;
use peregrine::game::spatial_hash::SpatialHash;
use peregrine::game::fixed_math::{FixedNum, FixedVec2};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};

/// Configuration for a single performance test
#[derive(Debug, Clone)]
struct PerfTestConfig {
    name: &'static str,
    unit_count: usize,
    target_tps: u32,
    test_ticks: u32, // Number of ticks to run
}

/// Checkpoint file for tracking test progress
#[derive(Debug, Serialize, Deserialize)]
struct TestCheckpoint {
    /// Index of the last test that passed (0-based)
    last_passed_index: Option<usize>,
    /// Name of the last test that passed
    last_passed_name: Option<String>,
    /// Timestamp of last run
    timestamp: String,
}

impl TestCheckpoint {
    fn load() -> Self {
        let path = Self::checkpoint_path();
        if path.exists() {
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(checkpoint) = serde_json::from_str(&contents) {
                    return checkpoint;
                }
            }
        }
        Self::default()
    }
    
    fn save(&self) {
        let path = Self::checkpoint_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }
    
    fn reset() {
        let path = Self::checkpoint_path();
        let _ = fs::remove_file(&path);
    }
    
    fn checkpoint_path() -> PathBuf {
        PathBuf::from("target").join("perf_test_checkpoint.json")
    }
}

impl Default for TestCheckpoint {
    fn default() -> Self {
        Self {
            last_passed_index: None,
            last_passed_name: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Test execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestMode {
    /// Run all tests from the beginning, stop at first failure
    Full,
    /// Run only tests that passed before (regression check)
    Regression,
    /// Resume from the test after the last passed one
    Resume,
    /// Reset checkpoint and run full suite
    Reset,
}

impl TestMode {
    fn from_env() -> Self {
        match std::env::var("PERF_TEST_MODE").as_deref() {
            Ok("regression") => TestMode::Regression,
            Ok("resume") => TestMode::Resume,
            Ok("reset") => TestMode::Reset,
            _ => TestMode::Full,
        }
    }
}

/// Result of a performance test
#[derive(Debug)]
struct PerfTestResult {
    config: PerfTestConfig,
    actual_ticks: u32,
    expected_ticks: u32,
    actual_tps: f32,
    elapsed_secs: f32,
    passed: bool,
}

impl PerfTestResult {
    fn new(config: PerfTestConfig, actual_ticks: u32, elapsed: Duration) -> Self {
        let elapsed_secs = elapsed.as_secs_f32();
        let test_ticks = config.test_ticks;
        let target_tps = config.target_tps;
        let actual_tps = actual_ticks as f32 / elapsed_secs;
        
        // Test passes if we achieved at least 95% of target tick rate
        let achieved_tps_ratio = actual_tps / target_tps as f32;
        let passed = achieved_tps_ratio >= 0.95;
        
        Self {
            config,
            actual_ticks,
            expected_ticks: test_ticks,
            actual_tps,
            elapsed_secs,
            passed,
        }
    }
    
    fn print_summary(&self) {
        let status = if self.passed { "✓ PASS" } else { "✗ FAIL" };
        println!("\n{} - {}", status, self.config.name);
        println!("  Units: {}", self.config.unit_count);
        println!("  Target TPS: {}", self.config.target_tps);
        println!("  Duration: {:.2}s", self.elapsed_secs);
        println!("  Ticks: {} / {} ({:.1}%)",
            self.actual_ticks,
            self.expected_ticks,
            (self.actual_ticks as f32 / self.expected_ticks as f32) * 100.0
        );
        println!("  Actual TPS: {:.1}", self.actual_tps);
    }
}

/// Test definitions - progressive scaling from small to massive
const PERF_TESTS: &[PerfTestConfig] = &[
    // Phase 1: Small scale validation (10 TPS)
    PerfTestConfig {
        name: "100 units @ 10 TPS",
        unit_count: 100,
        target_tps: 10,
        test_ticks: 50, // Run 50 ticks (~5 seconds at target rate)
    },
    PerfTestConfig {
        name: "1k units @ 10 TPS",
        unit_count: 1_000,
        target_tps: 10,
        test_ticks: 50,
    },
    PerfTestConfig {
        name: "10k units @ 10 TPS",
        unit_count: 10_000,
        target_tps: 10,
        test_ticks: 50,
    },
    
    // Phase 2: Medium scale with faster ticks (50 TPS)
    PerfTestConfig {
        name: "1k units @ 50 TPS",
        unit_count: 1_000,
        target_tps: 50,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "10k units @ 50 TPS",
        unit_count: 10_000,
        target_tps: 50,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "100k units @ 50 TPS",
        unit_count: 100_000,
        target_tps: 50,
        test_ticks: 100,
    },
    
    // Phase 3: Large scale with target production rate (100 TPS)
    PerfTestConfig {
        name: "10k units @ 100 TPS",
        unit_count: 10_000,
        target_tps: 100,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "100k units @ 100 TPS",
        unit_count: 100_000,
        target_tps: 100,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "500k units @ 100 TPS",
        unit_count: 500_000,
        target_tps: 100,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "1M units @ 100 TPS",
        unit_count: 1_000_000,
        target_tps: 100,
        test_ticks: 100,
    },
    
    // Phase 4: The final goals
    PerfTestConfig {
        name: "5M units @ 100 TPS",
        unit_count: 5_000_000,
        target_tps: 100,
        test_ticks: 100,
    },
    PerfTestConfig {
        name: "10M units @ 100 TPS (FINAL GOAL)",
        unit_count: 10_000_000,
        target_tps: 100,
        test_ticks: 100,
    },
];

/// Run a single performance test
fn run_perf_test(config: PerfTestConfig) -> PerfTestResult {
    let mut app = App::new();
    
    // Minimal plugins - just what we need for simulation
    app.add_plugins(MinimalPlugins);
    
    // Set up fixed timestep based on target TPS
    let tick_duration = Duration::from_secs_f32(1.0 / config.target_tps as f32);
    app.insert_resource(Time::<Fixed>::from_duration(tick_duration));
    
    // Initialize simulation resources
    let map_size = calculate_map_size(config.unit_count);
    app.insert_resource(SpatialHash::new(
        FixedNum::from_num(map_size),
        FixedNum::from_num(map_size),
        FixedNum::from_num(5.0), // Cell size for spatial partitioning
    ));
    
    // Add simulation config
    app.insert_resource(SimConfig {
        tick_rate: config.target_tps as f64,
        ..Default::default()
    });
    
    // Add collision events
    app.add_message::<CollisionEvent>();
    
    // Add REAL simulation systems in the correct order
    // Use Update schedule for tests (FixedUpdate requires time accumulation)
    app.add_systems(Update, (
        systems::update_spatial_hash,
        collision::detect_collisions,
        collision::resolve_collisions,
        physics::apply_velocity,
    ).chain());
    
    // Spawn units with ALL necessary components for realistic simulation
    let half_size = map_size / 2.0;
    let mut rng = fastrand::Rng::with_seed(42); // Deterministic
    
    for _ in 0..config.unit_count {
        let x = rng.f32() * map_size - half_size;
        let y = rng.f32() * map_size - half_size;
        let vx = (rng.f32() - 0.5) * 20.0; // Higher velocity to trigger more collisions
        let vy = (rng.f32() - 0.5) * 20.0;
        
        let pos = FixedVec2::new(FixedNum::from_num(x), FixedNum::from_num(y));
        
        app.world_mut().spawn((
            SimPosition(pos),
            SimPositionPrev(pos),
            SimVelocity(FixedVec2::new(
                FixedNum::from_num(vx),
                FixedNum::from_num(vy),
            )),
            SimAcceleration(FixedVec2::ZERO),
            Collider::default(), // Collision detection enabled
            CachedNeighbors::default(),
            OccupiedCells::default(),
        ));
    }
    
    // Add some obstacles to make collision detection actually work
    let num_obstacles = (config.unit_count / 100).max(10); // 1% obstacles, min 10
    for _ in 0..num_obstacles {
        let x = rng.f32() * map_size - half_size;
        let y = rng.f32() * map_size - half_size;
        let pos = FixedVec2::new(FixedNum::from_num(x), FixedNum::from_num(y));
        
        app.world_mut().spawn((
            StaticObstacle,
            SimPosition(pos),
            SimPositionPrev(pos),
            Collider {
                radius: FixedNum::from_num(2.0),
                layer: layers::OBSTACLE,
                mask: layers::UNIT,
            },
            OccupiedCells::default(),
        ));
    }
    
    // Run the test for a fixed number of ticks
    let start = Instant::now();
    let max_wall_time = Duration::from_secs(60); // Safety timeout: 60 seconds max
    let target_ticks = config.test_ticks;
    let mut tick_count = 0u32;
    
    while tick_count < target_ticks {
        // Safety check: abort if taking too long
        if start.elapsed() > max_wall_time {
            eprintln!("⚠ Test aborted: exceeded {} second timeout", max_wall_time.as_secs());
            break;
        }
        
        app.update();
        tick_count += 1;
        
        // Validation check on first tick (disabled in normal runs)
        // Uncomment for debugging spatial hash issues
        /*
        if tick_count == 1 {
            let world = app.world_mut();
            if let Some(spatial_hash) = world.get_resource::<SpatialHash>() {
                eprintln!("DEBUG: Spatial hash after tick 1: {} entries in {} cells", 
                          spatial_hash.total_entries(), spatial_hash.non_empty_cells());
            }
        }
        */
        
        // Validation check on tick 10 (disabled in normal runs)
        // Uncomment for debugging collision detection issues
        /*
        if tick_count == 10 {
            let world = app.world_mut();
            let total_neighbors: usize = world.query::<&CachedNeighbors>()
                .iter(world)
                .map(|c| c.neighbors.len())
                .sum();
            eprintln!("DEBUG: Tick 10 - Total cached neighbors: {}", total_neighbors);
        }
        */
    }
    
    let elapsed = start.elapsed();
    PerfTestResult::new(config, tick_count, elapsed)
}

/// Calculate appropriate map size based on unit count
/// Maintains reasonable density (~1 unit per 4 square units)
fn calculate_map_size(unit_count: usize) -> f32 {
    let area = unit_count as f32 * 4.0;
    area.sqrt()
}

#[test]
#[ignore] // This is a long-running performance test
fn test_performance_scaling_suite() {
    let mode = TestMode::from_env();
    
    println!("\n=== PERFORMANCE SCALING TEST SUITE ===");
    println!("Mode: {:?}", mode);
    println!("Progressive validation of simulation tick rate at increasing scales");
    
    // Load checkpoint
    let mut checkpoint = if mode == TestMode::Reset {
        println!("Resetting checkpoint...");
        TestCheckpoint::reset();
        TestCheckpoint::default()
    } else {
        TestCheckpoint::load()
    };
    
    // Display checkpoint info
    if let Some(ref name) = checkpoint.last_passed_name {
        println!("Last successful test: {}", name);
        println!("Last run: {}", checkpoint.timestamp);
    }
    
    // Determine test range
    let (start_index, end_index) = match mode {
        TestMode::Full | TestMode::Reset => {
            println!("Running all tests from the beginning\n");
            (0, PERF_TESTS.len())
        }
        TestMode::Regression => {
            let end = checkpoint.last_passed_index.map(|i| i + 1).unwrap_or(0);
            println!("Running regression tests (0..{})\n", end);
            (0, end)
        }
        TestMode::Resume => {
            let start = checkpoint.last_passed_index.unwrap_or(0);
            println!("Resuming from test {} (re-validating last pass)..{}\n", start, PERF_TESTS.len());
            (start, PERF_TESTS.len())
        }
    };
    
    if start_index >= PERF_TESTS.len() {
        println!("✓ All tests already passed!");
        return;
    }
    
    let mut results = Vec::new();
    let mut all_passed = true;
    let mut last_passed_index = checkpoint.last_passed_index;
    
    for (index, test_config) in PERF_TESTS.iter().enumerate() {
        if index < start_index || index >= end_index {
            continue;
        }
        
        println!("[{}/{}] Running: {}", index + 1, PERF_TESTS.len(), test_config.name);
        
        let result = run_perf_test(test_config.clone());
        result.print_summary();
        
        let passed = result.passed;
        results.push(result);
        
        if passed {
            last_passed_index = Some(index);
        } else {
            println!("\n⚠ Test failed - stopping test suite");
            all_passed = false;
            break;
        }
    }
    
    // Update checkpoint
    checkpoint.last_passed_index = last_passed_index;
    checkpoint.last_passed_name = last_passed_index
        .map(|i| PERF_TESTS[i].name.to_string());
    checkpoint.timestamp = chrono::Utc::now().to_rfc3339();
    checkpoint.save();
    
    // Print final summary
    println!("\n=== FINAL SUMMARY ===");
    println!("Mode: {:?}", mode);
    println!("Tests run: {}", results.len());
    println!("Tests passed: {}", results.iter().filter(|r| r.passed).count());
    println!("Tests failed: {}", results.iter().filter(|r| !r.passed).count());
    
    if let Some(index) = last_passed_index {
        println!("✓ Maximum validated scale: {} (test {}/{})", 
            PERF_TESTS[index].name, index + 1, PERF_TESTS.len());
    }
    
    if all_passed && end_index == PERF_TESTS.len() && results.len() == PERF_TESTS.len() {
        println!("\n🎉 ALL TESTS PASSED! 10M units @ 100 TPS achieved!");
    } else if mode == TestMode::Regression && results.iter().all(|r| r.passed) {
        println!("\n✓ Regression check passed - no performance degradation");
    }
    
    println!("\nCheckpoint saved to: {}", TestCheckpoint::checkpoint_path().display());
    
    // Test fails if any individual test failed
    assert!(all_passed, "Performance test suite failed");
}

/// Individual quick tests for specific scales (can run independently)
#[test]
fn test_100_units_baseline() {
    let config = PerfTestConfig {
        name: "100 units baseline",
        unit_count: 100,
        target_tps: 10,
        test_ticks: 50,
    };
    
    let result = run_perf_test(config);
    result.print_summary();
    assert!(result.passed, "Baseline 100 unit test failed");
}

#[test]
fn test_10k_units_moderate() {
    let config = PerfTestConfig {
        name: "10k units moderate",
        unit_count: 10_000,
        target_tps: 50,
        test_ticks: 100,
    };
    
    let result = run_perf_test(config);
    result.print_summary();
    assert!(result.passed, "10k unit test failed");
}

#[test]
#[ignore] // Long running
fn test_100k_units_stress() {
    let config = PerfTestConfig {
        name: "100k units stress test",
        unit_count: 100_000,
        target_tps: 100,
        test_ticks: 100,
    };
    
    let result = run_perf_test(config);
    result.print_summary();
    assert!(result.passed, "100k unit stress test failed");
}

#[test]
#[ignore] // Very long running
fn test_1m_units_extreme() {
    let config = PerfTestConfig {
        name: "1M units extreme test",
        unit_count: 1_000_000,
        target_tps: 100,
        test_ticks: 100,
    };
    
    let result = run_perf_test(config);
    result.print_summary();
    assert!(result.passed, "1M unit extreme test failed");
}

#[test]
#[ignore] // Ultimate goal test
fn test_10m_units_ultimate_goal() {
    let config = PerfTestConfig {
        name: "10M units @ 100 TPS - ULTIMATE GOAL",
        unit_count: 10_000_000,
        target_tps: 100,
        test_ticks: 100,
    };
    
    let result = run_perf_test(config);
    result.print_summary();
    assert!(result.passed, "10M unit ultimate goal failed");
}
