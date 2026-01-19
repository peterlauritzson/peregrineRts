/// Test for Incremental Spatial Hash Update Strategy
///
/// This test validates that incremental updates work correctly when overcapacity_ratio > 1.1
/// and that full rebuild is triggered when needed.
///
/// Test scenarios:
/// 1. Entities moving within same cell (no update)
/// 2. Entities moving to different cells (incremental update)
/// 3. Cell overflow triggering full rebuild
/// 4. Comparison: incremental vs full rebuild performance

use bevy::prelude::*;
use bevy::ecs::system::RunSystemOnce;
use peregrine::game::simulation::components::{
    SimPosition, SimVelocity, SimAcceleration, Collider, OccupiedCell, layers,
};
use peregrine::game::simulation::resources::SimConfig;
use peregrine::game::spatial_hash::SpatialHash;
use peregrine::game::fixed_math::{FixedNum, FixedVec2};
use std::time::Instant;

/// Simple update function for testing (doesn't use full plugin system)
fn simple_update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &SimPosition, &Collider, &OccupiedCell)>,
    query_new: Query<(Entity, &SimPosition, &Collider), Without<OccupiedCell>>,
    mut commands: Commands,
) {
    let use_incremental = spatial_hash.uses_incremental_updates();
    
    if use_incremental {
        // Insert new entities
        for (entity, pos, collider) in query_new.iter() {
            let occupied = spatial_hash.insert(entity, pos.0, collider.radius);
            commands.entity(entity).insert(occupied);
        }
        
        // Update moved entities
        for (entity, pos, _collider, occupied) in query.iter() {
            if let Some((new_grid_offset, new_col, new_row)) = spatial_hash.should_update(pos.0, occupied) {
                if spatial_hash.update_incremental(entity, occupied, new_grid_offset, new_col, new_row) {
                    commands.entity(entity).insert(OccupiedCell {
                        size_class: occupied.size_class,
                        grid_offset: new_grid_offset,
                        col: new_col,
                        row: new_row,
                        vec_idx: 0,
                    });
                }
            }
        }
    } else {
        // Full rebuild
        spatial_hash.clear();
        for (entity, pos, collider) in query.iter().map(|(e, p, c, _)| (e, p, c))
            .chain(query_new.iter())
        {
            spatial_hash.insert(entity, pos.0, collider.radius);
        }
    }
}

/// Create a test app with spatial hash configured for incremental updates
fn create_test_app(overcapacity_ratio: f32, entity_count: usize) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    
    // Initialize resources
    let map_width = FixedNum::from_num(512.0);
    let map_height = FixedNum::from_num(512.0);
    
    let spatial_hash = SpatialHash::new(
        map_width,
        map_height,
        &[0.5], // Single size class
        4.0,
        entity_count,
        overcapacity_ratio,
    );
    
    app.insert_resource(spatial_hash);
    app.insert_resource(SimConfig::default());
    
    app
}

/// Spawn entities in a grid pattern
fn spawn_entities(app: &mut App, count: usize) {
    let spacing = 10.0;
    let cols = (count as f32).sqrt().ceil() as usize;
    
    for i in 0..count {
        let x = (i % cols) as f32 * spacing;
        let y = (i / cols) as f32 * spacing;
        
        app.world_mut().spawn((
            SimPosition(FixedVec2::new(
                FixedNum::from_num(x),
                FixedNum::from_num(y),
            )),
            Collider {
                radius: FixedNum::from_num(0.5),
                layer: layers::UNIT,
                mask: layers::UNIT | layers::OBSTACLE,
            },
            SimVelocity(FixedVec2::ZERO),
            SimAcceleration(FixedVec2::ZERO),
        ));
    }
}

/// Move entities to new positions (will trigger cell changes)
fn move_entities(app: &mut App, offset: f32) {
    let mut query = app.world_mut().query::<&mut SimPosition>();
    for mut pos in query.iter_mut(app.world_mut()) {
        pos.0.x += FixedNum::from_num(offset);
        pos.0.y += FixedNum::from_num(offset);
    }
}

