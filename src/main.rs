mod player;
mod world;
mod ui;

use bevy::prelude::*;
use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use player::PlayerPlugin;
use world::WorldPlugin;
use ui::UiPlugin;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            FpsOverlayPlugin {
                config: FpsOverlayConfig {
                    text_config: TextStyle {
                        font_size: 20.0,
                        color: Color::srgb(0.0, 1.0, 0.0),
                        ..default()
                    },
                },
            },
        ))
        .insert_resource(ClearColor(Color::srgb(0.5, 0.8, 0.9)))
        .insert_resource(AmbientLight {
            color: Color::WHITE,
            brightness: 200.0,
        })
        .add_plugins((
            WorldPlugin,
            PlayerPlugin,
            UiPlugin,
        ))
        .run();
}
