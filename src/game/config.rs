use bevy::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;
use serde::Deserialize;

#[derive(Deserialize, Asset, TypePath, Clone, Debug)]
pub struct GameConfig {
    pub tick_rate: f64,
    pub unit_speed: f32,
    pub map_width: f32,
    pub map_height: f32,
    pub unit_radius: f32,
    pub collision_push_strength: f32,
    pub obstacle_push_strength: f32,
    pub arrival_threshold: f32,
}

#[derive(Resource)]
pub struct GameConfigHandle(pub Handle<GameConfig>);

pub struct GameConfigPlugin;

impl Plugin for GameConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RonAssetPlugin::<GameConfig>::new(&["game_config.ron"]))
           .add_systems(Startup, setup_config);
    }
}

fn setup_config(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("game_config.ron");
    commands.insert_resource(GameConfigHandle(handle));
}
