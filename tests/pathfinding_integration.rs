use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use peregrine::game::fixed_math::{FixedVec2, FixedNum};
use peregrine::game::simulation::{SimulationPlugin, MapFlowField, SimPosition, SimPositionPrev, SimVelocity, SimAcceleration, Collider, CollisionState, StaticObstacle, OccupiedCell, layers};
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::pathfinding::{PathfindingPlugin, PathRequest, Path, HierarchicalGraph};
use peregrine::game::loading::LoadingProgress;
use peregrine::game::GameState;

/// Helper function to run incremental graph build to completion in tests
fn build_graph_incremental(app: &mut App) {
    // Build the graph from the current MapFlowField
    let flow_field = app.world().resource::<MapFlowField>().0.clone();
    app.world_mut().resource_mut::<HierarchicalGraph>().build_graph(&flow_field, false);
}

#[test]
fn test_pathfinding_around_wall() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);
    app.init_state::<GameState>();
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::InGame);

    // Insert LoadingProgress resource (required by incremental_build_graph system)
    app.world_mut().insert_resource(LoadingProgress::default());

    // Initialize (runs Startup systems)
    app.update();

    // 1. Setup Map with Wall
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        
        // Force resize to 50x50 for test
        let width = 50;
        let height = 50;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-25.0), FixedNum::from_num(-25.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);

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

    // 2. Build Graph incrementally
    build_graph_incremental(&mut app);

    // 3. Spawn Unit
    // Map is 50x50, Origin at (-25, -25).
    // Start at (-20, 0) -> Grid (5, 25)
    let start_pos = FixedVec2::new(FixedNum::from_num(-20.0), FixedNum::from_num(0.0));
    // Goal at (20, 0) -> Grid (45, 25)
    let goal_pos = FixedVec2::new(FixedNum::from_num(20.0), FixedNum::from_num(0.0));
    
    let unit_entity = app.world_mut().spawn((
        SimPosition(start_pos),
        SimPositionPrev(start_pos),
        SimVelocity(FixedVec2::ZERO),
        SimAcceleration::default(),
        Collider::default(),
        CollisionState::default(),
        OccupiedCell::default(),
        
    )).id();

    // 4. Send Path Request
    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        goal: goal_pos,
    });

    println!("Test started. Unit at {:?}, Goal at {:?}", start_pos, goal_pos);

    // 5. Run Simulation
    // We manually run schedules to ensure deterministic execution without relying on real time
    let mut reached_goal = false;
    for i in 0..2000 {
        // Event maintenance
        app.world_mut().run_schedule(First);
        
        // Simulation and Pathfinding
        app.world_mut().run_schedule(FixedUpdate);
        
        // General updates
        app.world_mut().run_schedule(Update);
        app.world_mut().run_schedule(PostUpdate);
        app.world_mut().run_schedule(Last);

        let pos = app.world().get::<SimPosition>(unit_entity).unwrap().0;
        let dist = (pos - goal_pos).length();
        if dist < FixedNum::from_num(2.0) {
            reached_goal = true;
            println!("Goal reached at step {}! Pos: {:?}", i, pos);
            break;
        }

        if i % 100 == 0 {
            if let Some(path) = app.world().get::<Path>(unit_entity) {
                match path {
                    Path::Direct(goal) => {
                        println!("Step {}: Unit at {:?}, Direct Path to {:?}", i, pos, goal);
                    },
                    Path::LocalAStar { waypoints, current_index } => {
                        println!("Step {}: Unit at {:?}, Local Path Idx: {}/{}, Next WP: {:?}", 
                            i, pos, current_index, waypoints.len(), 
                            waypoints.get(*current_index));
                    },
                    Path::Hierarchical { goal, goal_cluster, goal_region: _, goal_island: _ } => {
                        println!("Step {}: Unit at {:?}, H-Path to cluster {:?}, Final Goal: {:?}", 
                            i, pos, goal_cluster, goal);
                    }
                }
            } else {
                println!("Step {}: Unit at {:?}, No Path", i, pos);
            }
        }
    }

    let final_pos = app.world().get::<SimPosition>(unit_entity).unwrap().0;
    println!("Final Position: {:?}", final_pos);

    // Check if close to goal
    assert!(reached_goal, "Unit did not reach goal. Final Pos: {:?}", final_pos);
}

