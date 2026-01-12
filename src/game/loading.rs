use bevy::prelude::*;
use crate::game::GameState;
use crate::game::editor::PendingMapGeneration;

pub struct LoadingPlugin;

#[derive(Resource)]
pub struct TargetGameState(pub GameState);

#[derive(Resource, Default)]
pub struct LoadingProgress {
    pub progress: f32,
    pub task: String,
}

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadingProgress>();
        app.add_systems(OnEnter(GameState::Loading), (setup_loading_screen, handle_pending_map_generation).chain());
        app.add_systems(OnExit(GameState::Loading), cleanup_loading_screen);
        app.add_systems(Update, update_loading_screen.run_if(in_state(GameState::Loading)));
        app.add_systems(Update, check_loading_complete.run_if(in_state(GameState::Loading)));
    }
}

#[derive(Component)]
struct LoadingRoot;

#[derive(Component)]
struct ProgressBar;

#[derive(Component)]
struct ProgressText;

fn setup_loading_screen(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
            LoadingRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Loading..."),
                TextFont {
                    font_size: 40.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // Progress Bar Container
            parent.spawn((
                Node {
                    width: Val::Px(400.0),
                    height: Val::Px(30.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::from(Color::WHITE),
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|parent| {
                // Progress Bar Fill
                parent.spawn((
                    Node {
                        width: Val::Percent(0.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.2, 0.8, 0.2)),
                    ProgressBar,
                ));
            });

            // Task Text
            parent.spawn((
                Text::new("Initializing..."),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                ProgressText,
            ));
        });
}

fn cleanup_loading_screen(mut commands: Commands, query: Query<Entity, With<LoadingRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn update_loading_screen(
    progress: Res<LoadingProgress>,
    mut bar_query: Query<&mut Node, With<ProgressBar>>,
    mut text_query: Query<&mut Text, With<ProgressText>>,
) {
    for mut node in bar_query.iter_mut() {
        node.width = Val::Percent(progress.progress * 100.0);
    }
    
    for mut text in text_query.iter_mut() {
        text.0 = format!("{} ({:.0}%)", progress.task, progress.progress * 100.0);
    }
}

fn check_loading_complete(
    progress: Res<LoadingProgress>,
    mut next_state: ResMut<NextState<GameState>>,
    target_state: Option<Res<TargetGameState>>,
) {
    if progress.progress >= 1.0 {
        if let Some(target) = target_state {
            next_state.set(target.0);
        } else {
            // Default to InGame if no target specified
            next_state.set(GameState::InGame);
        }
    }
}

fn handle_pending_map_generation(
    mut commands: Commands,
    pending: Option<Res<PendingMapGeneration>>,
    mut sim_config: ResMut<crate::game::simulation::SimConfig>,
    mut spatial_hash: ResMut<crate::game::spatial_hash::SpatialHash>,
    mut map_flow_field: ResMut<crate::game::simulation::MapFlowField>,
    mut graph: ResMut<crate::game::pathfinding::HierarchicalGraph>,
    mut build_state: ResMut<crate::game::pathfinding::GraphBuildState>,
    mut map_status: ResMut<crate::game::simulation::MapStatus>,
    mut meshes: ResMut<Assets<Mesh>>,
    ground_plane_query: Query<(Entity, &Mesh3d), With<crate::game::GroundPlane>>,
    editor_resources: Option<Res<crate::game::editor::EditorResources>>,
) {
    let Some(pending_gen) = pending else {
        return;
    };
    
    use crate::game::fixed_math::{FixedVec2, FixedNum};
    use crate::game::structures::{FlowField, CELL_SIZE};
    use crate::game::simulation::{StaticObstacle, SimPosition, Collider, layers};
    use crate::game::pathfinding::GraphBuildStep;
    use rand::Rng;
    
    info!("=== GENERATING RANDOM MAP DURING LOADING ===");
    info!("Generating map: {}x{} with {} obstacles...", pending_gen.map_width, pending_gen.map_height, pending_gen.num_obstacles);
    
    let map_width = pending_gen.map_width;
    let map_height = pending_gen.map_height;
    
    // Reset Graph and Build State
    graph.reset();
    build_state.step = GraphBuildStep::Done;
    
    // Update SimConfig with new map dimensions
    sim_config.map_width = FixedNum::from_num(map_width);
    sim_config.map_height = FixedNum::from_num(map_height);
    info!("Updated SimConfig: map size = {}x{}", map_width, map_height);
    
    // Update SpatialHash with new map dimensions
    spatial_hash.resize(
        FixedNum::from_num(map_width),
        FixedNum::from_num(map_height),
        &[0.5, 10.0, 25.0],  // Use default entity radii
        4.0  // Default radius to cell ratio
    );
    info!("Updated SpatialHash for new map size");

    let ff_width = (map_width / CELL_SIZE) as usize;
    let ff_height = (map_height / CELL_SIZE) as usize;
    info!("Creating FlowField: {} x {} cells ({}x{} world units)", 
          ff_width, ff_height, map_width, map_height);

    map_flow_field.0 = FlowField::new(
        ff_width, 
        ff_height, 
        FixedNum::from_num(CELL_SIZE), 
        FixedVec2::new(FixedNum::from_num(-map_width/2.0), FixedNum::from_num(-map_height/2.0))
    );
    info!("FlowField created successfully (total cells: {})", ff_width * ff_height);
    
    // Update ground plane mesh to match new map size
    for (entity, _mesh3d) in ground_plane_query.iter() {
        let new_mesh = meshes.add(Plane3d::default().mesh().size(map_width, map_height));
        commands.entity(entity).insert(Mesh3d(new_mesh));
        info!("Updated ground plane mesh to {}x{}", map_width, map_height);
    }

    // Spawn obstacles if any and if we have editor resources
    if pending_gen.num_obstacles > 0 {
        if let Some(resources) = editor_resources {
            let mut rng = rand::rng();
            let margin = 50.0;
            
            for _ in 0..pending_gen.num_obstacles {
                let x = rng.random_range((-map_width/2.0 + margin)..(map_width/2.0 - margin));
                let z = rng.random_range((-map_height/2.0 + margin)..(map_height/2.0 - margin));
                let radius = rng.random_range(pending_gen.min_radius..pending_gen.max_radius);
                
                commands.spawn((
                    crate::game::GameEntity,
                    Mesh3d(resources.obstacle_mesh.clone()),
                    MeshMaterial3d(resources.obstacle_material.clone()),
                    Transform::from_xyz(x, 1.0, z)
                        .with_scale(Vec3::new(radius, 1.0, radius)),
                    GlobalTransform::default(),
                    SimPosition(FixedVec2::from_f32(x, z)),
                    StaticObstacle,
                    Collider {
                        radius: FixedNum::from_num(radius),
                        layer: layers::OBSTACLE,
                        mask: layers::UNIT | layers::OBSTACLE,
                    },
                ));
            }
            
            info!("Spawned {} random obstacles", pending_gen.num_obstacles);
        } else {
            warn!("EditorResources not available, skipping obstacle generation");
        }
    }
    
    // Mark map as not loaded from file (since we generated it)
    map_status.loaded = false;
    
    // Trigger graph building by setting build state to NotStarted
    // This will cause the incremental graph building system to kick in
    graph.reset();
    build_state.step = GraphBuildStep::NotStarted;
    info!("Graph build triggered - will build incrementally during gameplay");
    
    // Remove the pending generation resource
    commands.remove_resource::<PendingMapGeneration>();
    
    info!("=== RANDOM MAP GENERATION COMPLETE ===");
}
