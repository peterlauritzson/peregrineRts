use bevy::prelude::*;
use peregrine::game::math::{FixedVec2, FixedNum};
use peregrine::game::simulation::{SimulationPlugin, MapFlowField, SimPosition, SimVelocity, Collider};
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::pathfinding::PathfindingPlugin;

#[test]
fn test_collision_unit_unit() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);

    app.update();

    // 1. Setup Map (Empty)
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        map.0.cost_field.fill(1);
    }

    // 2. Spawn two units moving towards each other
    // Unit 1 at (-2, 0) moving right
    // Unit 2 at (2, 0) moving left
    // Radius is typically 0.5. Collision distance is 1.0.
    
    let u1 = app.world_mut().spawn((
        SimPosition(FixedVec2::new(FixedNum::from_num(-2.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::new(FixedNum::from_num(1.0), FixedNum::from_num(0.0))),
        Collider::default(),
    )).id();

    let u2 = app.world_mut().spawn((
        SimPosition(FixedVec2::new(FixedNum::from_num(2.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::new(FixedNum::from_num(-1.0), FixedNum::from_num(0.0))),
        Collider::default(),
    )).id();

    // Run simulation for enough ticks for them to collide
    // Distance 4. Speed 1 each -> closing speed 2. Time to impact 2s.
    // Tick rate 20Hz -> 40 ticks.
    
    for _ in 0..60 {
        app.world_mut().run_schedule(FixedUpdate);
    }

    let p1 = app.world().get::<SimPosition>(u1).unwrap().0;
    let p2 = app.world().get::<SimPosition>(u2).unwrap().0;

    println!("Final Positions: U1 {:?}, U2 {:?}", p1, p2);

    // They should not pass through each other.
    // p1.x should be < p2.x
    assert!(p1.x < p2.x, "Units passed through each other!");
    
    // Distance should be maintained around 2*radius (approx 1.0)
    let dist = (p1 - p2).length();
    println!("Distance: {}", dist);
    
    // Soft collision might allow some overlap, but they shouldn't be on top of each other.
    assert!(dist > FixedNum::from_num(0.5), "Units overlapped too much!");
}

#[test]
fn test_collision_unit_wall() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);

    app.update();

    // 1. Setup Map with Wall at x=5
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        map.0.cost_field.fill(1);
        
        // Wall at x=5, y=0
        // Grid coords. Origin (-25, -25).
        // x=5 world -> 30 grid.
        let wall_grid_x = 30;
        let wall_grid_y = 25; // y=0 world
        
        let idx = map.0.get_index(wall_grid_x, wall_grid_y);
        map.0.cost_field[idx] = 255;
        
        // Verify world pos of wall
        let wall_pos = map.0.grid_to_world(wall_grid_x, wall_grid_y);
        println!("Wall at {:?}", wall_pos);
    }

    // 2. Spawn unit moving into wall
    // Unit at (4, 0) moving right (towards x=5)
    let u1 = app.world_mut().spawn((
        SimPosition(FixedVec2::new(FixedNum::from_num(4.0), FixedNum::from_num(0.0))),
        SimVelocity(FixedVec2::new(FixedNum::from_num(1.0), FixedNum::from_num(0.0))),
        Collider::default(),
    )).id();

    // Run simulation
    for _ in 0..40 {
        app.world_mut().run_schedule(FixedUpdate);
    }

    let p1 = app.world().get::<SimPosition>(u1).unwrap().0;
    println!("Final Position: {:?}", p1);

    // Wall is at x=5.5 (center of cell 30). Radius 0.5. Surface at 5.0.
    // Unit radius 0.5.
    // Unit center should stop around 4.5.
    
    assert!(p1.x < FixedNum::from_num(5.0), "Unit entered wall!");
}

#[test]
fn test_collision_crowding() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);

    app.update();

    // Spawn 10 units at the same spot (or very close)
    let mut units = Vec::new();
    for _ in 0..10 {
        let id = app.world_mut().spawn((
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
            Collider::default(),
        )).id();
        units.push(id);
    }

    // They should push each other apart
    for _ in 0..100 {
        app.world_mut().run_schedule(FixedUpdate);
    }

    let mut max_dist = FixedNum::ZERO;
    for id in &units {
        let pos = app.world().get::<SimPosition>(*id).unwrap().0;
        let dist = pos.length(); // Dist from origin
        if dist > max_dist {
            max_dist = dist;
        }
    }

    println!("Max spread distance: {}", max_dist);
    assert!(max_dist > FixedNum::from_num(1.0), "Units did not spread out!");
}