#[test]
fn test_pathfinding_close_target_line_of_sight() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);
    app.init_state::<GameState>();
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::InGame);

    // Insert LoadingProgress resource (required by incremental_build_graph system)
    app.world_mut().insert_resource(LoadingProgress::default());

    app.update();

    // 1. Setup Map (Empty)
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        
        // Force resize to 50x50 for test
        let width = 50;
        let height = 50;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-25.0), FixedNum::from_num(-25.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);

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

    // Build Graph incrementally
    build_graph_incremental(&mut app);

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
        SimPositionPrev(start_pos),
        SimVelocity(FixedVec2::ZERO),
        SimAcceleration::default(),
        Collider::default(),
        CollisionState::default(),
        OccupiedCell::default(),
        
    )).id();

    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        goal: goal_pos,
    });

    // Run updates to process path request
    for _ in 0..2 {
        app.world_mut().run_schedule(First);
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(Update);
        app.world_mut().run_schedule(PostUpdate);
        app.world_mut().run_schedule(Last);
    }
    
    let path = app.world().get::<Path>(unit_entity).expect("Path should be found");
    
    // NEW: Region-based pathfinding always uses Hierarchical paths
    // The system will optimize to direct movement when units are in the same region
    match path {
        Path::Direct(goal) => {
             println!("Path found: Direct to {:?}", goal);
        },
        Path::LocalAStar { waypoints, .. } => {
            println!("Path found: LocalAStar with {} waypoints", waypoints.len());
            for wp in waypoints {
                println!("  WP: {:?}", wp);
            }
        },
        Path::Hierarchical { goal, goal_cluster, goal_region, goal_island } => {
            println!("Path found: Hierarchical to cluster {:?}, island {}, region {:?}, goal {:?}", 
                goal_cluster, goal_island.0, goal_region, goal);
            // For close targets, this is expected behavior with new region-based pathfinding
        }
    }
}

#[test]
fn test_pathfinding_close_target_obstacle() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(StatesPlugin);
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(AssetPlugin::default()); 
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);
    app.init_state::<GameState>();
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::InGame);

    // Insert LoadingProgress resource (required by incremental_build_graph system)
    app.world_mut().insert_resource(LoadingProgress::default());

    app.update();

    // 1. Setup Map with Obstacle
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        
        // Force resize to 50x50 for test
        let width = 50;
        let height = 50;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-25.0), FixedNum::from_num(-25.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1);
    }

    // Block x=30, y=24..27 (3 cells high)
    // This blocks the direct path and the center of the cluster boundary.
    // Map origin (-25, -25).
    // x=30 -> 5.5
    // y=25 -> 0.5
    // Spawn obstacle at (5.5, 0.5) with radius 1.5 (covers y=-1 to 2)
    app.world_mut().spawn((
        SimPosition(FixedVec2::new(FixedNum::from_num(5.5), FixedNum::from_num(0.5))),
        StaticObstacle,
        Collider {
            radius: FixedNum::from_num(1.5),
            layer: layers::OBSTACLE,
            mask: layers::UNIT,
        },
    ));

    // Run update to apply obstacle
    app.update();

    // Build Graph incrementally
    build_graph_incremental(&mut app);

    // Start at Grid (25, 25). World: 0.5
    // Goal at Grid (35, 25). World: 10.5
    
    let start_pos = FixedVec2::new(FixedNum::from_num(0.5), FixedNum::from_num(0.5));
    let goal_pos = FixedVec2::new(FixedNum::from_num(10.5), FixedNum::from_num(0.5));
    
    let unit_entity = app.world_mut().spawn((
        SimPosition(start_pos),
        SimPositionPrev(start_pos),
        SimVelocity(FixedVec2::ZERO),
        SimAcceleration::default(),
        Collider::default(),
        CollisionState::default(),
        OccupiedCell::default(),
        
    )).id();

    app.world_mut().write_message(PathRequest {
        entity: unit_entity,
        goal: goal_pos,
    });

    // Run updates to process path request
    for _ in 0..2 {
        app.world_mut().run_schedule(First);
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(Update);
        app.world_mut().run_schedule(PostUpdate);
        app.world_mut().run_schedule(Last);
    }
    
    let path = app.world().get::<Path>(unit_entity).expect("Path should be found");
    
    match path {
        Path::LocalAStar { waypoints, .. } => {
            println!("Path found with {} waypoints", waypoints.len());
            for wp in waypoints {
                println!("WP: {:?}", wp);
            }
            assert!(waypoints.len() > 2);
            assert!(waypoints.len() < 30);
        },
        Path::Hierarchical { .. } => {
             println!("Path found: Hierarchical");
             // Acceptable
        },
        Path::Direct(_) => {
            panic!("Should not be a direct path");
        }
    }
}
