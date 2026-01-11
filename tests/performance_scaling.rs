/// Progressive Performance Scaling Tests
///
/// This test suite validates that the simulation can maintain target tick rates
/// at increasing scales. Each test only runs if the previous test succeeded,
/// allowing us to identify the exact point where performance degrades.
///
/// Tests progress from:
/// - 100 units @ 10 TPS
/// - 1M units @ 100 TPS (final goal)
///
/// ## Pathfinding Test Modes
///
/// By default, all tests run with **chunky pathfinding** (10-20% of units to same point every 10 ticks),
/// which simulates realistic RTS gameplay when players select and move groups of units.
///
/// Override with `PATHFINDING_MODE` environment variable:
/// - `chunky` (default): 10-20% of units to same point every 10 ticks (RTS player selection)
/// - `random`: 0.5% of units to random points every tick (uniform load)
/// - `none`: No pathfinding requests (baseline for comparison)
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
/// ## Usage Examples:
///
/// 1. **Full suite with chunky pathfinding** (default, stop at first failure):
///    ```
///    cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 2. **Baseline without pathfinding**:
///    ```
///    $env:PATHFINDING_MODE="none"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 3. **Random spread-out pathfinding**:
///    ```
///    $env:PATHFINDING_MODE="random"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 4. **Resume from last failure**:
///    ```
///    $env:PERF_TEST_MODE="resume"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 5. **Regression check** (run only previously passing tests):
///    ```
///    $env:PERF_TEST_MODE="regression"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```
///
/// 6. **Reset checkpoint**:
///    ```
///    $env:PERF_TEST_MODE="reset"; cargo test --release --test performance_scaling test_performance_scaling_suite -- --ignored --nocapture
///    ```

use bevy::prelude::*;
use bevy::ecs::system::RunSystemOnce;
use peregrine::game::simulation::components::{
    SimPosition, SimPositionPrev, SimVelocity, SimAcceleration, Collider,
    CachedNeighbors, OccupiedCells, StaticObstacle, layers,
};
use peregrine::game::simulation::resources::{SimConfig, MapFlowField};
use peregrine::game::simulation::collision::CollisionEvent;
use peregrine::game::simulation::physics;
use peregrine::game::simulation::collision;
use peregrine::game::simulation::systems;
use peregrine::game::spatial_hash::SpatialHash;
use peregrine::game::pathfinding::{
    PathRequest, HierarchicalGraph, ConnectedComponents, process_path_requests,
};
use peregrine::game::structures::FlowField;
use peregrine::game::fixed_math::{FixedNum, FixedVec2};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};
use std::sync::{Arc, Mutex};

// Global timing storage for system profiling
#[derive(Resource, Default, Clone)]
struct SystemTimings {
    spatial_hash_ms: Arc<Mutex<f32>>,
    collision_detect_ms: Arc<Mutex<f32>>,
    collision_resolve_ms: Arc<Mutex<f32>>,
    physics_ms: Arc<Mutex<f32>>,
    pathfinding_ms: Arc<Mutex<f32>>,
}

/// Pathfinding request pattern for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathfindingPattern {
    /// No pathfinding requests (baseline performance)
    None,
    /// Random spread-out requests: 0.5% of units to random points every tick
    RandomSpreadOut,
    /// Chunky requests: 10-20% of units to same point same tick (DEFAULT)
    ChunkyRequests,
}

impl PathfindingPattern {
    fn from_env() -> Self {
        match std::env::var("PATHFINDING_MODE").as_deref() {
            Ok("none") => PathfindingPattern::None,
            Ok("random") => PathfindingPattern::RandomSpreadOut,
            Ok("chunky") | _ => PathfindingPattern::ChunkyRequests, // Default to chunky
        }
    }
}

/// Deterministic pathfinding request generator
#[derive(Resource)]
struct PathRequestGenerator {
    rng: fastrand::Rng,
    map_size: f32,
    pattern: PathfindingPattern,
    tick_counter: u32,
}

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