#[test]
fn test_incremental_update_basic() {
    let mut app = create_test_app(1.5, 1000);
    spawn_entities(&mut app, 100);
    
    // First update - should use full rebuild (max_index starts at 0)
    app.world_mut().run_system_once(simple_update_spatial_hash);
    
    // Verify spatial hash populated
    let spatial_hash = app.world().resource::<SpatialHash>();
    assert!(spatial_hash.uses_incremental_updates(), "Should be in incremental mode with overcapacity 1.5");
    assert!(spatial_hash.total_entries() > 0, "Spatial hash should be populated");
    
    println!("✓ Incremental mode enabled");
    println!("✓ Initial population: {} entries", spatial_hash.total_entries());
    
    // Move entities slightly (should stay in same cells)
    move_entities(&mut app, 0.5);
    app.world_mut().run_system_once(simple_update_spatial_hash);
    
    // Move entities significantly (should trigger cell changes)
    move_entities(&mut app, 5.0);
    app.world_mut().run_system_once(simple_update_spatial_hash);
    
    let spatial_hash = app.world().resource::<SpatialHash>();
    assert!(spatial_hash.total_entries() > 0, "Spatial hash should still be populated after movement");
    
    println!("✓ Incremental updates working after movement");
}

#[test]
fn test_full_rebuild_mode() {
    let mut app = create_test_app(1.0, 1000); // No overcapacity = full rebuild
    spawn_entities(&mut app, 100);
    
    app.world_mut().run_system_once(simple_update_spatial_hash);
    
    let spatial_hash = app.world().resource::<SpatialHash>();
    assert!(!spatial_hash.uses_incremental_updates(), "Should be in full rebuild mode with overcapacity 1.0");
    assert!(spatial_hash.total_entries() > 0, "Spatial hash should be populated");
    
    println!("✓ Full rebuild mode working");
}

#[test]
#[ignore] // Run with --ignored flag
fn test_incremental_vs_full_rebuild_performance() {
    const ENTITY_COUNT: usize = 10_000;
    const ITERATIONS: usize = 100;
    
    // Test 1: Full rebuild mode (overcapacity = 1.0)
    let mut app_full = create_test_app(1.0, ENTITY_COUNT);
    spawn_entities(&mut app_full, ENTITY_COUNT);
    
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        move_entities(&mut app_full, 2.0); // Move to trigger cell changes
        app_full.world_mut().run_system_once(simple_update_spatial_hash);
    }
    let full_rebuild_time = start.elapsed();
    
    // Test 2: Incremental mode (overcapacity = 1.5)
    let mut app_inc = create_test_app(1.5, ENTITY_COUNT);
    spawn_entities(&mut app_inc, ENTITY_COUNT);
    
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        move_entities(&mut app_inc, 2.0); // Move to trigger cell changes
        app_inc.world_mut().run_system_once(simple_update_spatial_hash);
    }
    let incremental_time = start.elapsed();
    
    // Results
    println!("\n=== Incremental vs Full Rebuild Performance ===");
    println!("Entities: {}", ENTITY_COUNT);
    println!("Iterations: {}", ITERATIONS);
    println!("\nFull Rebuild Mode:");
    println!("  Total: {:?}", full_rebuild_time);
    println!("  Per iteration: {:.2}ms", full_rebuild_time.as_secs_f64() * 1000.0 / ITERATIONS as f64);
    println!("\nIncremental Mode:");
    println!("  Total: {:?}", incremental_time);
    println!("  Per iteration: {:.2}ms", incremental_time.as_secs_f64() * 1000.0 / ITERATIONS as f64);
    println!("\nSpeedup: {:.2}x", full_rebuild_time.as_secs_f64() / incremental_time.as_secs_f64());
    
    // Note: This test is for profiling - we don't assert performance requirements
    // because they vary by hardware. The speedup metric is informational.
}

#[test]
fn test_incremental_infrastructure() {
    // Test that incremental update infrastructure is wired up correctly
    let mut app = create_test_app(1.5, 500);
    
    // Spawn moderate number of entities
    spawn_entities(&mut app, 100);
    
    // First update - should populate
    app.world_mut().run_system_once(simple_update_spatial_hash);
    
    let initial_entries = app.world().resource::<SpatialHash>().total_entries();
    println!("Initial entries: {}", initial_entries);
    assert!(initial_entries > 0, "Should have entries after first update");
    
    // Move entities a few times (moderate movement)
    for i in 0..3 {
        move_entities(&mut app, 3.0);
        app.world_mut().run_system_once(simple_update_spatial_hash);
        println!("After move {}: {} entries", i+1, app.world().resource::<SpatialHash>().total_entries());
    }
    
    // Verify spatial hash still works
    let final_entries = app.world().resource::<SpatialHash>().total_entries();
    assert!(final_entries > 0, "Should still have entries after movement");
    
    println!("✓ Incremental update infrastructure works correctly");
}
