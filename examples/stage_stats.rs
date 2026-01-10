/// Quick test to show stage-by-stage optimization statistics
use bevy::prelude::*;
use peregrine::game::simulation::{components::*, systems, physics, collision, SimConfig};
use peregrine::game::spatial_hash::SpatialHash;
use peregrine::game::fixed_math::{FixedVec2, FixedNum};
use peregrine::game::simulation::collision::CollisionEvent;
use std::time::Duration;

fn main() {
    let mut app = App::new();
    
    app.add_plugins((
        MinimalPlugins,
        bevy::log::LogPlugin::default(),
    ));
    
    // Set up fixed timestep
    app.insert_resource(Time::<Fixed>::from_duration(Duration::from_secs_f32(1.0 / 30.0)));
    
    // Initialize spatial hash for 10K units
    let map_size = 320.0;
    app.insert_resource(SpatialHash::new(
        FixedNum::from_num(map_size),
        FixedNum::from_num(map_size),
        FixedNum::from_num(1.0), // Cell size
    ));
    
    app.insert_resource(SimConfig {
        tick_rate: 30.0,
        ..Default::default()
    });
    
    app.add_message::<CollisionEvent>();
    
    // Add systems
    app.add_systems(Update, (
        systems::update_spatial_hash,
        collision::detect_collisions,
        collision::resolve_collisions,
        physics::apply_velocity,
    ).chain());
    
    // Spawn 10K units
    let half_size = map_size / 2.0;
    let mut rng = fastrand::Rng::with_seed(42);
    
    for _ in 0..10_000 {
        let x = rng.f32() * map_size - half_size;
        let y = rng.f32() * map_size - half_size;
        let vx = (rng.f32() - 0.5) * 2.0;
        let vy = (rng.f32() - 0.5) * 2.0;
        
        let pos = FixedVec2::new(FixedNum::from_num(x), FixedNum::from_num(y));
        
        app.world_mut().spawn((
            SimPosition(pos),
            SimPositionPrev(pos),
            SimVelocity(FixedVec2::new(
                FixedNum::from_num(vx),
                FixedNum::from_num(vy),
            )),
            SimAcceleration(FixedVec2::ZERO),
            Collider::default(),
            CachedNeighbors::default(),
            OccupiedCells::default(),
        ));
    }
    
    // Run 30 ticks to see stage statistics
    println!("Running 30 ticks...\n");
    for i in 0..30 {
        println!("=== Tick {} ===", i + 1);
        app.update();
    }
}
