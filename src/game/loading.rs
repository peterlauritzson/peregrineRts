use bevy::prelude::*;
use crate::game::GameState;

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
        app.add_systems(OnEnter(GameState::Loading), setup_loading_screen);
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
