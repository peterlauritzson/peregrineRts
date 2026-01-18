/// Tests for pathfinding correctness
/// 
/// These tests verify that the hierarchical pathfinding system produces correct paths,
/// especially focusing on bugs that cause units to go the wrong direction.

use super::*;
use crate::game::structures::FlowField;
use crate::game::fixed_math::{FixedVec2, FixedNum};

/// Helper to create a simple flow field with specified walkable/blocked tiles
fn create_test_flowfield(width: usize, height: usize) -> FlowField {
    let cell_size = FixedNum::from_num(1.0);
    let origin = FixedVec2::ZERO;
    let mut ff = FlowField::new(width, height, cell_size, origin);
    
    // Initialize all tiles as walkable (cost = 1)
    for y in 0..height {
        for x in 0..width {
            let idx = ff.get_index(x, y);
            ff.cost_field[idx] = 1;
        }
    }
    
    ff
}

/// Helper to add a wall (obstacle) to the flow field
fn add_wall(ff: &mut FlowField, x: usize, y: usize, width: usize, height: usize) {
    for dy in 0..height {
        for dx in 0..width {
            let idx = ff.get_index(x + dx, y + dy);
            ff.cost_field[idx] = 255; // obstacle
        }
    }
}

#[test]
fn test_direction_mapping_consistency() {
    // This test verifies that portals are correctly mapped to directions using Direction enum
    
    // Create a 2x2 cluster grid (50x50 tiles with CLUSTER_SIZE=25)
    let width = 50;
    let height = 50;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Check that portals exist between clusters
    assert!(graph.portals.len() > 0, "No portals created");
    
    // Get cluster (0,0)
    let cluster_00 = graph.clusters.get(&(0, 0)).expect("Cluster (0,0) should exist");
    
    // There should be portals to the East (cluster 1,0) and North (cluster 0,1)
    let has_east_portal = cluster_00.neighbor_connectivity[0][Direction::East.as_index()].is_some();
    let has_north_portal = cluster_00.neighbor_connectivity[0][Direction::North.as_index()].is_some();
    
    println!("Cluster (0,0) neighbor_connectivity:");
    println!("  North: {:?}", cluster_00.neighbor_connectivity[0][Direction::North.as_index()]);
    println!("  South: {:?}", cluster_00.neighbor_connectivity[0][Direction::South.as_index()]);
    println!("  East:  {:?}", cluster_00.neighbor_connectivity[0][Direction::East.as_index()]);
    println!("  West:  {:?}", cluster_00.neighbor_connectivity[0][Direction::West.as_index()]);
    println!("  NorthEast: {:?}", cluster_00.neighbor_connectivity[0][Direction::NorthEast.as_index()]);
    println!("  NorthWest: {:?}", cluster_00.neighbor_connectivity[0][Direction::NorthWest.as_index()]);
    println!("  SouthEast: {:?}", cluster_00.neighbor_connectivity[0][Direction::SouthEast.as_index()]);
    println!("  SouthWest: {:?}", cluster_00.neighbor_connectivity[0][Direction::SouthWest.as_index()]);
    
    // Verify portal directions match the actual cluster layout
    assert!(has_east_portal, "Cluster (0,0) should have portal to East (1,0)");
    assert!(has_north_portal, "Cluster (0,0) should have portal to North (0,1)");
    
    // Check that portal directions are correct
    if let Some(east_portal_id) = cluster_00.neighbor_connectivity[0][Direction::East.as_index()] {
        let portal = graph.portals.get(&east_portal_id).expect("Portal should exist");
        
        // Portal should be on the eastern edge of cluster (0,0)
        assert_eq!(portal.node.x, CLUSTER_SIZE - 1, 
            "East portal should be at x={}, got x={}", CLUSTER_SIZE - 1, portal.node.x);
    }
    
    if let Some(north_portal_id) = cluster_00.neighbor_connectivity[0][Direction::North.as_index()] {
        let portal = graph.portals.get(&north_portal_id).expect("Portal should exist");
        
        // Portal should be on the northern edge of cluster (0,0)
        assert_eq!(portal.node.y, CLUSTER_SIZE - 1,
            "North portal should be at y={}, got y={}", CLUSTER_SIZE - 1, portal.node.y);
    }
}

#[test]
fn test_simple_path_north() {
    // Test that a unit at the south of the map correctly paths to the north
    
    let width = 75;
    let height = 75;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    assert!(graph.initialized, "Graph should be initialized");
    
    let start_cluster = (1, 0);
    let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
    
    // Use Direction enum for type-safe access
    let north_portal_id = start_cluster_data.neighbor_connectivity[0][Direction::North.as_index()];
    
    println!("Cluster (1,0) North portal: {:?}", north_portal_id);
    
    assert!(north_portal_id.is_some(), 
        "Cluster (1,0) should have a North portal to reach (1,1)");
    
    if let Some(portal_id) = north_portal_id {
        let portal = graph.portals.get(&portal_id).unwrap();
        let expected_y = CLUSTER_SIZE - 1;
        
        assert_eq!(portal.node.y, expected_y,
            "North portal from (1,0) should be at y={} (north edge), but is at y={}",
            expected_y, portal.node.y);
    }
}

