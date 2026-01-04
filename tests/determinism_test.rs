use bevy::prelude::*;
use peregrine::game::pathfinding::{HierarchicalGraph, PathfindingPlugin, GraphBuildState, GraphBuildStep};
use peregrine::game::config::GameConfigPlugin;
use peregrine::game::simulation::SimulationPlugin;
use peregrine::game::loading::LoadingProgress;

/// Helper to build graph for determinism testing
fn build_graph_for_determinism_test(app: &mut App) {
    // Manually insert LoadingProgress before transitioning to Loading state
    app.world_mut().insert_resource(LoadingProgress {
        progress: 0.0,
        task: "Initializing".to_string(),
    });
    
    // Transition to Loading state to trigger graph building
    let mut next_state = app.world_mut().resource_mut::<NextState<peregrine::game::GameState>>();
    next_state.set(peregrine::game::GameState::Loading);
    
    // Run until graph is built
    for _ in 0..10000 {
        app.update();
        
        let build_state = app.world().resource::<GraphBuildState>();
        if build_state.step == GraphBuildStep::Done {
            break;
        }
    }
}

#[test]
fn test_graph_build_is_deterministic() {
    // Build the graph twice and verify identical output
    let mut app1 = App::new();
    app1.add_plugins(MinimalPlugins);
    app1.add_plugins(bevy::state::app::StatesPlugin);
    app1.add_plugins(bevy::asset::AssetPlugin::default());
    app1.init_asset::<Mesh>();
    app1.init_asset::<StandardMaterial>();
    app1.init_resource::<ButtonInput<KeyCode>>();
    app1.add_plugins(bevy::gizmos::GizmoPlugin);
    app1.init_state::<peregrine::game::GameState>();
    app1.add_plugins(GameConfigPlugin);
    app1.add_plugins(SimulationPlugin);
    app1.add_plugins(PathfindingPlugin);
    
    let mut app2 = App::new();
    app2.add_plugins(MinimalPlugins);
    app2.add_plugins(bevy::state::app::StatesPlugin);
    app2.add_plugins(bevy::asset::AssetPlugin::default());
    app2.init_asset::<Mesh>();
    app2.init_asset::<StandardMaterial>();
    app2.init_resource::<ButtonInput<KeyCode>>();
    app2.add_plugins(bevy::gizmos::GizmoPlugin);
    app2.init_state::<peregrine::game::GameState>();
    app2.add_plugins(GameConfigPlugin);
    app2.add_plugins(SimulationPlugin);
    app2.add_plugins(PathfindingPlugin);
    
    // Wait for config to load
    for _ in 0..100 {
        app1.update();
        app2.update();
    }
    
    // Build graphs
    build_graph_for_determinism_test(&mut app1);
    build_graph_for_determinism_test(&mut app2);
    
    // Extract graphs
    let graph1 = app1.world().resource::<HierarchicalGraph>();
    let graph2 = app2.world().resource::<HierarchicalGraph>();
    
    // Verify they are identical
    assert_eq!(graph1.nodes.len(), graph2.nodes.len(), "Graphs should have same number of nodes");
    assert_eq!(graph1.edges.len(), graph2.edges.len(), "Graphs should have same number of edge entries");
    assert_eq!(graph1.clusters.len(), graph2.clusters.len(), "Graphs should have same number of clusters");
    
    // Verify nodes are identical
    for (i, (node1, node2)) in graph1.nodes.iter().zip(graph2.nodes.iter()).enumerate() {
        assert_eq!(node1.node, node2.node, "Node {} should have same grid position", i);
        assert_eq!(node1.cluster, node2.cluster, "Node {} should belong to same cluster", i);
    }
    
    // Verify edges are identical - this is critical for determinism
    // BTreeMap guarantees iteration order, so we can compare directly
    for ((id1, edges1), (id2, edges2)) in graph1.edges.iter().zip(graph2.edges.iter()) {
        assert_eq!(id1, id2, "Edge keys should be in same order");
        assert_eq!(edges1.len(), edges2.len(), "Edge lists should have same length");
        for ((target1, cost1), (target2, cost2)) in edges1.iter().zip(edges2.iter()) {
            assert_eq!(target1, target2, "Edge targets should match");
            assert_eq!(cost1, cost2, "Edge costs should match");
        }
    }
    
    // Verify clusters are identical - BTreeMap iteration order is deterministic
    for ((cluster_id1, cluster1), (cluster_id2, cluster2)) in graph1.clusters.iter().zip(graph2.clusters.iter()) {
        assert_eq!(cluster_id1, cluster_id2, "Cluster IDs should be in same order");
        assert_eq!(cluster1.portals.len(), cluster2.portals.len(), "Clusters should have same portal count");
        assert_eq!(cluster1.flow_field_cache.len(), cluster2.flow_field_cache.len(), "Clusters should have same cache size");
    }
    
    println!("Graph build is deterministic: {} nodes, {} edge entries, {} clusters",
        graph1.nodes.len(), graph1.edges.len(), graph1.clusters.len());
}

#[test]
fn test_cluster_iteration_order_is_deterministic() {
    // Verify that iterating over cluster BTreeMap produces same order every time
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.init_state::<peregrine::game::GameState>();
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);
    
    // Wait for config to load
    for _ in 0..100 {
        app.update();
    }
    
    build_graph_for_determinism_test(&mut app);
    
    let graph = app.world().resource::<HierarchicalGraph>();
    
    // Collect cluster IDs from first iteration
    let ids_first: Vec<_> = graph.clusters.keys().copied().collect();
    
    // Collect cluster IDs from second iteration
    let ids_second: Vec<_> = graph.clusters.keys().copied().collect();
    
    // They should be identical (same order)
    assert_eq!(ids_first, ids_second, "Cluster iteration order should be deterministic");
    
    // Verify BTreeMap property: keys are sorted
    let mut sorted_ids = ids_first.clone();
    sorted_ids.sort();
    assert_eq!(ids_first, sorted_ids, "BTreeMap keys should be in sorted order");
    
    println!("Cluster iteration is deterministic: {} clusters in consistent order", ids_first.len());
}

#[test]
fn test_edge_iteration_order_is_deterministic() {
    // Verify that iterating over edge BTreeMap produces same order every time
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins(bevy::gizmos::GizmoPlugin);
    app.init_state::<peregrine::game::GameState>();
    app.add_plugins(GameConfigPlugin);
    app.add_plugins(SimulationPlugin);
    app.add_plugins(PathfindingPlugin);
    
    // Wait for config to load
    for _ in 0..100 {
        app.update();
    }
    
    build_graph_for_determinism_test(&mut app);
    
    let graph = app.world().resource::<HierarchicalGraph>();
    
    // Collect edge keys from first iteration
    let keys_first: Vec<_> = graph.edges.keys().copied().collect();
    
    // Collect edge keys from second iteration
    let keys_second: Vec<_> = graph.edges.keys().copied().collect();
    
    // They should be identical (same order)
    assert_eq!(keys_first, keys_second, "Edge iteration order should be deterministic");
    
    // Verify BTreeMap property: keys are sorted
    let mut sorted_keys = keys_first.clone();
    sorted_keys.sort();
    assert_eq!(keys_first, sorted_keys, "BTreeMap keys should be in sorted order");
    
    println!("Edge iteration is deterministic: {} edge entries in consistent order", keys_first.len());
}