/// System-level performance metrics
#[derive(Debug, Clone, Default)]
struct SystemMetrics {
    spatial_hash_ms: f32,
    collision_detect_ms: f32,
    collision_resolve_ms: f32,
    physics_ms: f32,
    pathfinding_ms: f32,
    // Max times for detecting spikes
    spatial_hash_max_ms: f32,
    collision_detect_max_ms: f32,
    collision_resolve_max_ms: f32,
    physics_max_ms: f32,
    pathfinding_max_ms: f32,
}

impl SystemMetrics {
    fn total_ms(&self) -> f32 {
        self.spatial_hash_ms + self.collision_detect_ms + 
        self.collision_resolve_ms + self.physics_ms + self.pathfinding_ms
    }
    
    fn add(&mut self, other: &SystemMetrics) {
        self.spatial_hash_ms += other.spatial_hash_ms;
        self.collision_detect_ms += other.collision_detect_ms;
        self.collision_resolve_ms += other.collision_resolve_ms;
        self.physics_ms += other.physics_ms;
        self.pathfinding_ms += other.pathfinding_ms;
    }
    
    fn update_max(&mut self, other: &SystemMetrics) {
        self.spatial_hash_max_ms = self.spatial_hash_max_ms.max(other.spatial_hash_ms);
        self.collision_detect_max_ms = self.collision_detect_max_ms.max(other.collision_detect_ms);
        self.collision_resolve_max_ms = self.collision_resolve_max_ms.max(other.collision_resolve_ms);
        self.physics_max_ms = self.physics_max_ms.max(other.physics_ms);
        self.pathfinding_max_ms = self.pathfinding_max_ms.max(other.pathfinding_ms);
    }
    