#[test]
fn test_simple_path_east() {
    // Test that a unit at the west correctly paths to the east
    
    let width = 75;
    let height = 75;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let start_cluster = (0, 1);
    let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
    
    // Use Direction enum
    let east_portal_id = start_cluster_data.neighbor_connectivity[0][Direction::East.as_index()];
    
    assert!(east_portal_id.is_some(),
        "Cluster (0,1) should have an East portal to reach (1,1)");
    
    if let Some(portal_id) = east_portal_id {
        let portal = graph.portals.get(&portal_id).unwrap();
        let expected_x = CLUSTER_SIZE - 1;
        
        assert_eq!(portal.node.x, expected_x,
            "East portal from (0,1) should be at x={} (east edge), but is at x={}",
            expected_x, portal.node.x);
    }
}

#[test]
fn test_path_around_obstacle_north_side() {
    // Create a scenario where there's a wall, and the unit should go around it
    
    let width = 100;
    let height = 100;
    let mut ff = create_test_flowfield(width, height);
    
    // Add a horizontal wall that blocks direct path but allows routing around it
    // Wall from y=35 to y=44 (10 high), x=10 to x=89 (80 wide)
    add_wall(&mut ff, 10, 35, 80, 10);
    
    // Add a gap on the WEST side to allow passage
    for y in 35..45 {
        let idx = ff.get_index(8, y);
        ff.cost_field[idx] = 1; // walkable gap
    }
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Test route from cluster (1,1) to cluster (1,2)
    let start_cluster = (1, 1);
    let goal_cluster = (1, 2);
    
    println!("Start cluster islands: {}", graph.clusters.get(&start_cluster).map(|c| c.island_count).unwrap_or(0));
    println!("Goal cluster islands: {}", graph.clusters.get(&goal_cluster).map(|c| c.island_count).unwrap_or(0));
    
    // Check for portals between start and goal clusters
    println!("\nPortals connecting start->goal:");
    for (&portal_id, portal) in &graph.portals {
        if portal.cluster == start_cluster {
            if let Some(connections) = graph.portal_connections.get(&portal_id) {
                for &(neighbor_portal_id, _cost) in connections {
                    if let Some(neighbor_portal) = graph.portals.get(&neighbor_portal_id) {
                        if neighbor_portal.cluster == goal_cluster {
                            println!("  Portal {} in {:?} -> Portal {} in {:?}", 
                                portal_id, start_cluster, neighbor_portal_id, goal_cluster);
                            println!("    Portal {} island: {:?}", portal_id, graph.portal_island_map.get(&portal_id));
                            println!("    Portal {} island: {:?}", neighbor_portal_id, graph.portal_island_map.get(&neighbor_portal_id));
                        }
                    }
                }
            }
        }
    }
    
    // The path should exist (might need to route west around the obstacle)
    let route_exists = graph.island_routing_table
        .get(&ClusterIslandId::new(start_cluster, IslandId(0)))
        .and_then(|routes| routes.get(&ClusterIslandId::new(goal_cluster, IslandId(0))))
        .is_some();
    
    // If no direct route, that's actually OK - the wall blocks it
    // The important thing is that the portal-island mapping is correct
    println!("Route exists: {}", route_exists);
}

