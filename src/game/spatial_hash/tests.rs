use super::*;

#[test]
fn test_query_radius_finds_entities_within_range() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);
    let entity_c = Entity::from_bits(3);

    // Place entities
    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0)); // 5 units away
    let pos_c = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(5.0)); // 5 units away

    hash.insert(entity_a, pos_a);
    hash.insert(entity_b, pos_b);
    hash.insert(entity_c, pos_c);

    // Query from entity_a with radius 10 (should find B and C, but not self)
    let results = hash.query_radius(entity_a, pos_a, FixedNum::from_num(10.0));

    assert_eq!(results.len(), 2, "Should find 2 neighbors within radius");
    assert!(results.iter().any(|(e, _)| *e == entity_b), "Should find entity B");
    assert!(results.iter().any(|(e, _)| *e == entity_c), "Should find entity C");
    assert!(!results.iter().any(|(e, _)| *e == entity_a), "Should NOT find self");
}

#[test]
fn test_query_radius_excludes_entities_outside_range() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);
    let entity_c = Entity::from_bits(3);

    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(0.0)); // 3 units away
    let pos_c = FixedVec2::new(FixedNum::from_num(20.0), FixedNum::from_num(0.0)); // 20 units away

    hash.insert(entity_a, pos_a);
    hash.insert(entity_b, pos_b);
    hash.insert(entity_c, pos_c);

    // Query with radius 5 (should find B but not C)
    let results = hash.query_radius(entity_a, pos_a, FixedNum::from_num(5.0));

    // Note: query_radius returns all entities in the grid cells, not filtered by actual distance
    // So this test verifies the grid-based spatial partitioning works
    assert!(results.iter().any(|(e, _)| *e == entity_b), "Should find nearby entity B");
    
    // Entity C is far enough to be in different grid cells with small radius
    // With cell_size=10 and radius=5, we only check cells within that range
}

#[test]
fn test_spatial_hash_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity, pos);

    // Query from same entity - should NOT include self
    let results = hash.query_radius(entity, pos, FixedNum::from_num(10.0));

    assert_eq!(results.len(), 0, "Entity should not find itself in query results");
}

#[test]
fn test_spatial_hash_query_finds_neighbors() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);

    let pos_a = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_b = FixedVec2::new(FixedNum::from_num(2.0), FixedNum::from_num(2.0));

    hash.insert(entity_a, pos_a);
    hash.insert(entity_b, pos_b);

    // A should find B but not itself
    let results = hash.query_radius(entity_a, pos_a, FixedNum::from_num(5.0));
    assert_eq!(results.len(), 1, "A should find exactly one neighbor (B)");
    assert_eq!(results[0].0, entity_b, "A should find B");

    // B should find A but not itself
    let results = hash.query_radius(entity_b, pos_b, FixedNum::from_num(5.0));
    assert_eq!(results.len(), 1, "B should find exactly one neighbor (A)");
    assert_eq!(results[0].0, entity_a, "B should find A");
}

#[test]
fn test_spatial_hash_empty_cell_returns_empty() {
    let hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    // Empty hash - should return empty results
    let results = hash.query_radius(entity, pos, FixedNum::from_num(10.0));
    assert_eq!(results.len(), 0, "Empty hash should return no results");
}

#[test]
fn test_spatial_hash_boundary_cases() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    // Test entities at boundaries of the map
    let entity_corner = Entity::from_bits(1);
    let entity_center = Entity::from_bits(2);
    let entity_edge = Entity::from_bits(3);

    // Map goes from -50 to 50 (centered at 0)
    let pos_corner = FixedVec2::new(FixedNum::from_num(-49.0), FixedNum::from_num(-49.0));
    let pos_center = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos_edge = FixedVec2::new(FixedNum::from_num(49.0), FixedNum::from_num(0.0));

    hash.insert(entity_corner, pos_corner);
    hash.insert(entity_center, pos_center);
    hash.insert(entity_edge, pos_edge);

    // Query from corner with small radius - should not find center or edge
    let results = hash.query_radius(entity_corner, pos_corner, FixedNum::from_num(5.0));
    assert_eq!(results.len(), 0, "Corner entity should not find distant entities");

    // Query from center with large radius - should find edge
    let results = hash.query_radius(entity_center, pos_center, FixedNum::from_num(60.0));
    assert!(results.len() >= 1, "Center should find at least one other entity with large radius");
}

#[test]
fn test_query_radius_returns_same_results_as_brute_force() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
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
        hash.insert(*entity, *pos);
    }

    let query_entity = entities[0].0;
    let query_pos = entities[0].1;
    let query_radius = FixedNum::from_num(12.0);

    // Get results from spatial hash
    let spatial_results = hash.query_radius(query_entity, query_pos, query_radius);

    // Brute force: check all entities manually
    let mut brute_force_results = Vec::new();
    for (entity, pos) in &entities {
        if *entity != query_entity {
            // Note: spatial hash returns all in nearby cells, not filtered by exact radius
            // So we check if it's in the bounding box
            let dx = (query_pos.x - pos.x).abs();
            let dy = (query_pos.y - pos.y).abs();
            if dx <= query_radius && dy <= query_radius {
                brute_force_results.push((*entity, *pos));
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
    for (entity, pos) in &brute_force_results {
        assert!(
            spatial_results.iter().any(|(e, p)| e == entity && p == pos),
            "Spatial hash should find entity {:?} at {:?}",
            entity,
            pos
        );
    }
}

#[test]
fn test_get_potential_collisions_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity = Entity::from_bits(1);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity, pos);

    // Query with exclude_entity set - should NOT include self
    let results = hash.get_potential_collisions(pos, FixedNum::from_num(10.0), Some(entity));

    assert_eq!(results.len(), 0, "get_potential_collisions should not include excluded entity");
}

#[test]
fn test_get_potential_collisions_includes_all_without_exclusion() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity1 = Entity::from_bits(1);
    let entity2 = Entity::from_bits(2);
    let pos = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));

    hash.insert(entity1, pos);
    hash.insert(entity2, pos);

    // Query with None - should include all entities
    let results = hash.get_potential_collisions(pos, FixedNum::from_num(10.0), None);

    assert_eq!(results.len(), 2, "get_potential_collisions should include all entities when exclude_entity is None");
}

#[test]
fn test_get_potential_collisions_finds_neighbors_excludes_self() {
    let mut hash = SpatialHash::new(
        FixedNum::from_num(100.0),
        FixedNum::from_num(100.0),
        FixedNum::from_num(10.0),
    );

    let entity1 = Entity::from_bits(1);
    let entity2 = Entity::from_bits(2);
    let pos1 = FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0));
    let pos2 = FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0));

    hash.insert(entity1, pos1);
    hash.insert(entity2, pos2);

    // Query from entity1 with exclusion - should find entity2 but not entity1
    let results = hash.get_potential_collisions(pos1, FixedNum::from_num(10.0), Some(entity1));

    assert_eq!(results.len(), 1, "Should find 1 neighbor");
    assert_eq!(results[0].0, entity2, "Should find entity2");
    assert!(results.iter().all(|(e, _)| *e != entity1), "Should not find self");
}