    fn print_breakdown(&self) {
        let total = self.total_ms();
        if total < 0.001 {
            println!("  System breakdown: (too fast to measure)");
            return;
        }
        
        println!("  System breakdown (per tick, from sampled ticks):");
        println!("    System              Avg (ms)   Max (ms)    Avg %");
        println!("    ----------------    --------   --------   ------");
        println!("    Spatial Hash        {:8.3}   {:8.3}   {:5.1}%", 
            self.spatial_hash_ms, self.spatial_hash_max_ms, (self.spatial_hash_ms / total) * 100.0);
        println!("    Collision Detect    {:8.3}   {:8.3}   {:5.1}%", 
            self.collision_detect_ms, self.collision_detect_max_ms, (self.collision_detect_ms / total) * 100.0);
        println!("    Collision Resolve   {:8.3}   {:8.3}   {:5.1}%", 
            self.collision_resolve_ms, self.collision_resolve_max_ms, (self.collision_resolve_ms / total) * 100.0);
        println!("    Physics             {:8.3}   {:8.3}   {:5.1}%", 
            self.physics_ms, self.physics_max_ms, (self.physics_ms / total) * 100.0);
        println!("    Pathfinding         {:8.3}   {:8.3}   {:5.1}%", 
            self.pathfinding_ms, self.pathfinding_max_ms, (self.pathfinding_ms / total) * 100.0);
        
        // Highlight the slowest system by average
        let max_time = self.spatial_hash_ms
            .max(self.collision_detect_ms)
            .max(self.collision_resolve_ms)
            .max(self.physics_ms)
            .max(self.pathfinding_ms);
        
        if (self.spatial_hash_ms - max_time).abs() < 0.001 {
            println!("  ⚠ PRIMARY BOTTLENECK (avg): Spatial Hash (O(n) - entity insertion/updates)");
        } else if (self.collision_detect_ms - max_time).abs() < 0.001 {
            println!("  ⚠ PRIMARY BOTTLENECK (avg): Collision Detection (O(n*neighbors) - dominant at scale)");
        } else if (self.collision_resolve_ms - max_time).abs() < 0.001 {
            println!("  ⚠ PRIMARY BOTTLENECK (avg): Collision Resolution (O(collisions))");
        } else if (self.physics_ms - max_time).abs() < 0.001 {
            println!("  ⚠ PRIMARY BOTTLENECK (avg): Physics (O(n) - velocity application)");
        } else if (self.pathfinding_ms - max_time).abs() < 0.001 {
            println!("  ⚠ PRIMARY BOTTLENECK (avg): Pathfinding (hierarchical A* + cluster flow)");
        }
        
        // Also highlight worst spike
        let max_spike = self.spatial_hash_max_ms
            .max(self.collision_detect_max_ms)
            .max(self.collision_resolve_max_ms)
            .max(self.physics_max_ms)
            .max(self.pathfinding_max_ms);
        
        if max_spike > 0.0 {
            if (self.spatial_hash_max_ms - max_spike).abs() < 0.001 {
                println!("  ⚠ WORST SPIKE (max): Spatial Hash ({:.2}ms)", max_spike);
            } else if (self.collision_detect_max_ms - max_spike).abs() < 0.001 {
                println!("  ⚠ WORST SPIKE (max): Collision Detection ({:.2}ms)", max_spike);
            } else if (self.collision_resolve_max_ms - max_spike).abs() < 0.001 {
                println!("  ⚠ WORST SPIKE (max): Collision Resolution ({:.2}ms)", max_spike);
            } else if (self.physics_max_ms - max_spike).abs() < 0.001 {
                println!("  ⚠ WORST SPIKE (max): Physics ({:.2}ms)", max_spike);
            } else if (self.pathfinding_max_ms - max_spike).abs() < 0.001 {
                println!("  ⚠ WORST SPIKE (max): Pathfinding ({:.2}ms)", max_spike);
            }
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
    system_metrics: SystemMetrics,
    pathfinding_pattern: PathfindingPattern, // Track which pattern was used
}

impl PerfTestResult {
    fn new(config: PerfTestConfig, actual_ticks: u32, elapsed: Duration, metrics: SystemMetrics, pathfinding_pattern: PathfindingPattern) -> Self {
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
            system_metrics: metrics,
            pathfinding_pattern,
        }
    }
    
    fn print_summary(&self) {
        let status = if self.passed { "✓ PASS" } else { "✗ FAIL" };
        println!("\n{} - {}", status, self.config.name);
        println!("  Units: {}", self.config.unit_count);
        println!("  Target TPS: {}", self.config.target_tps);
        println!("  Pathfinding: {:?}", self.pathfinding_pattern);
        println!("  Duration: {:.2}s", self.elapsed_secs);
        println!("  Ticks: {} / {} ({:.1}%)",
            self.actual_ticks,
            self.expected_ticks,
            (self.actual_ticks as f32 / self.expected_ticks as f32) * 100.0
        );
        println!("  Actual TPS: {:.1}", self.actual_tps);
        self.system_metrics.print_breakdown();
    }
}

/// Test definitions - progressive scaling from small to massive
/// All tests use the pathfinding pattern specified by PATHFINDING_MODE env var (default: chunky)
const PERF_TESTS: &[PerfTestConfig] = &[
    // Phase 1: Small scale validation (10 TPS)
    PerfTestConfig {
        name: "100 units @ 10 TPS",
        unit_count: 100,
        target_tps: 10,
        test_ticks: 50,
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
        name: "1M units @ 100 TPS (FINAL GOAL)",
        unit_count: 1_000_000,
        target_tps: 100,
        test_ticks: 100,
    },
];

// Timing wrapper systems
fn timed_spatial_hash(world: &mut World) {
    let t = Instant::now();
    world.run_system_once(systems::update_spatial_hash).ok();
    let elapsed = t.elapsed().as_secs_f32() * 1000.0;
    if let Some(timings) = world.get_resource::<SystemTimings>() {
        *timings.spatial_hash_ms.lock().unwrap() = elapsed;
    }
}

fn timed_collision_detect(world: &mut World) {
    let t = Instant::now();
    world.run_system_once(collision::detect_collisions).ok();
    let elapsed = t.elapsed().as_secs_f32() * 1000.0;
    if let Some(timings) = world.get_resource::<SystemTimings>() {
        *timings.collision_detect_ms.lock().unwrap() = elapsed;
    }
}

fn timed_collision_resolve(world: &mut World) {
    let t = Instant::now();
    world.run_system_once(collision::resolve_collisions).ok();
    let elapsed = t.elapsed().as_secs_f32() * 1000.0;
    if let Some(timings) = world.get_resource::<SystemTimings>() {
        *timings.collision_resolve_ms.lock().unwrap() = elapsed;
    }
}

fn timed_physics(world: &mut World) {
    let t = Instant::now();
    world.run_system_once(physics::apply_velocity).ok();
    let elapsed = t.elapsed().as_secs_f32() * 1000.0;
    if let Some(timings) = world.get_resource::<SystemTimings>() {
        *timings.physics_ms.lock().unwrap() = elapsed;
    }
}

fn timed_pathfinding(world: &mut World) {
    let t = Instant::now();
    world.run_system_once(process_path_requests).ok();
    let elapsed = t.elapsed().as_secs_f32() * 1000.0;
    if let Some(timings) = world.get_resource::<SystemTimings>() {
        *timings.pathfinding_ms.lock().unwrap() = elapsed;
    }
}

/// Generate pathfinding requests based on the configured pattern
fn generate_pathfinding_requests(
    mut generator: ResMut<PathRequestGenerator>,
    query: Query<Entity, With<SimPosition>>,
    mut writer: MessageWriter<PathRequest>,
) {
    generator.tick_counter += 1;
    
    match generator.pattern {
        PathfindingPattern::None => {
            // No pathfinding requests
        }
        PathfindingPattern::RandomSpreadOut => {
            // Request paths for 0.5% of units to random points every tick
            let entities: Vec<Entity> = query.iter().collect();
            let request_count = (entities.len() as f32 * 0.005).max(1.0) as usize;
            
            for _ in 0..request_count {
                if let Some(&entity) = entities.get(generator.rng.usize(0..entities.len())) {
                    let half_size = generator.map_size / 2.0;
                    let goal_x = generator.rng.f32() * generator.map_size - half_size;
                    let goal_y = generator.rng.f32() * generator.map_size - half_size;
                    
                    writer.write(PathRequest {
                        entity,
                        start: FixedVec2::ZERO, // System will use actual position
                        goal: FixedVec2::new(
                            FixedNum::from_num(goal_x),
                            FixedNum::from_num(goal_y),
                        ),
                    });
                }
            }
        }
        PathfindingPattern::ChunkyRequests => {
            // Every 10 ticks, request paths for 10-20% of units to same point
            if generator.tick_counter % 10 == 0 {
                let entities: Vec<Entity> = query.iter().collect();
                let selection_pct = 0.10 + generator.rng.f32() * 0.10; // 10-20%
                let request_count = (entities.len() as f32 * selection_pct) as usize;
                
                // Pick one common goal for all selected units
                let half_size = generator.map_size / 2.0;
                let goal_x = generator.rng.f32() * generator.map_size - half_size;
                let goal_y = generator.rng.f32() * generator.map_size - half_size;
                let common_goal = FixedVec2::new(
                    FixedNum::from_num(goal_x),
                    FixedNum::from_num(goal_y),
                );
                
                // Randomly select units and give them all the same goal
                let mut selected_indices: Vec<usize> = (0..entities.len()).collect();
                generator.rng.shuffle(&mut selected_indices);
                
                for &idx in selected_indices.iter().take(request_count) {
                    if let Some(&entity) = entities.get(idx) {
                        writer.write(PathRequest {
                            entity,
                            start: FixedVec2::ZERO,
                            goal: common_goal,
                        });
                    }
                }
            }
        }
    }
}

/// Run a single performance test
fn run_perf_test(config: PerfTestConfig) -> PerfTestResult {
    let mut app = App::new();
    
    // Get pathfinding pattern from environment (default: chunky)
    let pathfinding_pattern = PathfindingPattern::from_env();
    
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
        FixedNum::from_num(40.0), // Cell size for spatial partitioning
    ));
    
    // Add simulation config
    app.insert_resource(SimConfig {
        tick_rate: config.target_tps as f64,
        ..Default::default()
    });
    
    // Add collision events
    app.add_message::<CollisionEvent>();
    
    // Add timing resource
    app.insert_resource(SystemTimings::default());
    
    // Initialize pathfinding resources
    let flow_field = FlowField::new(
        map_size as usize,
        map_size as usize,
        FixedNum::from_num(1.0), // 1 world unit per cell
        FixedVec2::new(
            FixedNum::from_num(-map_size / 2.0),
            FixedNum::from_num(-map_size / 2.0),
        ),
    );
    app.insert_resource(MapFlowField(flow_field));
    
    // Build pathfinding graph and connectivity components
    let mut hierarchical_graph = HierarchicalGraph::default();
    let mut connected_components = ConnectedComponents::default();
    {
        let flow_field_ref = app.world().resource::<MapFlowField>();
        hierarchical_graph.build_graph_sync(&flow_field_ref.0);
        connected_components.build_from_graph(&hierarchical_graph);
    }
    app.insert_resource(hierarchical_graph);
    app.insert_resource(connected_components);
    
    // Add pathfinding message
    app.add_message::<PathRequest>();
    
    // Add pathfinding request generator (deterministic)
    app.insert_resource(PathRequestGenerator {
        rng: fastrand::Rng::with_seed(123), // Different seed from unit spawning
        map_size,
        pattern: pathfinding_pattern,
        tick_counter: 0,
    });
    
    // Add simulation systems with timing wrappers
    // These will record their execution time into the SystemTimings resource
    app.add_systems(Update, timed_spatial_hash);
    app.add_systems(Update, timed_collision_detect.after(timed_spatial_hash));
    app.add_systems(Update, timed_collision_resolve.after(timed_collision_detect));
    app.add_systems(Update, timed_physics.after(timed_collision_resolve));
    
    // Add pathfinding systems (only run if pattern is not None)
    if pathfinding_pattern != PathfindingPattern::None {
        app.add_systems(Update, generate_pathfinding_requests.after(timed_physics));
        app.add_systems(Update, timed_pathfinding.after(generate_pathfinding_requests));
    }
    
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
    
    // Accumulate system metrics by sampling every 10th tick (to reduce overhead)
    let mut total_metrics = SystemMetrics::default();
    let sample_frequency = 10;
    let mut sample_count = 0;
    
    while tick_count < target_ticks {
        // Safety check: abort if taking too long
        if start.elapsed() > max_wall_time {
            eprintln!("⚠ Test aborted: exceeded {} second timeout", max_wall_time.as_secs());
            break;
        }
        
        // Sample system performance every Nth tick to measure bottlenecks
        if tick_count % sample_frequency == 0 {
            let sampled_metrics = profile_tick(&mut app);
            total_metrics.add(&sampled_metrics);
            total_metrics.update_max(&sampled_metrics);
            sample_count += 1;
        } else {
            // Normal tick without profiling overhead
            app.update();
        }
        
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
    
    // Calculate average metrics per sampled tick
    let avg_metrics = if sample_count > 0 {
        SystemMetrics {
            spatial_hash_ms: total_metrics.spatial_hash_ms / sample_count as f32,
            collision_detect_ms: total_metrics.collision_detect_ms / sample_count as f32,
            collision_resolve_ms: total_metrics.collision_resolve_ms / sample_count as f32,
            physics_ms: total_metrics.physics_ms / sample_count as f32,
            pathfinding_ms: total_metrics.pathfinding_ms / sample_count as f32,
            // Max values are already tracked
            spatial_hash_max_ms: total_metrics.spatial_hash_max_ms,
            collision_detect_max_ms: total_metrics.collision_detect_max_ms,
            collision_resolve_max_ms: total_metrics.collision_resolve_max_ms,
            physics_max_ms: total_metrics.physics_max_ms,
            pathfinding_max_ms: total_metrics.pathfinding_max_ms,
        }
    } else {
        SystemMetrics::default()
    };
    
    let elapsed = start.elapsed();
    PerfTestResult::new(config, tick_count, elapsed, avg_metrics, pathfinding_pattern)
}