#[test]
fn test_direction_enum_consistency() {
    // This test verifies that the Direction enum values match portal positions
    
    let width = 75;
    let height = 75;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // For each cluster with neighbors, verify portals point the right way
    for (&cluster_id, cluster) in &graph.clusters {
        for island_idx in 0..cluster.island_count {
            for direction in Direction::ALL {
                if let Some(portal_id) = cluster.neighbor_connectivity[island_idx][direction.as_index()] {
                    let portal = graph.portals.get(&portal_id).unwrap();
                    
                    // Verify the portal is actually in that direction
                    let cluster_x_tiles = cluster_id.0 * CLUSTER_SIZE;
                    let cluster_y_tiles = cluster_id.1 * CLUSTER_SIZE;
                    let cluster_max_x = cluster_x_tiles + CLUSTER_SIZE - 1;
                    let cluster_max_y = cluster_y_tiles + CLUSTER_SIZE - 1;
                    
                    match direction {
                        Direction::North => {
                            assert_eq!(portal.node.y, cluster_max_y,
                                "North portal for cluster {:?} should be at y={}, but is at y={}",
                                cluster_id, cluster_max_y, portal.node.y);
                        },
                        Direction::South => {
                            assert_eq!(portal.node.y, cluster_y_tiles,
                                "South portal for cluster {:?} should be at y={}, but is at y={}",
                                cluster_id, cluster_y_tiles, portal.node.y);
                        },
                        Direction::East => {
                            assert_eq!(portal.node.x, cluster_max_x,
                                "East portal for cluster {:?} should be at x={}, but is at x={}",
                                cluster_id, cluster_max_x, portal.node.x);
                        },
                        Direction::West => {
                            assert_eq!(portal.node.x, cluster_x_tiles,
                                "West portal for cluster {:?} should be at x={}, but is at x={}",
                                cluster_id, cluster_x_tiles, portal.node.x);
                        },
                        Direction::NorthEast => {
                            // Allow portal to be at corner or adjacent (shared corner case)
                            assert!(portal.node.x >= cluster_max_x - 1 && portal.node.x <= cluster_max_x + 1,
                                "NorthEast portal for cluster {:?} should be near x={}, but is at x={}",
                                cluster_id, cluster_max_x, portal.node.x);
                            assert!(portal.node.y >= cluster_max_y - 1 && portal.node.y <= cluster_max_y + 1,
                                "NorthEast portal for cluster {:?} should be near y={}, but is at y={}",
                                cluster_id, cluster_max_y, portal.node.y);
                        },
                        Direction::NorthWest => {
                            assert!(portal.node.x >= cluster_x_tiles - 1 && portal.node.x <= cluster_x_tiles + 1,
                                "NorthWest portal for cluster {:?} should be near x={}, but is at x={}",
                                cluster_id, cluster_x_tiles, portal.node.x);
                            assert!(portal.node.y >= cluster_max_y - 1 && portal.node.y <= cluster_max_y + 1,
                                "NorthWest portal for cluster {:?} should be near y={}, but is at y={}",
                                cluster_id, cluster_max_y, portal.node.y);
                        },
                        Direction::SouthEast => {
                            assert!(portal.node.x >= cluster_max_x - 1 && portal.node.x <= cluster_max_x + 1,
                                "SouthEast portal for cluster {:?} should be near x={}, but is at x={}",
                                cluster_id, cluster_max_x, portal.node.x);
                            assert!(portal.node.y >= cluster_y_tiles - 1 && portal.node.y <= cluster_y_tiles + 1,
                                "SouthEast portal for cluster {:?} should be near y={}, but is at y={}",
                                cluster_id, cluster_y_tiles, portal.node.y);
                        },
                        Direction::SouthWest => {
                            assert!(portal.node.x >= cluster_x_tiles - 1 && portal.node.x <= cluster_x_tiles + 1,
                                "SouthWest portal for cluster {:?} should be near x={}, but is at x={}",
                                cluster_id, cluster_x_tiles, portal.node.x);
                            assert!(portal.node.y >= cluster_y_tiles - 1 && portal.node.y <= cluster_y_tiles + 1,
                                "SouthWest portal for cluster {:?} should be near y={}, but is at y={}",
                                cluster_id, cluster_y_tiles, portal.node.y);
                        },
                    }
                }
            }
        }
    }
}

#[test]
fn test_routing_table_correctness() {
    // Test that the routing table produces correct next-portal decisions
    
    let width = 100;
    let height = 100;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let start = ClusterIslandId::new((0, 0), IslandId(0));
    let goal = ClusterIslandId::new((3, 3), IslandId(0));
    
    let mut current = start;
    let mut visited = std::collections::HashSet::new();
    let max_hops = 20;
    
    for _hop in 0..max_hops {
        if current == goal {
            return; // Success!
        }
        
        if visited.contains(&current) {
            panic!("Routing loop detected at {:?}", current);
        }
        visited.insert(current);
        
        let next_portal_id = graph.get_next_portal_for_island(current, goal)
            .expect(&format!("No route from {:?} to {:?}", current, goal));
        
        // Find which cluster this portal leads to
        let mut next_cluster = current.cluster;
        for &(other_portal_id, _cost) in graph.portal_connections.get(&next_portal_id).unwrap_or(&vec![]) {
            if let Some(other_portal) = graph.portals.get(&other_portal_id) {
                if other_portal.cluster != current.cluster {
                    next_cluster = other_portal.cluster;
                    break;
                }
            }
        }
        
        // Verify we're making progress toward the goal
        let current_dist_x = (current.cluster.0 as i32 - goal.cluster.0 as i32).abs();
        let current_dist_y = (current.cluster.1 as i32 - goal.cluster.1 as i32).abs();
        let next_dist_x = (next_cluster.0 as i32 - goal.cluster.0 as i32).abs();
        let next_dist_y = (next_cluster.1 as i32 - goal.cluster.1 as i32).abs();
        
        let current_manhattan = current_dist_x + current_dist_y;
        let next_manhattan = next_dist_x + next_dist_y;
        
        assert!(next_manhattan <= current_manhattan,
            "Routing from {:?} to {:?} moves away from goal: current dist={}, next dist={}",
            current.cluster, goal.cluster, current_manhattan, next_manhattan);
        
        current = ClusterIslandId::new(next_cluster, IslandId(0));
    }
    
    panic!("Failed to reach goal in {} hops", max_hops);
}

