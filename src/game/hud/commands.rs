use bevy::prelude::*;
use crate::game::unit::Selected;
use crate::game::simulation::UnitStopCommand;
use crate::game::control::InputMode;
use super::components::*;

/// Handle button visual feedback on interaction
pub fn button_system(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = BackgroundColor(Color::srgb(0.1, 0.5, 0.1));
            }
            Interaction::Hovered => {
                *color = BackgroundColor(Color::srgb(0.4, 0.4, 0.4));
            }
            Interaction::None => {
                *color = BackgroundColor(Color::srgb(0.3, 0.3, 0.3));
            }
        }
    }
}

/// Handle command button clicks
pub fn command_handler(
    interaction_query: Query<
        (&Interaction, &CommandButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut stop_events: MessageWriter<UnitStopCommand>,
    selected_units: Query<Entity, With<Selected>>,
    mut input_mode: ResMut<InputMode>,
) {
    for (interaction, command) in &interaction_query {
        if *interaction == Interaction::Pressed {
            match command.0 {
                CommandAction::Stop => {
                    for entity in &selected_units {
                        stop_events.write(UnitStopCommand {
                            player_id: 0, // Local player
                            entity,
                        });
                    }
                }
                CommandAction::Move => {
                    *input_mode = InputMode::CommandMove;
                }
                CommandAction::Attack => {
                    *input_mode = InputMode::CommandAttack;
                }
            }
        }
    }
}
