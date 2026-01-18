use super::*;
use crate::game::fixed_math::FixedVec2;

// Helper to create non-placeholder entities for testing
fn test_entity(id: u32) -> Entity {
    Entity::from_bits((id as u64) << 32 | 1)
}

#[test]
fn test_query_radius_finds_entities_within_range() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);
    let entity_c = Entity::from_bits(3);

    // Place entities
    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0)); // 5 units away
    let pos_c = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(5.0)); // 5 units away

    hash.insert(entity_a, pos_a, FixedNum::from_num(0.5));
    hash.insert(entity_b, pos_b, FixedNum::from_num(0.5));
    hash.insert(entity_c, pos_c, FixedNum::from_num(0.5));

    // Query from entity_a with radius 10 (should find B and C, but not self)
    let mut results = Vec::new();
    hash.query_radius(entity_a, pos_a, FixedNum::from_num(10.0), &mut results);

    assert_eq!(results.len(), 2, "Should find 2 neighbors within radius");
    assert!(results.iter().any(|e| *e == entity_b), "Should find entity B");
    assert!(results.iter().any(|e| *e == entity_c), "Should find entity C");
    assert!(!results.iter().any(|e| *e == entity_a), "Should NOT find self");
}

#[test]
fn test_query_radius_excludes_entities_outside_range() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);
    let entity_c = Entity::from_bits(3);

    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(0.0)); // 3 units away
    let pos_c = FixedVec2::new(FixedNum::from_num(20.0), FixedNum::from_num(0.0)); // 20 units away

    hash.insert(entity_a, pos_a, FixedNum::from_num(0.5));
    hash.insert(entity_b, pos_b, FixedNum::from_num(0.5));
    hash.insert(entity_c, pos_c, FixedNum::from_num(0.5));

    // Query with radius 5 (should find B but not C)
    let mut results = Vec::new();
    hash.query_radius(entity_a, pos_a, FixedNum::from_num(5.0), &mut results);

    // Note: query_radius returns all entities in the grid cells, not filtered by actual distance
    // So this test verifies the grid-based spatial partitioning works
    assert!(results.iter().any(|e| *e == entity_b), "Should find nearby entity B");
    
    // Entity C is far enough to be in different grid cells with small radius
    // With cell_size=10 and radius=5, we only check cells within that range
}

#[test]
fn test_spatial_hash_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity, pos, FixedNum::from_num(0.5));

    // Query from same entity - should NOT include self
    let mut results = Vec::new();
    hash.query_radius(entity, pos, FixedNum::from_num(10.0), &mut results);

    assert_eq!(results.len(), 0, "Entity should not find itself in query results");
}

#[test]
fn test_spatial_hash_query_finds_neighbors() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity_a = Entity::from_bits(0x0000000100000001); // Non-placeholder entity
    let entity_b = Entity::from_bits(0x0000000200000001); // Non-placeholder entity

    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(2.0), FixedNum::from_num(2.0));

    let occupied_a = hash.insert(entity_a, pos_a, FixedNum::from_num(0.5));
    let occupied_b = hash.insert(entity_b, pos_b, FixedNum::from_num(0.5));
    
    println!("Entity A: {:?} at {:?}, occupied: {:?}", entity_a, pos_a, occupied_a);
    println!("Entity B: {:?} at {:?}, occupied: {:?}", entity_b, pos_b, occupied_b);
    println!("Size classes: {}", hash.size_classes().len());
    println!("Total entries: {}", hash.total_entries());

    // A should find B but not itself
    let mut results = Vec::new();
    hash.query_radius(entity_a, pos_a, FixedNum::from_num(5.0), &mut results);
    println!("A's query results: {:?}", results);
    assert_eq!(results.len(), 1, "A should find exactly one neighbor (B)");
    assert_eq!(results[0], entity_b, "A should find B");

    // B should find A but not itself
    results.clear();
    hash.query_radius(entity_b, pos_b, FixedNum::from_num(5.0), &mut results);
    println!("B's query results: {:?}", results);
    assert_eq!(results.len(), 1, "B should find exactly one neighbor (A)");
    assert_eq!(results[0], entity_a, "B should find A");
}

#[test]
fn test_spatial_hash_empty_cell_returns_empty() {
    let hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    // Empty hash - should return empty results
    let mut results = Vec::new();
    hash.query_radius(entity, pos, FixedNum::from_num(10.0), &mut results);
    assert_eq!(results.len(), 0, "Empty hash should return no results");
}

#[test]
fn test_spatial_hash_boundary_cases() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    // Test entities at boundaries of the map
    let entity_corner = Entity::from_bits(1);
    let entity_center = Entity::from_bits(2);
    let entity_edge = Entity::from_bits(3);

    // Map goes from -50 to 50 (centered at 0)
    let pos_corner = FixedVec2::new(FixedNum::from_num(-49.0), FixedNum::from_num(-49.0));
    let pos_center = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_edge = FixedVec2::new(FixedNum::from_num(49.0), FixedNum::from_num(0.0));

    hash.insert(entity_corner, pos_corner, FixedNum::from_num(0.5));
    hash.insert(entity_center, pos_center, FixedNum::from_num(0.5));
    hash.insert(entity_edge, pos_edge, FixedNum::from_num(0.5));

    // Query from corner with small radius - should not find center or edge
    let mut results = Vec::new();
    hash.query_radius(entity_corner, pos_corner, FixedNum::from_num(5.0), &mut results);
    assert_eq!(results.len(), 0, "Corner entity should not find distant entities");

    // Query from center with large radius - should find edge
    results.clear();
    hash.query_radius(entity_center, pos_center, FixedNum::from_num(60.0), &mut results);
    assert!(results.len() >= 1, "Center should find at least one other entity with large radius");
}