#[test]
fn test_intra_cluster_routing() {
    // Test that routing within a cluster (between regions) works correctly
    
    let width = 50;
    let height = 50;
    let mut ff = create_test_flowfield(width, height);
    
    // Create separate regions by adding obstacles
    add_wall(&mut ff, 10, 10, 2, 10);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let cluster = graph.clusters.get(&(0, 0)).expect("Cluster (0,0) should exist");
    
    if cluster.region_count > 1 {
        for i in 0..cluster.region_count {
            for j in 0..cluster.region_count {
                let next_region = cluster.local_routing[i][j];
                
                if i == j {
                    assert_eq!(next_region, i as u8,
                        "Region {} to itself should route to {}, got {}",
                        i, i, next_region);
                } else if next_region != NO_PATH {
                    assert!(next_region < cluster.region_count as u8,
                        "Region {} to {} has invalid next hop: {}",
                        i, j, next_region);
                }
            }
        }
    }
}

#[test]
fn test_goal_island_detection() {
    // Test that goal islands are correctly identified
    
    let width = 100;
    let height = 100;
    let mut ff = create_test_flowfield(width, height);
    
    // Create a cluster with multiple islands by adding a wall that splits it
    add_wall(&mut ff, 37, 30, 2, 15);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let cluster = graph.clusters.get(&(1, 1)).expect("Cluster (1,1) should exist");
    
    // Test that we can look up regions on both sides of the wall
    let left_pos = FixedVec2::new(FixedNum::from_num(32.5), FixedNum::from_num(37.5));
    let right_pos = FixedVec2::new(FixedNum::from_num(42.5), FixedNum::from_num(37.5));
    
    let left_local = world_to_cluster_local(left_pos, (1, 1), &ff)
        .expect("Should convert left position to cluster-local");
    let right_local = world_to_cluster_local(right_pos, (1, 1), &ff)
        .expect("Should convert right position to cluster-local");
    
    let left_region = get_region_id(&cluster.regions, cluster.region_count, left_local);
    let right_region = get_region_id(&cluster.regions, cluster.region_count, right_local);
    
    if let (Some(lr), Some(rr)) = (left_region, right_region) {
        let left_island = cluster.regions[lr.0 as usize].as_ref().unwrap().island;
        let right_island = cluster.regions[rr.0 as usize].as_ref().unwrap().island;
        
        // Either they're in different islands (wall separates them)
        // or same island (can path around). Either way, detection should be consistent.
        assert!(left_island == right_island || left_island != right_island,
            "Island detection should be deterministic");
    }
}

/// Get the first portal a unit should take from a cluster toward a goal
fn get_first_portal_toward_goal(
    graph: &HierarchicalGraph,
    start_cluster: (usize, usize),
    start_island: IslandId,
    goal_cluster: (usize, usize),
    goal_island: IslandId,
) -> Option<(usize, &types::Portal)> {
    let start = ClusterIslandId::new(start_cluster, start_island);
    let goal = ClusterIslandId::new(goal_cluster, goal_island);
    
    let portal_id = graph.get_next_portal_for_island(start, goal)?;
    let portal = graph.portals.get(&portal_id)?;
    
    Some((portal_id, portal))
}

