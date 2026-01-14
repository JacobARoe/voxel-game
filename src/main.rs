mod player;
mod world;
mod ui;
mod mobs;

use bevy::prelude::*;

#[derive(Resource)]
pub struct DayNightCycle {
    pub time: f32,          // 0.0 to 24.0 (hours)
    pub day_duration: f32,  // Duration of a full day in seconds
    pub is_day: bool,
}

impl Default for DayNightCycle {
    fn default() -> Self {
        Self {
            time: 6.0, // Start at 6 AM
            day_duration: 900.0, // 15 minutes for a full day/night cycle (10 min day + 5 min night)
            is_day: true,
        }
    }
}

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct Moon;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use player::PlayerPlugin;
use world::WorldPlugin;
use ui::UiPlugin;
use mobs::MobsPlugin;

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
        .insert_resource(DayNightCycle::default())
        .add_plugins((
            WorldPlugin,
            PlayerPlugin,
            UiPlugin,
            MobsPlugin,
        ))
        .add_systems(Startup, setup_day_night_cycle)
        .add_systems(Update, update_day_night_cycle)
        .run();
}

fn setup_day_night_cycle(
    mut commands: Commands,
) {
    // Create the sun (day light)
    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight {
                shadows_enabled: true,
                illuminance: 10000.0,
                color: Color::srgb(1.0, 0.9, 0.7), // Warm yellowish light
                ..default()
            },
            transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -std::f32::consts::PI * 0.3, 0.0, 0.0)),
            ..default()
        },
        Sun,
    ));

    // Create the moon (night light)
    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight {
                shadows_enabled: false, // Moon doesn't cast strong shadows
                illuminance: 1000.0, // Much dimmer than the sun
                color: Color::srgb(0.7, 0.7, 1.0), // Cool bluish light
                ..default()
            },
            transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, std::f32::consts::PI * 0.3, 0.0, 0.0)), // Opposite of sun initially
            ..default()
        },
        Moon,
    ));
}

fn update_day_night_cycle(
    mut day_night_cycle: ResMut<DayNightCycle>,
    mut light_params: ParamSet<(
        Query<(Entity, &mut Transform, &mut DirectionalLight), With<Sun>>,
        Query<(Entity, &mut Transform, &mut DirectionalLight), With<Moon>>,
    )>,
    time: Res<Time>,
) {
    // Update time - 15 minutes total cycle (10 min day + 5 min night)
    day_night_cycle.time += 24.0 * time.delta_seconds() / day_night_cycle.day_duration;
    if day_night_cycle.time >= 24.0 {
        day_night_cycle.time -= 24.0;
    }

    // Determine if it's day or night (16 hours of day to represent 10 minutes real time, 8 hours of night to represent 5 minutes real time)
    day_night_cycle.is_day = day_night_cycle.time >= 0.0 && day_night_cycle.time < 16.0; // 0:00 to 16:00 is 16 hours of day (representing 10 minutes real time)

    // Calculate sun and moon positions based on time
    // Sun moves across the sky during 16 hours of day (0:00 to 16:00)
    let sun_angle = if day_night_cycle.is_day {
        // Sun moves from sunrise to sunset during the 16-hour day period
        let normalized_time = day_night_cycle.time / 16.0; // 0.0 to 1.0 during day
        normalized_time * std::f32::consts::PI // From 0 to PI (sunrise to sunset)
    } else {
        // Sun is below horizon during night
        0.0
    };

    // Moon moves across the sky during the night period (16:00-24:00 game time = 8 hours of game time for 5 minutes real time)
    let moon_angle = if !day_night_cycle.is_day {
        // Night time is from 16.0 to 24.0 (8 hours of game time)
        // Normalize the time within the night period (0.0 to 1.0)
        let normalized_night_time = (day_night_cycle.time - 16.0) / 8.0;
        normalized_night_time * std::f32::consts::PI // From 0 to PI (moonrise to moonset)
    } else {
        0.0
    };

    // Update sun position and intensity
    if let Ok((_, mut sun_transform, mut sun_light)) = light_params.p0().get_single_mut() {
        let x = sun_angle.sin();
        let y = -sun_angle.cos();
        sun_transform.rotation = Quat::from_rotation_arc(Vec3::Y, Vec3::new(x, y, 0.0).normalize_or_zero());

        // Calculate sun altitude (how high it is in the sky)
        let sun_direction = sun_transform.forward();
        let sun_altitude = (-sun_direction.y).max(0.0); // Flip Y since forward is negative Y

        // Adjust sun intensity based on altitude (brighter when higher in sky)
        sun_light.illuminance = 10000.0 * sun_altitude;
        sun_light.shadows_enabled = sun_altitude > 0.1; // Disable shadows when sun is low
    }

    // Update moon position and intensity
    if let Ok((_, mut moon_transform, mut moon_light)) = light_params.p1().get_single_mut() {
        let x = moon_angle.sin();
        let y = -moon_angle.cos();
        moon_transform.rotation = Quat::from_rotation_arc(Vec3::Y, Vec3::new(x, y, 0.0).normalize_or_zero());

        // Calculate moon altitude (how high it is in the sky)
        let moon_direction = moon_transform.forward();
        let moon_altitude = (-moon_direction.y).max(0.0); // Flip Y since forward is negative Y

        // Adjust moon intensity based on altitude (dimmer than sun)
        moon_light.illuminance = 1500.0 * moon_altitude;
        moon_light.color = Color::srgb(0.7, 0.7, 1.0); // Cool blue tint
    }
}