/// Profile a single tick by running systems and estimating their individual costs
///
/// NOTE: These are now REAL measurements, not estimates!
/// Each system is timed individually using wrapper functions that record their execution time.
///
/// For precise per-system measurements, consider:
/// - Using Bevy's built-in diagnostics
/// - Adding manual instrumentation to system code
/// - Using a profiler like tracy or puffin
///
/// Despite being estimates, they accurately reflect the relative costs and bottlenecks.
fn profile_tick(app: &mut App) -> SystemMetrics {
    let mut metrics = SystemMetrics::default();
    
    // Run the tick - our wrapper systems will record timings
    app.update();
    
    // Retrieve recorded timings from the resource
    if let Some(timings) = app.world().get_resource::<SystemTimings>() {
        metrics.spatial_hash_ms = *timings.spatial_hash_ms.lock().unwrap();
        metrics.collision_detect_ms = *timings.collision_detect_ms.lock().unwrap();
        metrics.collision_resolve_ms = *timings.collision_resolve_ms.lock().unwrap();
        metrics.physics_ms = *timings.physics_ms.lock().unwrap();
        metrics.pathfinding_ms = *timings.pathfinding_ms.lock().unwrap();
    }
    
    metrics
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
    let pathfinding_pattern = PathfindingPattern::from_env();
    
    println!("\n=== PERFORMANCE SCALING TEST SUITE ===");
    println!("Mode: {:?}", mode);
    println!("Pathfinding: {:?}", pathfinding_pattern);
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
    println!("Pathfinding: {:?}", pathfinding_pattern);
    println!("Tests run: {}", results.len());
    println!("Tests passed: {}", results.iter().filter(|r| r.passed).count());
    println!("Tests failed: {}", results.iter().filter(|r| !r.passed).count());
    
    if let Some(index) = last_passed_index {
        println!("✓ Maximum validated scale: {} (test {}/{})", 
            PERF_TESTS[index].name, index + 1, PERF_TESTS.len());
    }
    
    // Analyze bottleneck trends across all tests
    if !results.is_empty() {
        println!("\n=== BOTTLENECK ANALYSIS ===");
        for result in &results {
            let metrics = &result.system_metrics;
            let total = metrics.total_ms();
            if total < 0.001 {
                continue;
            }
            
            let max_pct = (metrics.spatial_hash_ms / total).max(metrics.collision_detect_ms / total)
                .max(metrics.collision_resolve_ms / total)
                .max(metrics.physics_ms / total) * 100.0;
            
            let bottleneck = if (metrics.spatial_hash_ms / total * 100.0 - max_pct).abs() < 0.1 {
                "Spatial Hash"
            } else if (metrics.collision_detect_ms / total * 100.0 - max_pct).abs() < 0.1 {
                "Collision Detect"
            } else if (metrics.collision_resolve_ms / total * 100.0 - max_pct).abs() < 0.1 {
                "Collision Resolve"
            } else {
                "Physics"
            };
            
            println!("  {} ({} units): {} ({:.0}%)",
                result.config.name,
                result.config.unit_count,
                bottleneck,
                max_pct
            );
        }
        
        println!("\nNote: System breakdown uses REAL per-system timing measurements.");
        println!("      Each system is timed individually during sampled ticks.");
    }
    
    if all_passed && end_index == PERF_TESTS.len() && results.len() == PERF_TESTS.len() {
        println!("\n🎉 ALL TESTS PASSED! 1M units @ 100 TPS achieved!");
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