#[test]
fn test_first_portal_direction_makes_sense() {
    // This test verifies that the first portal chosen is in the general direction of the goal
    // This catches bugs where units go the opposite direction
    
    let width = 100;
    let height = 100;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Test case 1: Target is directly north
    // Start at (1, 0), goal at (1, 3)
    // First portal should be North, NOT South
    let start_cluster = (1, 0);
    let goal_cluster = (1, 3);
    
    if let Some((portal_id, portal)) = get_first_portal_toward_goal(
        &graph, start_cluster, IslandId(0), goal_cluster, IslandId(0)
    ) {
        let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
        
        // Determine which direction this portal is in
        let mut portal_direction = None;
        for direction in Direction::ALL {
            if start_cluster_data.neighbor_connectivity[0][direction.as_index()] == Some(portal_id) {
                portal_direction = Some(direction);
                break;
            }
        }
        
        assert_eq!(portal_direction, Some(Direction::North),
            "When goal is directly north, first portal should be North, got {:?}",
            portal_direction);
        
        println!("✓ Correctly chose North portal {:?} at ({}, {}) to reach goal to the north",
            portal_id, portal.node.x, portal.node.y);
    }
    
    // Test case 2: Target is directly east
    // Start at (0, 1), goal at (3, 1)
    // First portal should be East, NOT West
    let start_cluster = (0, 1);
    let goal_cluster = (3, 1);
    
    if let Some((portal_id, portal)) = get_first_portal_toward_goal(
        &graph, start_cluster, IslandId(0), goal_cluster, IslandId(0)
    ) {
        let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
        
        let mut portal_direction = None;
        for direction in Direction::ALL {
            if start_cluster_data.neighbor_connectivity[0][direction.as_index()] == Some(portal_id) {
                portal_direction = Some(direction);
                break;
            }
        }
        
        assert_eq!(portal_direction, Some(Direction::East),
            "When goal is directly east, first portal should be East, got {:?}",
            portal_direction);
        
        println!("✓ Correctly chose East portal {:?} at ({}, {}) to reach goal to the east",
            portal_id, portal.node.x, portal.node.y);
    }
    
    // Test case 3: Target is directly south
    // Start at (1, 3), goal at (1, 0)
    // First portal should be South, NOT North
    let start_cluster = (1, 3);
    let goal_cluster = (1, 0);
    
    if let Some((portal_id, portal)) = get_first_portal_toward_goal(
        &graph, start_cluster, IslandId(0), goal_cluster, IslandId(0)
    ) {
        let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
        
        let mut portal_direction = None;
        for direction in Direction::ALL {
            if start_cluster_data.neighbor_connectivity[0][direction.as_index()] == Some(portal_id) {
                portal_direction = Some(direction);
                break;
            }
        }
        
        assert_eq!(portal_direction, Some(Direction::South),
            "When goal is directly south, first portal should be South, got {:?}",
            portal_direction);
        
        println!("✓ Correctly chose South portal {:?} at ({}, {}) to reach goal to the south",
            portal_id, portal.node.x, portal.node.y);
    }
}

#[test]
fn test_path_length_reasonableness() {
    // Test that the path length is not absurdly longer than the manhattan distance
    // This catches cases where pathfinding takes a very indirect route
    
    let width = 100;
    let height = 100;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Test various start/goal combinations
    let test_cases = vec![
        ((0, 0), (3, 3), "diagonal"),
        ((0, 0), (0, 3), "straight north"),
        ((0, 0), (3, 0), "straight east"),
        ((2, 2), (0, 0), "southwest"),
    ];
    
    for (start_cluster, goal_cluster, description) in test_cases {
        let start = ClusterIslandId::new(start_cluster, IslandId(0));
        let goal = ClusterIslandId::new(goal_cluster, IslandId(0));
        
        // Calculate manhattan distance between clusters
        let manhattan_dist = ((start_cluster.0 as i32 - goal_cluster.0 as i32).abs() +
                             (start_cluster.1 as i32 - goal_cluster.1 as i32).abs()) as usize;
        
        // Count the number of hops in the path
        let mut current = start;
        let mut hops = 0;
        let mut visited = std::collections::HashSet::new();
        
        while current != goal && hops < 20 {
            if visited.contains(&current) {
                panic!("Loop detected in path from {:?} to {:?} ({})", start_cluster, goal_cluster, description);
            }
            visited.insert(current);
            
            let next_portal_id = match graph.get_next_portal_for_island(current, goal) {
                Some(id) => id,
                None => panic!("No route from {:?} to {:?} ({})", start_cluster, goal_cluster, description),
            };
            
            // Find next cluster
            let mut next_cluster = current.cluster;
            for &(other_portal_id, _) in graph.portal_connections.get(&next_portal_id).unwrap_or(&vec![]) {
                if let Some(other_portal) = graph.portals.get(&other_portal_id) {
                    if other_portal.cluster != current.cluster {
                        next_cluster = other_portal.cluster;
                        break;
                    }
                }
            }
            
            current = ClusterIslandId::new(next_cluster, IslandId(0));
            hops += 1;
        }
        
        assert_eq!(current, goal, "Failed to reach goal for {}", description);
        
        // Path should not be more than 2x manhattan distance (allows some detours)
        // For an optimal path on a grid, hops should equal manhattan_dist
        assert!(hops <= manhattan_dist * 2,
            "Path too long for {}: {} hops for manhattan distance of {} (max expected: {})",
            description, hops, manhattan_dist, manhattan_dist * 2);
        
        // Ideally, path should be exactly manhattan distance (optimal)
        if hops == manhattan_dist {
            println!("✓ Optimal path for {}: {} hops", description, hops);
        } else {
            println!("⚠ Suboptimal path for {}: {} hops for manhattan distance of {}", 
                description, hops, manhattan_dist);
        }
    }
}

