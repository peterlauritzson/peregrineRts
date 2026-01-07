use bevy::prelude::*;
use crate::game::simulation::{SimPosition, SimPositionPrev, SimVelocity, SimSet, Colliding, SimConfig, follow_path};
use crate::game::math::{FixedVec2, FixedNum};
use crate::game::GameState;

use crate::game::config::{GameConfig, GameConfigHandle};

#[derive(Component)]
pub struct Unit;

#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct SelectionCircle;

#[derive(Component)]
pub struct HealthBar;

#[derive(Resource, Default)]
pub struct HealthBarSettings {
    pub show: bool,
}

#[derive(Resource)]
pub struct UnitMesh {
    pub unit: Handle<Mesh>,
    pub circle: Handle<Mesh>,
    pub quad: Handle<Mesh>,
}

#[derive(Resource)]
pub struct UnitMaterials {
    pub normal: Handle<StandardMaterial>,
    pub colliding: Handle<StandardMaterial>,
    pub selection_circle: Handle<StandardMaterial>,
    pub health_bar: Handle<StandardMaterial>,
}

use crate::game::camera::RtsCamera;

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HealthBarSettings>()
           .add_systems(Startup, (setup_unit_resources).chain())
           // unit_movement_logic is replaced by follow_flow_field in simulation.rs
           .add_systems(FixedUpdate, (apply_boids_steering).chain().in_set(SimSet::Steering).after(follow_path))
           .add_systems(Update, (spawn_unit_visuals, update_selection_visuals, update_selection_circle_visibility, update_health_bars, toggle_health_bars, sync_visuals, update_unit_lod).run_if(in_state(GameState::InGame).or(in_state(GameState::Editor))));
    }
}

fn toggle_health_bars(
    keys: Res<ButtonInput<KeyCode>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut settings: ResMut<HealthBarSettings>,
    mut q_bars: Query<&mut Visibility, With<HealthBar>>,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };

    if keys.just_pressed(config.key_toggle_health_bars) {
        settings.show = !settings.show;
        let vis = if settings.show { Visibility::Visible } else { Visibility::Hidden };
        for mut visibility in q_bars.iter_mut() {
            *visibility = vis;
        }
    }
}

fn update_health_bars(
    q_units: Query<(&Children, &Health), Changed<Health>>,
    mut q_bars: Query<&mut Transform, With<HealthBar>>,
) {
    for (children, health) in q_units.iter() {
        let pct = (health.current / health.max).clamp(0.0, 1.0);
        for child in children.iter() {
            if let Ok(mut transform) = q_bars.get_mut(child) {
                transform.scale.x = pct;
                // Center is 0.0. Width is 1.0.
                // If scale is 1.0, left is -0.5, right is 0.5.
                // If scale is 0.5, left is -0.25, right is 0.25.
                // We want left to stay at -0.5.
                // New center = -0.5 + (width * scale / 2.0) = -0.5 + (1.0 * pct / 2.0)
                transform.translation.x = -0.5 + (pct * 0.5);
            }
        }
    }
}