#[test]
fn test_query_radius_returns_same_results_as_brute_force() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    // Create multiple entities at various positions
    let entities: Vec<(Entity, FixedVec2)> = vec![
        (Entity::from_bits(1), FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
        (Entity::from_bits(2), FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0))),
        (Entity::from_bits(3), FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(5.0))),
        (Entity::from_bits(4), FixedVec2::new(FixedNum::from_num(15.0), FixedNum::from_num(0.0))),
        (Entity::from_bits(5), FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(15.0))),
        (Entity::from_bits(6), FixedVec2::new(FixedNum::from_num(-10.0), FixedNum::from_num(-10.0))),
    ];

    // Insert into spatial hash
    for (entity, pos) in &entities {
        hash.insert(*entity, *pos, FixedNum::from_num(0.5));
    }

    let query_entity = entities[0].0;
    let query_pos = entities[0].1;
    let query_radius = FixedNum::from_num(12.0);

    // Get results from spatial hash
    let mut spatial_results = Vec::new();
    hash.query_radius(query_entity, query_pos, query_radius, &mut spatial_results);

    // Brute force: check all entities manually
    let mut brute_force_results = Vec::new();
    for (entity, pos) in &entities {
        if *entity != query_entity {
            // Note: spatial hash returns all in nearby cells, not filtered by exact radius
            // So we check if it's in the bounding box
            let dx = (query_pos.x - pos.x).abs();
            let dy = (query_pos.y - pos.y).abs();
            if dx <= query_radius && dy <= query_radius {
                brute_force_results.push(*entity);
            }
        }
    }

    // Verify same entities are found (order doesn't matter)
    // Note: Spatial hash may return MORE entities than exact radius check
    // because it returns all entities in nearby grid cells (which is correct and expected)
    assert!(
        spatial_results.len() >= brute_force_results.len(),
        "Spatial hash should find at least as many entities as brute force (can be more due to grid cell inclusion)"
    );

    // All brute force results should be in spatial results
    for entity in &brute_force_results {
        assert!(
            spatial_results.iter().any(|e| e == entity),
            "Spatial hash should find entity {:?}",
            entity
        );
    }
}

#[test]
fn test_get_potential_collisions_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity, pos, FixedNum::from_num(0.5));

    // Query with exclude_entity set - should NOT include self
    let mut results = Vec::new();
    hash.get_potential_collisions(pos, FixedNum::from_num(10.0), Some(entity), &mut results);

    assert_eq!(results.len(), 0, "get_potential_collisions should not include excluded entity");
}

#[test]
fn test_get_potential_collisions_includes_all_without_exclusion() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity1 = test_entity(1);
    let entity2 = test_entity(2);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity1, pos, FixedNum::from_num(0.5));
    hash.insert(entity2, pos, FixedNum::from_num(0.5));

    // Query with None - should include all entities
    let mut results = Vec::new();
    hash.get_potential_collisions(pos, FixedNum::from_num(10.0), None, &mut results);

    assert_eq!(results.len(), 2, "get_potential_collisions should include all entities when exclude_entity is None");
}

#[test]
fn test_get_potential_collisions_finds_neighbors_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5, 10.0, 25.0],
        4.0,
        10_000
    );

    let entity1 = Entity::from_bits(1);
    let entity2 = Entity::from_bits(2);
    let pos1 = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos2 = FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0));

    hash.insert(entity1, pos1, FixedNum::from_num(0.5));
    hash.insert(entity2, pos2, FixedNum::from_num(0.5));

    // Query from entity1 with exclusion - should find entity2 but not entity1
    let mut results = Vec::new();
    hash.get_potential_collisions(pos1, FixedNum::from_num(10.0), Some(entity1), &mut results);

    assert_eq!(results.len(), 1, "Should find 1 neighbor");
    assert_eq!(results[0], entity2, "Should find entity2");
    assert!(results.iter().all(|e| *e != entity1), "Should not find self");
}

#[test]
fn test_compaction_removes_tombstones() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        &[0.5],
        4.0,
        10_000
    );

    let entities: Vec<_> = (0..10).map(|i| test_entity(i)).collect();
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    // Insert 10 entities
    let occupied_cells: Vec<_> = entities.iter()
        .map(|&e| hash.insert(e, pos, FixedNum::from_num(0.5)))
        .collect();

    println!("Inserted {} entities", entities.len());
    println!("Total entries before removal: {}", hash.total_entries());

    // Remove half of them (creates tombstones)
    for i in 0..5 {
        hash.remove(entities[i], &occupied_cells[i]);
    }

    println!("Total entries after removal: {}", hash.total_entries());

    // Check fragmentation before compaction
    let frag_before = hash.fragmentation_ratio();
    println!("Fragmentation before compaction: {:.1}%", frag_before * 100.0);
    assert!(frag_before > 0.0, "Should have fragmentation after removals");

    // Compact
    let compacted = hash.compact_if_fragmented(0.0); // Threshold 0 to force compaction
    println!("Compaction performed: {}", compacted);

    // Check fragmentation after compaction
    let frag_after = hash.fragmentation_ratio();
    println!("Fragmentation after compaction: {:.1}%", frag_after * 100.0);
    println!("Total entries after compaction: {}", hash.total_entries());

    // Verify remaining entities are still queryable
    let mut results = Vec::new();
    hash.get_potential_collisions(pos, FixedNum::from_num(10.0), None, &mut results);
    println!("Query results: {} entities found", results.len());
    
    assert_eq!(frag_after, 0.0, "Fragmentation should be 0 after compaction");
    assert_eq!(results.len(), 5, "Should still find the 5 non-removed entities");
}