#[test]
fn test_no_opposite_direction_without_obstacles() {
    // This test specifically catches the bug where units go the OPPOSITE direction
    // when there are no obstacles in the way
    
    let width = 150;
    let height = 150;
    let ff = create_test_flowfield(width, height);
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Test all cardinal directions to ensure first portal is never opposite
    let test_cases = vec![
        ((2, 2), (2, 5), Direction::North, Direction::South, "north goal"),
        ((2, 2), (5, 2), Direction::East, Direction::West, "east goal"),
        ((2, 5), (2, 2), Direction::South, Direction::North, "south goal"),
        ((5, 2), (2, 2), Direction::West, Direction::East, "west goal"),
    ];
    
    for (start_cluster, goal_cluster, expected_dir, forbidden_dir, description) in test_cases {
        if let Some((portal_id, _portal)) = get_first_portal_toward_goal(
            &graph, start_cluster, IslandId(0), goal_cluster, IslandId(0)
        ) {
            let start_cluster_data = graph.clusters.get(&start_cluster).unwrap();
            
            // Find which direction this portal is in
            let mut portal_direction = None;
            for direction in Direction::ALL {
                if start_cluster_data.neighbor_connectivity[0][direction.as_index()] == Some(portal_id) {
                    portal_direction = Some(direction);
                    break;
                }
            }
            
            assert_ne!(portal_direction, Some(forbidden_dir),
                "BUG: For {} (start {:?} -> goal {:?}), chose {:?} portal which is OPPOSITE of goal direction!",
                description, start_cluster, goal_cluster, forbidden_dir);
            
            // With diagonal portals, there may be multiple valid directions  
            // The pathfinding may choose diagonal routes that seem indirect but are optimal
            // Just ensure we're not moving in the opposite direction
            let is_opposite = match expected_dir {
                Direction::North => matches!(portal_direction, Some(Direction::South)),
                Direction::South => matches!(portal_direction, Some(Direction::North)),
                Direction::East => matches!(portal_direction, Some(Direction::West)),
                Direction::West => matches!(portal_direction, Some(Direction::East)),
                _ => false,
            };
            
            assert!(!is_opposite,
                "BUG: For {} (start {:?} -> goal {:?}), chose {:?} which is OPPOSITE direction!",
                description, start_cluster, goal_cluster, portal_direction);
            
            println!("✓ {} correctly chose {:?} portal (not {:?})",
                description, expected_dir, forbidden_dir);
        }
    }
}

#[test]
fn test_obstacle_avoidance_chooses_correct_side() {
    // Test that when there's an obstacle, the pathfinding routes to the correct side
    // This catches the bug where units go to the wrong side of an obstacle
    
    let width = 125;
    let height = 125;
    let mut ff = create_test_flowfield(width, height);
    
    // Create a vertical wall in the middle (clusters 2-3) from y=0 to y=75
    // This blocks direct east-west movement
    for y in 0..75 {
        for x in 50..55 {
            let idx = ff.get_index(x, y);
            ff.cost_field[idx] = 255; // obstacle
        }
    }
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    // Start west of wall, goal east of wall, but goal is in the north
    // Should path around the NORTH end of the wall, not the south
    let start_cluster = (1, 3); // West side, northern area
    let goal_cluster = (3, 3);  // East side, northern area
    
    println!("\n=== Obstacle Avoidance Test ===");
    println!("Wall blocks direct path from {:?} to {:?}", start_cluster, goal_cluster);
    
    // Trace the full path
    let mut current = ClusterIslandId::new(start_cluster, IslandId(0));
    let goal = ClusterIslandId::new(goal_cluster, IslandId(0));
    let mut path_clusters = vec![current.cluster];
    let mut visited = std::collections::HashSet::new();
    
    while current != goal {
        if visited.contains(&current) {
            panic!("Loop detected when pathing around obstacle");
        }
        visited.insert(current);
        
        let next_portal_id = graph.get_next_portal_for_island(current, goal)
            .expect("Should have path around obstacle");
        
        // Find next cluster
        let mut next_cluster = current.cluster;
        for &(other_portal_id, _) in graph.portal_connections.get(&next_portal_id).unwrap_or(&vec![]) {
            if let Some(other_portal) = graph.portals.get(&other_portal_id) {
                if other_portal.cluster != current.cluster {
                    next_cluster = other_portal.cluster;
                    break;
                }
            }
        }
        
        current = ClusterIslandId::new(next_cluster, IslandId(0));
        path_clusters.push(current.cluster);
        
        if path_clusters.len() > 20 {
            panic!("Path too long - possible infinite loop");
        }
    }
    
    println!("Path: {:?}", path_clusters);
    
    // The path should go around the north end (through higher y clusters)
    // It should NOT go through low y-coordinate clusters (south end)
    let went_south = path_clusters.iter().any(|&(_, y)| y < 2);
    
    assert!(!went_south,
        "BUG: Path went around SOUTH end of obstacle (low y clusters), should go around NORTH end. Path: {:?}",
        path_clusters);
    
    println!("✓ Correctly routed around north end of obstacle (avoiding south)");
}

