/// Macro to spawn a styled button with text
macro_rules! spawn_button {
    ($parent:expr, $text:expr, $action:expr) => {
        $parent.spawn((
            Button,
            Node {
                width: Val::Px(200.0),
                height: Val::Px(50.0),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::from(Color::BLACK),
            BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
            $action,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new($text),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
    };
}

pub(crate) use spawn_button;
