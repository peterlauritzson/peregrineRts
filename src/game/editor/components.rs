use bevy::prelude::*;

/// Resource for pending map generation requests
#[derive(Resource)]
pub struct PendingMapGeneration {
    pub map_width: f32,
    pub map_height: f32,
    pub num_obstacles: usize,
    pub min_radius: f32,
    pub max_radius: f32,
}

/// Resources needed for editor rendering
#[derive(Resource)]
pub struct EditorResources {
    pub obstacle_mesh: Handle<Mesh>,
    pub obstacle_material: Handle<StandardMaterial>,
}

/// Marker component for the editor UI root
#[derive(Component)]
pub struct EditorUiRoot;

/// Button actions available in the editor
#[derive(Component)]
pub enum EditorButtonAction {
    OpenGenerateDialog,
    SaveMap,
    TogglePlaceObstacle,
    ClearMap,
    FinalizeMap,
    
    // Dialog buttons
    DialogGenerate,
    DialogCancel,
    
    // Input adjustment buttons
    IncrementMapWidth,
    DecrementMapWidth,
    IncrementMapHeight,
    DecrementMapHeight,
    IncrementObstacles,
    DecrementObstacles,
    IncrementObstacleSize,
    DecrementObstacleSize,
}

/// Editor state tracking
#[derive(Resource, Default)]
pub struct EditorState {
    pub placing_obstacle: bool,
    pub show_generation_dialog: bool,
    pub is_generating: bool,
    pub is_finalizing: bool,
    pub generation_params: GenerationParams,
    pub current_map_size: Vec2,
    // Input field values (defaults)
    pub input_map_width: String,
    pub input_map_height: String,
    pub input_num_obstacles: String,
    pub input_obstacle_size: String, // Combined min/max for simplicity
}

/// Parameters for map generation
#[derive(Default, Clone, Copy)]
pub struct GenerationParams {
    pub map_width: f32,
    pub map_height: f32,
    pub num_obstacles: usize,
    pub min_radius: f32,
    pub max_radius: f32,
}

/// Marker component for generation dialog
#[derive(Component)]
pub struct GenerationDialogRoot;

/// Marker component for loading overlay
#[derive(Component)]
pub struct LoadingOverlayRoot;

/// Types of input fields in the generation dialog
#[derive(Component, Clone, Copy, PartialEq)]
pub enum InputFieldType {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

/// Tracks which input field is currently active
#[derive(Resource, Default)]
pub struct ActiveInputField {
    pub field: Option<InputFieldType>,
    pub first_input: bool,  // True when field was just selected, false after first keypress
}