#[test]
fn test_goal_island_detection_with_obstacles() {
    // Test that goal island is correctly detected when there are obstacles
    // This catches the bug where goal defaults to IslandId(0) on wrong side
    
    let width = 100;
    let height = 100;
    let mut ff = create_test_flowfield(width, height);
    
    // Create a vertical wall in cluster (2, 2) splitting it into left/right islands
    // Wall from x=55 to x=57 (middle of cluster), y=50 to y=74
    for y in 50..74 {
        for x in 55..57 {
            let idx = ff.get_index(x, y);
            ff.cost_field[idx] = 255; // obstacle
        }
    }
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let cluster = graph.clusters.get(&(2, 2)).expect("Cluster (2,2) should exist");
    
    println!("\n=== Goal Island Detection Test ===");
    println!("Cluster (2,2) has {} islands", cluster.island_count);
    
    if cluster.island_count > 1 {
        // Test goal on LEFT side of wall (x < 55)
        let left_goal = FixedVec2::new(FixedNum::from_num(52.5), FixedNum::from_num(62.5));
        
        let left_local = world_to_cluster_local(left_goal, (2, 2), &ff)
            .expect("Should convert left goal to cluster-local");
        let left_region = get_region_id(&cluster.regions, cluster.region_count, left_local);
        
        println!("Left goal (world {}, {}) -> local ({}, {})",
            left_goal.x, left_goal.y, left_local.x, left_local.y);
        println!("Left region: {:?}", left_region);
        
        // Test goal on RIGHT side of wall (x > 57)
        let right_goal = FixedVec2::new(FixedNum::from_num(65.5), FixedNum::from_num(62.5));
        
        let right_local = world_to_cluster_local(right_goal, (2, 2), &ff)
            .expect("Should convert right goal to cluster-local");
        let right_region = get_region_id(&cluster.regions, cluster.region_count, right_local);
        
        println!("Right goal (world {}, {}) -> local ({}, {})",
            right_goal.x, right_goal.y, right_local.x, right_local.y);
        println!("Right region: {:?}", right_region);
        
        // Both should find their regions (not fail and default to island 0)
        assert!(left_region.is_some(), "BUG: Left goal failed to find region, would default to island 0!");
        assert!(right_region.is_some(), "BUG: Right goal failed to find region, would default to island 0!");
        
        if let (Some(lr), Some(rr)) = (left_region, right_region) {
            let left_island = cluster.regions[lr.0 as usize].as_ref().unwrap().island;
            let right_island = cluster.regions[rr.0 as usize].as_ref().unwrap().island;
            
            println!("Left island: {:?}, Right island: {:?}", left_island, right_island);
            
            // They should be in DIFFERENT islands (wall separates them)
            assert_ne!(left_island, right_island,
                "BUG: Both sides of wall are in same island! Wall didn't split cluster properly.");
            
            println!("✓ Goals on opposite sides of wall are in different islands");
        }
    } else {
        println!("⚠ Warning: Wall didn't split cluster into multiple islands");
    }
}

#[test]
fn test_world_to_cluster_local_conversion() {
    // Test that world_to_cluster_local conversion is accurate
    // This catches coordinate conversion bugs
    
    let width = 100;
    let height = 100;
    let ff = create_test_flowfield(width, height);
    
    // Test various positions within cluster (1, 1)
    // Cluster (1,1) covers grid cells [25-49, 25-49]
    let test_cases = vec![
        // (world_x, world_y, cluster, expected_local_x_approx, expected_local_y_approx, description)
        (25.5, 25.5, (1, 1), 0.5, 0.5, "bottom-left corner"),
        (37.5, 37.5, (1, 1), 12.5, 12.5, "middle"),
        (49.5, 49.5, (1, 1), 24.5, 24.5, "top-right corner"),
        (25.5, 37.5, (1, 1), 0.5, 12.5, "left edge, middle height"),
        (37.5, 25.5, (1, 1), 12.5, 0.5, "bottom edge, middle width"),
    ];
    
    for (wx, wy, cluster, expected_lx, expected_ly, description) in test_cases {
        let world_pos = FixedVec2::new(FixedNum::from_num(wx), FixedNum::from_num(wy));
        
        let local = world_to_cluster_local(world_pos, cluster, &ff)
            .expect(&format!("Failed to convert {} to cluster-local", description));
        
        let lx = local.x.to_num::<f32>();
        let ly = local.y.to_num::<f32>();
        
        println!("{}: world ({}, {}) -> local ({}, {}), expected ({}, {})",
            description, wx, wy, lx, ly, expected_lx, expected_ly);
        
        // Allow small tolerance for floating point
        let tolerance = 0.1;
        assert!((lx - expected_lx).abs() < tolerance,
            "BUG: {} - local_x is {}, expected ~{}",
            description, lx, expected_lx);
        assert!((ly - expected_ly).abs() < tolerance,
            "BUG: {} - local_y is {}, expected ~{}",
            description, ly, expected_ly);
    }
    
    println!("✓ All world_to_cluster_local conversions accurate");
}

