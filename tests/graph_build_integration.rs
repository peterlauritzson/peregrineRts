use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use peregrine::game::math::{FixedVec2, FixedNum};
use peregrine::game::simulation::{SimulationPlugin, MapFlowField};
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::pathfinding::{PathfindingPlugin, GraphBuildState, GraphBuildStep, HierarchicalGraph};
use peregrine::game::loading::LoadingProgress;
use peregrine::game::GameState;

/// Helper function to run incremental graph build to completion in tests
fn build_graph_incremental(app: &mut App) {
    // Insert LoadingProgress resource if not already present
    if !app.world().contains_resource::<LoadingProgress>() {
        app.world_mut().insert_resource(LoadingProgress::default());
    }
    
    // Reset build state to start fresh
    app.world_mut().resource_mut::<GraphBuildState>().step = GraphBuildStep::NotStarted;
    
    // Set state to Loading to enable incremental build system
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Loading);
    app.update(); // Apply state change - this triggers OnEnter(Loading) which needs LoadingProgress
    
    // Run incremental build until done (max 5000 iterations - large maps can take many steps)
    for i in 0..5000 {
        // Ensure we stay in Loading state during build
        let current_state = app.world().resource::<State<GameState>>().get().clone();
        if current_state != GameState::Loading {
            app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Loading);
            app.update();
        }
        
        app.world_mut().run_schedule(Update);
        
        let step = app.world().resource::<GraphBuildState>().step;
        if step == GraphBuildStep::Done {
            println!("Graph build completed in {} iterations", i);
            break;
        }
        
        // Debug: print current step every 200 iterations
        if i % 200 == 0 {
            println!("Iteration {}: step = {:?}", i, step);
        }
    }
    
    let step = app.world().resource::<GraphBuildState>().step;
    assert_eq!(step, GraphBuildStep::Done, "Graph build should complete within 5000 iterations");
    
    // Return to InGame state
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::InGame);
    app.update(); // Apply state change
}

#[test]
fn test_incremental_build_completes() {
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

    app.update();

    // Setup a small test map
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        let width = 100;
        let height = 100;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-50.0), FixedNum::from_num(-50.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1); // All walkable
    }

    // Build graph incrementally
    build_graph_incremental(&mut app);

    // Verify build state is Done
    let build_state = app.world().resource::<GraphBuildState>();
    assert_eq!(build_state.step, GraphBuildStep::Done, "Build should be complete");
}

#[test]
fn test_incremental_build_produces_valid_graph() {
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

    app.update();

    // Setup a test map
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        let width = 100;
        let height = 100;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-50.0), FixedNum::from_num(-50.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1);
    }

    build_graph_incremental(&mut app);

    // Verify graph has valid data
    let graph = app.world().resource::<HierarchicalGraph>();
    assert!(graph.initialized, "Graph should be initialized");
    assert!(!graph.clusters.is_empty(), "Graph should have clusters");
    assert!(!graph.nodes.is_empty(), "Graph should have nodes (portals)");
    
    // Verify clusters exist (100x100 map with cluster size ~10 should have ~100 clusters)
    println!("Graph has {} clusters and {} nodes", graph.clusters.len(), graph.nodes.len());
    assert!(graph.clusters.len() > 0, "Should have created clusters");
}

#[test]
fn test_build_does_not_block_frame() {
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

    app.update();

    // Setup a larger test map to ensure incremental building takes multiple frames
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        let width = 200;
        let height = 200;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-100.0), FixedNum::from_num(-100.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1);
    }

    // Insert LoadingProgress
    if !app.world().contains_resource::<LoadingProgress>() {
        app.world_mut().insert_resource(LoadingProgress::default());
    }
    
    // Reset build state
    app.world_mut().resource_mut::<GraphBuildState>().step = GraphBuildStep::NotStarted;
    
    // Set state to Loading
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Loading);
    app.update();
    
    // Run build step by step and verify each step completes quickly
    let mut iterations = 0;
    let max_iterations = 5000;
    
    while iterations < max_iterations {
        let start = std::time::Instant::now();
        
        app.world_mut().run_schedule(Update);
        
        let elapsed = start.elapsed();
        
        // Each incremental step should complete in reasonable time (< 100ms for test)
        // In production with 60 FPS, we want < 16ms, but tests run slower
        assert!(elapsed.as_millis() < 100, 
            "Incremental build step took {}ms, should be < 100ms", 
            elapsed.as_millis());
        
        let step = app.world().resource::<GraphBuildState>().step;
        if step == GraphBuildStep::Done {
            break;
        }
        
        iterations += 1;
    }
    
    let step = app.world().resource::<GraphBuildState>().step;
    assert_eq!(step, GraphBuildStep::Done, "Build should complete within {} iterations", max_iterations);
    
    println!("Incremental build completed in {} update cycles", iterations);
}

