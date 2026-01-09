use bevy::prelude::*;

// Main Menu Components
#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub enum MenuButtonAction {
    Play,
    PlayRandomMap,
    Editor,
    Settings,
    Quit,
}

// Pause Menu Components
#[derive(Component)]
pub struct PauseMenuRoot;

#[derive(Component)]
pub enum PauseButtonAction {
    Resume,
    MainMenu,
    Quit,
}

// Settings Menu Components
#[derive(Component)]
pub struct SettingsMenuRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindableAction {
    CameraForward,
    CameraBackward,
    CameraLeft,
    CameraRight,
    DebugFlow,
    DebugGraph,
    DebugPath,
    SpawnBlackHole,
    SpawnWindSpot,
    SpawnUnit,
    SpawnBatch,
    Pause,
    ToggleHealthBars,
}

impl BindableAction {
    pub fn to_string(&self) -> String {
        match self {
            BindableAction::CameraForward => "Camera Forward".to_string(),
            BindableAction::CameraBackward => "Camera Backward".to_string(),
            BindableAction::CameraLeft => "Camera Left".to_string(),
            BindableAction::CameraRight => "Camera Right".to_string(),
            BindableAction::DebugFlow => "Debug Flow".to_string(),
            BindableAction::DebugGraph => "Debug Graph".to_string(),
            BindableAction::DebugPath => "Debug Path".to_string(),
            BindableAction::SpawnBlackHole => "Spawn Black Hole".to_string(),
            BindableAction::SpawnWindSpot => "Spawn Wind Spot".to_string(),
            BindableAction::SpawnUnit => "Spawn Unit".to_string(),
            BindableAction::SpawnBatch => "Spawn Batch".to_string(),
            BindableAction::Pause => "Pause".to_string(),
            BindableAction::ToggleHealthBars => "Toggle Health Bars".to_string(),
        }
    }
}

#[derive(Component)]
pub enum SettingsButtonAction {
    Back,
    Rebind(BindableAction),
    ToggleFullscreen,
    Save,
}

#[derive(Component)]
pub struct Rebinding;

// Random Map Dialog Components
#[derive(Component)]
pub struct RandomMapDialogRoot;

#[derive(Component)]
pub enum RandomMapDialogAction {
    Generate,
    Cancel,
    IncrementMapWidth,
    DecrementMapWidth,
    IncrementMapHeight,
    DecrementMapHeight,
    IncrementObstacles,
    DecrementObstacles,
    IncrementObstacleSize,
    DecrementObstacleSize,
}

#[derive(Component, Clone, Copy, PartialEq)]
pub enum RandomMapInputField {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

#[derive(Component, Clone, Copy, PartialEq)]
pub enum RandomMapValueText {
    MapWidth,
    MapHeight,
    NumObstacles,
    ObstacleSize,
}

// Random Map Resources
#[derive(Resource, Default)]
pub struct RandomMapState {
    pub show_dialog: bool,
    pub map_width: String,
    pub map_height: String,
    pub num_obstacles: String,
    pub obstacle_size: String,
}

#[derive(Resource, Default)]
pub struct ActiveRandomMapField {
    pub field: Option<RandomMapInputField>,
    pub first_input: bool,
}