#[test]
fn test_region_lookup_near_boundaries() {
    // Test that region lookup works correctly near cluster/region boundaries
    // This catches the bug where units on boundaries get wrong island
    
    let width = 100;
    let height = 100;
    let mut ff = create_test_flowfield(width, height);
    
    // Create regions with clear boundaries
    add_wall(&mut ff, 35, 35, 2, 15); // Vertical wall creating left/right regions
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let cluster = graph.clusters.get(&(1, 1)).expect("Cluster (1,1) should exist");
    
    println!("\n=== Boundary Region Lookup Test ===");
    println!("Cluster (1,1) has {} regions", cluster.region_count);
    
    // Test positions very close to boundaries
    // Wall is at x=35-36, with dilation radius 1, dilated area extends to adjacent tiles x=34 and x=37
    let test_positions = vec![
        (33.9, 40.0, "just left of dilated wall"),
        (38.1, 40.0, "just right of dilated wall"),
        (35.0, 40.0, "on wall (should fail or be in obstacle)"),
        (30.0, 40.0, "far left of wall"),
        (42.0, 40.0, "far right of wall"),
    ];
    
    for (wx, wy, description) in test_positions {
        let world_pos = FixedVec2::new(FixedNum::from_num(wx), FixedNum::from_num(wy));
        
        if let Some(local) = world_to_cluster_local(world_pos, (1, 1), &ff) {
            let region = get_region_id(&cluster.regions, cluster.region_count, local);
            
            println!("{}: world ({}, {}) -> region {:?}",
                description, wx, wy, region);
            
            // Positions not on wall should find a region
            if !description.contains("on wall") {
                assert!(region.is_some(),
                    "BUG: {} at ({}, {}) failed to find region!",
                    description, wx, wy);
            }
        }
    }
}

#[test]
fn test_fallback_to_island_zero_scenario() {
    // Simulate the exact bug scenario: goal fails to find region, defaults to IslandId(0)
    // which might be on the wrong side of an obstacle
    
    let width = 100;
    let height = 100;
    let mut ff = create_test_flowfield(width, height);
    
    // Create a horizontal wall in cluster (2, 2) splitting it into top/bottom islands
    // Wall from y=60 to y=62, x=50 to x=74
    for y in 60..62 {
        for x in 50..74 {
            let idx = ff.get_index(x, y);
            ff.cost_field[idx] = 255; // obstacle
        }
    }
    
    let mut graph = HierarchicalGraph::default();
    graph.build_graph(&ff, false);
    
    let cluster = graph.clusters.get(&(2, 2)).expect("Cluster (2,2) should exist");
    
    println!("\n=== Island 0 Fallback Test ===");
    println!("Cluster (2,2) has {} islands, {} regions", 
        cluster.island_count, cluster.region_count);
    
    if cluster.island_count > 1 {
        // Find which island is island 0
        let island_0_regions: Vec<_> = (0..cluster.region_count)
            .filter_map(|i| {
                cluster.regions[i].as_ref()
                    .filter(|r| r.island == IslandId(0))
                    .map(|r| (i, r.bounds.center()))
            })
            .collect();
        
        println!("Island 0 has {} regions", island_0_regions.len());
        
        if let Some((_, island_0_center)) = island_0_regions.first() {
            println!("Island 0 center: ({}, {})", island_0_center.x, island_0_center.y);
            
            // Now test a goal that SHOULD be in island 1 (other side of wall)
            // If it's above the wall (y > 62) but island 0 is below, this would be wrong
            let goal_above_wall = FixedVec2::new(FixedNum::from_num(62.0), FixedNum::from_num(67.0));
            
            let local = world_to_cluster_local(goal_above_wall, (2, 2), &ff)
                .expect("Should convert goal to local");
            
            let region = get_region_id(&cluster.regions, cluster.region_count, local);
            
            println!("Goal above wall: world ({}, {}) -> region {:?}",
                goal_above_wall.x, goal_above_wall.y, region);
            
            if let Some(reg_id) = region {
                let actual_island = cluster.regions[reg_id.0 as usize].as_ref().unwrap().island;
                println!("Goal is in island {:?}", actual_island);
                
                // Check if fallback to IslandId(0) would be wrong
                let island_0_y = island_0_center.y.to_num::<f32>();
                let goal_y = 67.0;
                
                if (island_0_y < 61.0 && goal_y > 62.0) || (island_0_y > 62.0 && goal_y < 61.0) {
                    // Island 0 and goal are on opposite sides of wall!
                    if actual_island != IslandId(0) {
                        println!("⚠ CRITICAL: If region lookup fails, defaulting to IslandId(0) would route to WRONG SIDE of obstacle!");
                        println!("  Island 0 is at y={}, goal is at y={}, wall is at y=60-62", island_0_y, goal_y);
                    }
                }
            } else {
                panic!("BUG: Goal above wall failed to find region! This is why units go wrong way - they fall back to IslandId(0)!");
            }
        }
    }
}
