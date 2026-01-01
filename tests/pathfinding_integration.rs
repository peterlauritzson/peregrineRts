use bevy::prelude::*;
use peregrine::game::math::{FixedVec2, FixedNum};
use peregrine::game::simulation::{SimulationPlugin, MapFlowField, SimPosition, SimVelocity, Collider, StaticObstacle};
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::pathfinding::{PathfindingPlugin, PathRequest, HierarchicalGraph, Path};

#[test]
fn test_pathfinding_around_wall() {
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

    // Initialize (runs Startup systems)
    app.update();

    // 1. Setup Map with Wall
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        // Clear
        map.0.cost_field.fill(1);
        
        // Wall at x=25, y=0..40
        // Map is 50x50.
        let wall_x = 25;
        for y in 0..40 {
            let idx = map.0.get_index(wall_x, y);
            map.0.cost_field[idx] = 255;
        }
    }

    // Despawn existing StaticObstacles from Startup
    let mut obstacles = Vec::new();
    {
        let mut query = app.world_mut().query_filtered::<Entity, With<StaticObstacle>>();
        for entity in query.iter(app.world()) {
            obstacles.push(entity);
        }
    }
    for entity in obstacles {
        app.world_mut().despawn(entity);
    }

    // 2. Reset Graph to force rebuild
    app.world_mut().resource_mut::<HierarchicalGraph>().reset();
    app.update(); // Rebuilds graph in Update schedule

    // 3. Spawn Unit
    // Map is 50x50, Origin at (-25, -25).
    // Start at (-20, 0) -> Grid (5, 25)
    let start_pos = FixedVec2::new(FixedNum::from_num(-20.0), FixedNum::from_num(0.0));
    // Goal at (20, 0) -> Grid (45, 25)
    let goal_pos = FixedVec2::new(FixedNum::from_num(20.0), FixedNum::from_num(0.0));
    
    let unit_entity = app.world_mut().spawn((
        SimPosition(start_pos),
        SimVelocity(FixedVec2::ZERO),
        Collider::default(),
    )).id();

    // 4. Send Path Request
    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        start: start_pos,
        goal: goal_pos,
    });

    println!("Test started. Unit at {:?}, Goal at {:?}", start_pos, goal_pos);

    // 5. Run Simulation
    // We manually run schedules to ensure deterministic execution without relying on real time
    for i in 0..2000 {
        // Event maintenance
        app.world_mut().run_schedule(First);
        
        // Simulation and Pathfinding
        app.world_mut().run_schedule(FixedUpdate);
        
        // General updates
        app.world_mut().run_schedule(Update);
        app.world_mut().run_schedule(PostUpdate);
        app.world_mut().run_schedule(Last);

        if i % 100 == 0 {
            let pos = app.world().get::<SimPosition>(unit_entity).unwrap().0;
            if let Some(path) = app.world().get::<Path>(unit_entity) {
                println!("Step {}: Unit at {:?}, Path Idx: {}/{}, Next WP: {:?}", 
                    i, pos, path.current_index, path.waypoints.len(), 
                    path.waypoints.get(path.current_index));
            } else {
                println!("Step {}: Unit at {:?}, No Path", i, pos);
            }
        }
    }

    let final_pos = app.world().get::<SimPosition>(unit_entity).unwrap().0;
    println!("Final Position: {:?}", final_pos);

    // Check if close to goal
    let dist = (final_pos - goal_pos).length();
    assert!(dist < FixedNum::from_num(5.0), "Unit did not reach goal. Dist: {}, Final Pos: {:?}", dist, final_pos);
}

#[test]
fn test_pathfinding_close_target_line_of_sight() {
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

    // Despawn existing StaticObstacles
    let mut obstacles = Vec::new();
    {
        let mut query = app.world_mut().query_filtered::<Entity, With<StaticObstacle>>();
        for entity in query.iter(app.world()) {
            obstacles.push(entity);
        }
    }
    for entity in obstacles {
        app.world_mut().despawn(entity);
    }

    app.world_mut().resource_mut::<HierarchicalGraph>().reset();
    app.update();

    // Map origin is (-25, -25). Cell size 1.
    // Cluster size 10.
    // Cluster (2, 2) is x=20..30.
    // Cluster (3, 2) is x=30..40.
    
    // Start at Grid (29, 25). World: -25 + 29 + 0.5 = 4.5
    // Goal at Grid (31, 25). World: -25 + 31 + 0.5 = 6.5
    
    let start_pos = FixedVec2::new(FixedNum::from_num(4.5), FixedNum::from_num(0.5));
    let goal_pos = FixedVec2::new(FixedNum::from_num(6.5), FixedNum::from_num(0.5));
    
    let unit_entity = app.world_mut().spawn((
        SimPosition(start_pos),
        SimVelocity(FixedVec2::ZERO),
        Collider::default(),
    )).id();

    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        start: start_pos,
        goal: goal_pos,
    });

    // Run one update to process path request
    app.world_mut().run_schedule(FixedUpdate);
    
    let path = app.world().get::<Path>(unit_entity).expect("Path should be found");
    
    println!("Path found with {} waypoints", path.waypoints.len());
    for wp in &path.waypoints {
        println!("WP: {:?}", wp);
    }

    assert_eq!(path.waypoints.len(), 2, "Should be a direct path with 2 waypoints");
}

#[test]
fn test_pathfinding_close_target_obstacle() {
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

    // 1. Setup Map with Obstacle
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        map.0.cost_field.fill(1);
        
        // Block x=30, y=24..27 (3 cells high)
        // This blocks the direct path and the center of the cluster boundary.
        for y in 24..27 {
            let idx = map.0.get_index(30, y);
            map.0.cost_field[idx] = 255;
        }
    }

    // Despawn existing StaticObstacles
    let mut obstacles = Vec::new();
    {
        let mut query = app.world_mut().query_filtered::<Entity, With<StaticObstacle>>();
        for entity in query.iter(app.world()) {
            obstacles.push(entity);
        }
    }
    for entity in obstacles {
        app.world_mut().despawn(entity);
    }

    app.world_mut().resource_mut::<HierarchicalGraph>().reset();
    app.update();

    // Start at Grid (29, 25).
    // Goal at Grid (31, 25).
    
    let start_pos = FixedVec2::new(FixedNum::from_num(4.5), FixedNum::from_num(0.5));
    let goal_pos = FixedVec2::new(FixedNum::from_num(6.5), FixedNum::from_num(0.5));
    
    let unit_entity = app.world_mut().spawn((
        SimPosition(start_pos),
        SimVelocity(FixedVec2::ZERO),
        Collider::default(),
    )).id();

    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        start: start_pos,
        goal: goal_pos,
    });

    app.world_mut().run_schedule(FixedUpdate);
    
    let path = app.world().get::<Path>(unit_entity).expect("Path should be found");
    
    println!("Path found with {} waypoints", path.waypoints.len());
    for wp in &path.waypoints {
        println!("WP: {:?}", wp);
    }

    // Direct path blocked.
    // Path should be around the obstacle.
    // (29, 25) -> (29, 23) -> (30, 23) -> (31, 23) -> (31, 25) approx 5 steps.
    // Hierarchical might be longer.
    
    // We just want to ensure a path is found and it's reasonable.
    assert!(path.waypoints.len() > 2);
    assert!(path.waypoints.len() < 15); // Should be short.
}
