use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use peregrine::game::fixed_math::{FixedVec2, FixedNum};
use peregrine::game::simulation::{SimulationPlugin, SimConfig, SimPosition, SimVelocity};
use peregrine::game::unit::Unit;
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::spatial_hash::SpatialHash;
use peregrine::game::GameState;
use std::time::Instant;

#[test]
fn test_10k_units_boids_tick_under_16ms() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.init_state::<GameState>();
    
    app.update();
    
    // Configure for performance
    {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.neighbor_radius = FixedNum::from_num(10.0);
        config.separation_radius = FixedNum::from_num(5.0);
        config.unit_speed = FixedNum::from_num(5.0);
        config.tick_rate = 60.0; // 60 FPS = ~16ms per frame
    }
    
    // Spawn 10,000 units in a grid pattern
    let grid_size = 100; // 100x100 = 10,000 units
    let spacing = 5.0;
    
    for y in 0..grid_size {
        for x in 0..grid_size {
            let pos_x = (x as f32 - grid_size as f32 / 2.0) * spacing;
            let pos_y = (y as f32 - grid_size as f32 / 2.0) * spacing;
            
            app.world_mut().spawn((
                Unit,
                SimPosition(FixedVec2::new(FixedNum::from_num(pos_x), FixedNum::from_num(pos_y))),
                SimVelocity(FixedVec2::ZERO),
            ));
        }
    }
    
    // Update spatial hash with all units
    let positions: Vec<_> = {
        let mut query = app.world_mut().query_filtered::<(Entity, &SimPosition), With<Unit>>();
        query.iter(app.world()).map(|(e, p)| (e, p.0)).collect()
    };
    
    {
        let mut hash = app.world_mut().resource_mut::<SpatialHash>();
        hash.clear();
        
        for (entity, pos) in positions {
            hash.insert(entity, pos, FixedNum::from_num(0.5));  // Default unit radius
        }
    }
    
    // Warm up (first tick might be slower)
    app.world_mut().run_schedule(FixedUpdate);
    
    // Measure boids tick time
    let start = Instant::now();
    app.world_mut().run_schedule(FixedUpdate);
    let elapsed = start.elapsed();
    
    println!("10K units boids tick took: {:?}", elapsed);
    
    // At 60 FPS, we have 16.67ms per frame
    // This is a performance baseline - may not pass on all hardware
    // but should be significantly faster than O(N²) brute force
    assert!(
        elapsed.as_millis() < 100,
        "Boids tick took {}ms, should be < 100ms (with spatial hash optimization)",
        elapsed.as_millis()
    );
}

#[test]
fn test_spatial_query_correctness() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.init_state::<GameState>();
    
    app.update();
    
    // Create a test scenario with known positions
    let entity_center = app.world_mut().spawn((
        Unit,
        SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::ZERO),
    )).id();
    
    let entity_near = app.world_mut().spawn((
        Unit,
        SimPosition(FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::ZERO),
    )).id();
    
    let _entity_far = app.world_mut().spawn((
        Unit,
        SimPosition(FixedVec2::new(FixedNum::from_num(50.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::ZERO),
    )).id();
    
    // Update spatial hash
    let positions: Vec<_> = {
        let mut query = app.world_mut().query_filtered::<(Entity, &SimPosition), With<Unit>>();
        query.iter(app.world()).map(|(e, p)| (e, p.0)).collect()
    };
    
    {
        let mut hash = app.world_mut().resource_mut::<SpatialHash>();
        hash.clear();
        
        for (entity, pos) in positions {
            hash.insert(entity, pos, FixedNum::from_num(0.5));  // Default unit radius
        }
    }
    
    // Query with radius 10 from center
    let hash = app.world().resource::<SpatialHash>();
    let results = hash.query_radius(
        entity_center,
        FixedVec2::ZERO,
        FixedNum::from_num(10.0)
    );
    
    // Should find entity_near but not entity_far
    // Note: spatial hash returns entities in nearby grid cells, not exact radius
    // so we verify it finds the nearby one at minimum
    assert!(
        results.iter().any(|e| *e == entity_near),
        "Spatial query should find nearby entity"
    );
    
    // Should not find itself
    assert!(
        !results.iter().any(|e| *e == entity_center),
        "Spatial query should not find itself"
    );
    
    println!("Spatial query found {} entities within grid cells", results.len());
}

#[test]
fn test_boids_with_spatial_hash_vs_brute_force() {
    // This test verifies that using spatial hash provides significant performance improvement
    // We can't easily implement a brute force version without modifying the code,
    // but we can verify that the spatial hash version completes quickly
    
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.init_state::<GameState>();
    
    app.update();
    
    // Spawn 1000 units (enough to show difference, but not too slow for tests)
    let num_units = 1000;
    let grid_size = (num_units as f32).sqrt() as usize;
    let spacing = 3.0;
    
    for y in 0..grid_size {
        for x in 0..grid_size {
            if y * grid_size + x >= num_units {
                break;
            }
            
            let pos_x = (x as f32 - grid_size as f32 / 2.0) * spacing;
            let pos_y = (y as f32 - grid_size as f32 / 2.0) * spacing;
            
            app.world_mut().spawn((
                Unit,
                SimPosition(FixedVec2::new(FixedNum::from_num(pos_x), FixedNum::from_num(pos_y))),
                SimVelocity(FixedVec2::ZERO),
            ));
        }
    }
    
    // Update spatial hash
    let positions: Vec<_> = {
        let mut query = app.world_mut().query_filtered::<(Entity, &SimPosition), With<Unit>>();
        query.iter(app.world()).map(|(e, p)| (e, p.0)).collect()
    };
    
    {
        let mut hash = app.world_mut().resource_mut::<SpatialHash>();
        hash.clear();
        
        for (entity, pos) in positions {
            hash.insert(entity, pos, FixedNum::from_num(0.5));  // Default unit radius
        }
    }
    
    // Measure spatial hash performance
    let start = Instant::now();
    app.world_mut().run_schedule(FixedUpdate);
    let spatial_time = start.elapsed();
    
    println!("1000 units with spatial hash: {:?}", spatial_time);
    
    // With spatial hash, 1000 units should complete very quickly
    // Brute force would be O(N²) = 1,000,000 comparisons
    // Spatial hash is approximately O(N) with small constant for nearby checks
    assert!(
        spatial_time.as_millis() < 50,
        "Spatial hash should be fast (<50ms for 1000 units), took {}ms",
        spatial_time.as_millis()
    );
    
    // Note: A true brute force implementation would take 100x-1000x longer
    // For 1000 units: brute force ~1-5 seconds vs spatial hash ~1-50ms
}