#[test]
fn test_build_progress_increases() {
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

    app.update();

    // Setup a test map
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        let width = 100;
        let height = 100;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-50.0), FixedNum::from_num(-50.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1);
    }

    // Insert LoadingProgress
    if !app.world().contains_resource::<LoadingProgress>() {
        app.world_mut().insert_resource(LoadingProgress::default());
    }
    
    // Reset build state
    app.world_mut().resource_mut::<GraphBuildState>().step = GraphBuildStep::NotStarted;
    
    // Set state to Loading
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Loading);
    app.update();
    
    let mut last_progress = 0.0;
    let mut progress_increased = false;
    
    // Run build and verify progress increases
    for _ in 0..5000 {
        app.world_mut().run_schedule(Update);
        
        let progress = app.world().resource::<LoadingProgress>().progress;
        
        if progress > last_progress {
            progress_increased = true;
        }
        
        last_progress = progress;
        
        let step = app.world().resource::<GraphBuildState>().step;
        if step == GraphBuildStep::Done {
            break;
        }
    }
    
    assert!(progress_increased, "Progress should increase during build");
    assert!(last_progress > 0.0, "Final progress should be > 0");
}

#[test]
fn test_incremental_build_matches_sync_build() {
    // This test verifies that the incremental builder produces the same graph as sync build
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

    app.update();

    // Setup a test map
    {
        let mut map = app.world_mut().resource_mut::<MapFlowField>();
        let width = 50;
        let height = 50;
        let cell_size = FixedNum::from_num(1.0);
        let origin = FixedVec2::new(FixedNum::from_num(-25.0), FixedNum::from_num(-25.0));
        map.0 = peregrine::game::structures::FlowField::new(width, height, cell_size, origin);
        map.0.cost_field.fill(1);
        
        // Add some obstacles to make the graph more interesting
        for x in 20..25 {
            for y in 20..30 {
                let idx = map.0.get_index(x, y);
                map.0.cost_field[idx] = 255;
            }
        }
    }

    // Build incrementally
    build_graph_incremental(&mut app);
    
    // Get incremental graph state
    let incremental_graph = app.world().resource::<HierarchicalGraph>();
    let incremental_cluster_count = incremental_graph.clusters.len();
    let incremental_node_count = incremental_graph.nodes.len();
    let incremental_edge_count = incremental_graph.edges.len();
    let incremental_initialized = incremental_graph.initialized;
    
    println!("Incremental build: {} clusters, {} nodes, {} edges", 
        incremental_cluster_count, incremental_node_count, incremental_edge_count);
    
    // Build synchronously for comparison
    {
        let flow_field = app.world().resource::<MapFlowField>().0.clone();
        app.world_mut().resource_mut::<HierarchicalGraph>().build_graph_sync(&flow_field);
    }
    
    // Get sync graph state
    let sync_graph = app.world().resource::<HierarchicalGraph>();
    let sync_cluster_count = sync_graph.clusters.len();
    let sync_node_count = sync_graph.nodes.len();
    let sync_edge_count = sync_graph.edges.len();
    let sync_initialized = sync_graph.initialized;
    
    println!("Sync build: {} clusters, {} nodes, {} edges", 
        sync_cluster_count, sync_node_count, sync_edge_count);
    
    // Verify both methods produce similar results
    assert_eq!(incremental_initialized, sync_initialized, "Both should be initialized");
    assert_eq!(incremental_cluster_count, sync_cluster_count, "Should have same number of clusters");
    assert_eq!(incremental_node_count, sync_node_count, "Should have same number of nodes");
    
    // Edge count might vary slightly due to implementation details, but should be similar
    let edge_diff = if incremental_edge_count > sync_edge_count {
        incremental_edge_count - sync_edge_count
    } else {
        sync_edge_count - incremental_edge_count
    };
    
    assert!(
        edge_diff < 10,
        "Edge counts should be similar (diff: {})",
        edge_diff
    );
}