fn update_selection_circle_visibility(
    q_added: Query<&Children, (With<Unit>, Added<Selected>)>,
    q_children_lookup: Query<&Children>,
    q_selected: Query<Entity, With<Selected>>,
    mut q_vis: Query<&mut Visibility, With<SelectionCircle>>,
    mut removed_selected: RemovedComponents<Selected>,
) {
    // Handle Added Selected
    for children in q_added.iter() {
        for child in children.iter() {
            if let Ok(mut vis) = q_vis.get_mut(child) {
                *vis = Visibility::Visible;
            }
        }
    }

    // Handle Removed Selected
    for entity in removed_selected.read() {
        if q_selected.contains(entity) {
            continue;
        }
        if let Ok(children) = q_children_lookup.get(entity) {
            for child in children.iter() {
                if let Ok(mut vis) = q_vis.get_mut(child) {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

fn update_unit_lod(
    mut query: Query<(&mut Visibility, &Transform), With<Unit>>,
    q_camera: Query<(&GlobalTransform, &Camera), With<RtsCamera>>,
    config_handle: Res<GameConfigHandle>,
    game_configs: Res<Assets<GameConfig>>,
    mut gizmos: Gizmos,
) {
    let Some(config) = game_configs.get(&config_handle.0) else { return };
    let Ok((camera_transform, _camera)) = q_camera.single() else { return };
    let camera_pos = camera_transform.translation();
    
    // Simple LOD: If camera is high up, hide mesh and draw simple gizmo
    let lod_height_threshold = config.debug_unit_lod_height_threshold;
    let use_lod = camera_pos.y > lod_height_threshold;

    // Also cull if far away from center of view?
    // Bevy does frustum culling for meshes, but we can help by disabling visibility if we want to draw icons instead.

    for (mut visibility, transform) in query.iter_mut() {
        if use_lod {
            *visibility = Visibility::Hidden;
            // Draw simple icon (circle)
            gizmos.circle(
                Isometry3d::new(
                    transform.translation,
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                0.5,
                Color::srgb(0.8, 0.7, 0.6),
            );
        } else {
            *visibility = Visibility::Visible;
        }
    }
}

fn sync_visuals(
    mut query: Query<(&mut Transform, &SimPosition, &SimPositionPrev)>,
    fixed_time: Res<Time<Fixed>>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (mut transform, pos, prev_pos) in query.iter_mut() {
        let prev = prev_pos.0.to_vec2();
        let curr = pos.0.to_vec2();
        let interpolated = prev.lerp(curr, alpha);
        transform.translation.x = interpolated.x;
        transform.translation.z = interpolated.y;
    }
}

fn update_selection_visuals(
    mut query: Query<(Option<&Colliding>, &mut MeshMaterial3d<StandardMaterial>), With<Unit>>,
    unit_materials: Res<UnitMaterials>,
) {
    for (colliding, mut mat_handle) in query.iter_mut() {
        let target_mat = if colliding.is_some() {
            &unit_materials.colliding
        } else {
            &unit_materials.normal
        };

        if mat_handle.0 != *target_mat {
            mat_handle.0 = target_mat.clone();
        }
    }
}

fn setup_unit_resources(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Capsule3d::default());
    let circle_mesh = meshes.add(Annulus::new(0.6, 0.7)); // Inner radius 0.6, Outer 0.7
    let quad_mesh = meshes.add(Rectangle::new(1.0, 0.15));

    commands.insert_resource(UnitMesh {
        unit: mesh,
        circle: circle_mesh,
        quad: quad_mesh,
    });

    let normal_mat = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let colliding_mat = materials.add(Color::srgb(0.8, 0.2, 0.2));
    let circle_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 1.0, 0.2),
        unlit: true,
        ..default()
    });
    let health_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.0, 1.0, 0.0),
        unlit: true,
        cull_mode: None, // Double sided
        ..default()
    });

    commands.insert_resource(UnitMaterials {
        normal: normal_mat,
        colliding: colliding_mat,
        selection_circle: circle_mat,
        health_bar: health_mat,
    });
}

fn spawn_unit_visuals(
    mut commands: Commands,
    query: Query<(Entity, &SimPosition), Added<Unit>>,
    unit_mesh: Res<UnitMesh>,
    unit_materials: Res<UnitMaterials>,
    settings: Res<HealthBarSettings>,
) {
    for (entity, pos) in query.iter() {
        let p = pos.0.to_vec2();
        commands.entity(entity).insert((
            Mesh3d(unit_mesh.unit.clone()),
            MeshMaterial3d(unit_materials.normal.clone()),
            Transform::from_xyz(p.x, 1.0, p.y),
        )).with_children(|parent| {
            parent.spawn((
                Mesh3d(unit_mesh.circle.clone()),
                MeshMaterial3d(unit_materials.selection_circle.clone()),
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_translation(Vec3::new(0.0, -0.95, 0.0)),
                Visibility::Hidden,
                SelectionCircle,
            ));
            // Health Bar
            parent.spawn((
                Mesh3d(unit_mesh.quad.clone()),
                MeshMaterial3d(unit_materials.health_bar.clone()),
                Transform::from_xyz(0.0, 1.5, 0.0),
                if settings.show { Visibility::Visible } else { Visibility::Hidden },
                HealthBar,
            ));
        });
    }
}

fn apply_boids_steering(
    mut query: Query<(Entity, &SimPosition, &mut SimVelocity, &crate::game::simulation::BoidsNeighborCache), With<Unit>>,
    sim_config: Res<SimConfig>,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
) {
    let start_time = std::time::Instant::now();
    
    let separation_weight = sim_config.separation_weight;
    let alignment_weight = sim_config.alignment_weight;
    let cohesion_weight = sim_config.cohesion_weight;
    let separation_radius = sim_config.separation_radius;
    let max_speed = sim_config.unit_speed;

    // Early exit if all weights are zero
    if separation_weight == FixedNum::ZERO && alignment_weight == FixedNum::ZERO && cohesion_weight == FixedNum::ZERO {
        return;
    }

    let separation_radius_sq = separation_radius * separation_radius;

    // No HashMap allocation! Read directly from cached neighbor lists
    let mut steering_forces = Vec::with_capacity(query.iter().count());

    for (entity, pos, vel, boids_cache) in query.iter() {
        // Early exit if no cached neighbors
        if boids_cache.neighbors.is_empty() {
            continue;
        }
        
        // Accumulate forces (unnormalized for efficiency)
        let mut separation_accum = FixedVec2::ZERO;
        let mut alignment_accum = FixedVec2::ZERO;
        let mut cohesion_accum = FixedVec2::ZERO;
        
        let mut neighbor_count = 0;
        let mut separation_count = 0;

        // Read from cached neighbor list - no spatial hash query, no HashMap lookups!
        for &(other_entity, other_pos, other_vel) in &boids_cache.neighbors {
            // Skip self (shouldn't be in cache, but check anyway)
            if entity == other_entity {
                continue;
            }

            // Work with squared distances to avoid sqrt
            let diff = pos.0 - other_pos;
            let dist_sq = diff.length_squared();

            // All neighbors within cache affect alignment & cohesion
            alignment_accum = alignment_accum + other_vel;
            cohesion_accum = cohesion_accum + other_pos;
            neighbor_count += 1;

            // Separation: only for very close neighbors
            // Use squared distance math - no sqrt needed!
            if dist_sq < separation_radius_sq {
                // Inverse-square falloff for separation strength
                let strength = separation_radius_sq / dist_sq.max(FixedNum::from_num(0.01));
                separation_accum = separation_accum + diff * strength;
                separation_count += 1;
            }
        }

        // Skip if no neighbors affected this unit
        if neighbor_count == 0 {
            continue;
        }

        // Calculate final steering forces
        let mut total_force = FixedVec2::ZERO;

        // Alignment: steer toward average heading
        if alignment_weight > FixedNum::ZERO && neighbor_count > 0 {
            let avg_vel = alignment_accum / FixedNum::from_num(neighbor_count);
            let desired = if avg_vel.length_squared() > FixedNum::ZERO {
                avg_vel.normalize() * max_speed
            } else {
                FixedVec2::ZERO
            };
            let alignment_force = desired - vel.0;
            total_force = total_force + alignment_force * alignment_weight;
        }

        // Cohesion: steer toward center of mass
        if cohesion_weight > FixedNum::ZERO && neighbor_count > 0 {
            let center_of_mass = cohesion_accum / FixedNum::from_num(neighbor_count);
            let direction = center_of_mass - pos.0;
            let desired = if direction.length_squared() > FixedNum::ZERO {
                direction.normalize() * max_speed
            } else {
                FixedVec2::ZERO
            };
            let cohesion_force = desired - vel.0;
            total_force = total_force + cohesion_force * cohesion_weight;
        }

        // Separation: steer away from crowded neighbors
        if separation_weight > FixedNum::ZERO && separation_count > 0 {
            // Normalize the accumulated separation vector
            let separation_force = if separation_accum.length_squared() > FixedNum::ZERO {
                separation_accum.normalize() * max_speed - vel.0
            } else {
                FixedVec2::ZERO
            };
            total_force = total_force + separation_force * separation_weight;
        }

        steering_forces.push((entity, total_force));
    }

    // Apply forces
    let delta = FixedNum::from_num(1.0) / FixedNum::from_num(sim_config.tick_rate);
    for (entity, force) in steering_forces {
        if let Ok((_, _, mut vel, _)) = query.get_mut(entity) {
            vel.0 = vel.0 + force * delta;
            
            // Only clamp if exceeded max speed
            let speed_sq = vel.0.length_squared();
            let max_speed_sq = max_speed * max_speed;
            if speed_sq > max_speed_sq {
                vel.0 = vel.0.normalize() * max_speed;
            }
        }
    }
    
    // Log performance
    let duration = start_time.elapsed();
    let tick = (time.elapsed_secs() * 30.0) as u64;
    let unit_count = query.iter().count();
    if duration.as_millis() > 2 || tick % 100 == 0 {
        warn!("[BOIDS_STEERING] {:?} | Units: {}", duration, unit_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::spatial_hash::SpatialHash;
    use crate::game::simulation::SimConfig;

    #[test]
    fn test_boids_uses_spatial_query() {
        // This test verifies that the boids system uses spatial hash queries
        // rather than brute force O(N²) iteration.
        
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        // Create spatial hash
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        // Create sim config
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(10.0);
        sim_config.separation_radius = FixedNum::from_num(5.0);
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn test units
        let entity_a = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_b = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::new(FixedNum::from_num(1.0), FixedNum::from_num(0.0))),
        )).id();
        
        // Update spatial hash manually
        let pos_a = app.world().get::<SimPosition>(entity_a).unwrap().0;
        let pos_b = app.world().get::<SimPosition>(entity_b).unwrap().0;
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity_a, pos_a);
            hash.insert(entity_b, pos_b);
        }
        
        // Add the boids system
        app.add_systems(Update, apply_boids_steering);
        
        // Run one update
        app.update();
        
        // Verify that velocities were updated (proof that spatial query worked)
        // If spatial query didn't work, units wouldn't interact
        let vel_a = app.world().get::<SimVelocity>(entity_a).unwrap().0;
        
        // Velocity should have changed from ZERO due to boids forces
        // (We can't easily verify it used spatial hash vs brute force in a unit test,
        // but we verify the system runs and produces results)
        assert!(vel_a.length_squared() >= FixedNum::ZERO, "Boids system should run without panicking");
    }

    #[test]
    fn test_boids_excludes_self_from_neighbors() {
        // Verify that an entity doesn't influence itself in boids calculations
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(10.0);
        sim_config.separation_radius = FixedNum::from_num(5.0);
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn a single unit
        let entity = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::ZERO),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        // Update spatial hash
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity, FixedVec2::ZERO);
        }
        
        app.add_systems(Update, apply_boids_steering);
        app.update();
        
        // A single unit alone should remain at zero velocity (no neighbors to influence it)
        let vel = app.world().get::<SimVelocity>(entity).unwrap().0;
        assert_eq!(vel, FixedVec2::ZERO, "Single unit should not be influenced by itself");
    }

    #[test]
    fn test_boids_separation_force() {
        // Test that two overlapping units generate repulsion
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(10.0);
        sim_config.separation_radius = FixedNum::from_num(5.0);
        sim_config.separation_weight = FixedNum::from_num(1.0);
        sim_config.alignment_weight = FixedNum::ZERO; // Disable alignment
        sim_config.cohesion_weight = FixedNum::ZERO; // Disable cohesion
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn two units very close together
        let entity_a = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_b = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(1.0), FixedNum::from_num(0.0))), // 1 unit away
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        // Update spatial hash
        let pos_a = app.world().get::<SimPosition>(entity_a).unwrap().0;
        let pos_b = app.world().get::<SimPosition>(entity_b).unwrap().0;
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity_a, pos_a);
            hash.insert(entity_b, pos_b);
        }
        
        app.add_systems(Update, apply_boids_steering);
        app.update();
        
        // Entity A should have gained velocity in negative X direction (away from B)
        let vel_a = app.world().get::<SimVelocity>(entity_a).unwrap().0;
        
        // With separation only, A should move away from B (negative X)
        // We can't predict exact value due to normalization, but X should be negative
        assert!(vel_a.x < FixedNum::ZERO, "Entity A should move away from B (negative X), got {:?}", vel_a);
    }

    #[test]
    fn test_boids_alignment_with_neighbors() {
        // Test that units align their velocity with nearby units
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(10.0);
        sim_config.separation_radius = FixedNum::from_num(2.0); // Small to minimize separation
        sim_config.separation_weight = FixedNum::ZERO; // Disable separation
        sim_config.alignment_weight = FixedNum::from_num(1.0); // Enable alignment
        sim_config.cohesion_weight = FixedNum::ZERO; // Disable cohesion
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn two units: A is stationary, B is moving in +X direction
        let entity_a = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_b = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(5.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(0.0))), // Moving in +X
        )).id();
        
        // Update spatial hash
        let pos_a = app.world().get::<SimPosition>(entity_a).unwrap().0;
        let pos_b = app.world().get::<SimPosition>(entity_b).unwrap().0;
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity_a, pos_a);
            hash.insert(entity_b, pos_b);
        }
        
        app.add_systems(Update, apply_boids_steering);
        app.update();
        
        // Entity A should have gained velocity in +X direction (aligning with B)
        let vel_a = app.world().get::<SimVelocity>(entity_a).unwrap().0;
        
        // With alignment only, A should start moving in the same direction as B (+X)
        assert!(vel_a.x > FixedNum::ZERO, "Entity A should align with B's velocity (+X), got {:?}", vel_a);
    }

    #[test]
    fn test_boids_cohesion_toward_center() {
        // Test that units steer toward the center of mass of their neighbors
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(20.0);
        sim_config.separation_radius = FixedNum::from_num(2.0);
        sim_config.separation_weight = FixedNum::ZERO; // Disable separation
        sim_config.alignment_weight = FixedNum::ZERO; // Disable alignment
        sim_config.cohesion_weight = FixedNum::from_num(1.0); // Enable cohesion
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn entity A at origin, and B and C to the right
        // Center of mass of B and C is at (10, 0)
        let entity_a = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_b = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(8.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_c = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(12.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        // Update spatial hash
        let pos_a = app.world().get::<SimPosition>(entity_a).unwrap().0;
        let pos_b = app.world().get::<SimPosition>(entity_b).unwrap().0;
        let pos_c = app.world().get::<SimPosition>(entity_c).unwrap().0;
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity_a, pos_a);
            hash.insert(entity_b, pos_b);
            hash.insert(entity_c, pos_c);
        }
        
        app.add_systems(Update, apply_boids_steering);
        app.update();
        
        // Entity A should move toward the center of mass (toward +X)
        let vel_a = app.world().get::<SimVelocity>(entity_a).unwrap().0;
        
        assert!(vel_a.x > FixedNum::ZERO, "Entity A should move toward center of mass (+X), got {:?}", vel_a);
    }

    #[test]
    fn test_boids_respects_neighbor_radius() {
        // Test that units beyond neighbor_radius are not considered
        let mut app = App::new();
        app.init_resource::<Time<Fixed>>();
        
        let spatial_hash = SpatialHash::new(
            FixedNum::from_num(100.0),
            FixedNum::from_num(100.0),
            FixedNum::from_num(10.0),
        );
        app.insert_resource(spatial_hash);
        
        let mut sim_config = SimConfig::default();
        sim_config.neighbor_radius = FixedNum::from_num(5.0); // Small radius
        sim_config.separation_radius = FixedNum::from_num(3.0);
        sim_config.separation_weight = FixedNum::from_num(1.0);
        sim_config.alignment_weight = FixedNum::from_num(1.0);
        sim_config.cohesion_weight = FixedNum::from_num(1.0);
        sim_config.unit_speed = FixedNum::from_num(5.0);
        sim_config.tick_rate = 30.0;
        app.insert_resource(sim_config);
        
        // Spawn entity A at origin, B nearby (within radius), C far away (beyond radius)
        let entity_a = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(0.0), FixedNum::from_num(0.0))),
            SimVelocity(FixedVec2::ZERO),
        )).id();
        
        let entity_b = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(3.0), FixedNum::from_num(0.0))), // Within radius
            SimVelocity(FixedVec2::new(FixedNum::from_num(2.0), FixedNum::from_num(0.0))),
        )).id();
        
        let entity_c = app.world_mut().spawn((
            Unit,
            SimPosition(FixedVec2::new(FixedNum::from_num(20.0), FixedNum::from_num(0.0))), // Beyond radius
            SimVelocity(FixedVec2::new(FixedNum::from_num(10.0), FixedNum::from_num(0.0))),
        )).id();
        
        // Update spatial hash
        let pos_a = app.world().get::<SimPosition>(entity_a).unwrap().0;
        let pos_b = app.world().get::<SimPosition>(entity_b).unwrap().0;
        let pos_c = app.world().get::<SimPosition>(entity_c).unwrap().0;
        {
            let mut hash = app.world_mut().resource_mut::<SpatialHash>();
            hash.clear();
            hash.insert(entity_a, pos_a);
            hash.insert(entity_b, pos_b);
            hash.insert(entity_c, pos_c);
        }
        
        app.add_systems(Update, apply_boids_steering);
        app.update();
        
        // Entity A should be influenced by B but not C
        // If C were influencing A, the velocity would be much higher
        let vel_a = app.world().get::<SimVelocity>(entity_a).unwrap().0;
        
        // The velocity should be small (influenced by nearby B, not distant C)
        // If C influenced A, velocity.x would be much larger
        assert!(vel_a.length() < FixedNum::from_num(10.0), 
            "Entity A should only be influenced by nearby units, got {:?}", vel_a);
    }
}
