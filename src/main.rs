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
            day_duration: 10.0,//900.0, // 15 minutes for a full day/night cycle (10 min day + 5 min night)
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
    mut ambient_light: ResMut<AmbientLight>,
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
        // Calculate sun position in the sky based on time
        // At 6 AM (time 0), sun is at -PI/2 (eastern horizon)
        // At 12 PM (time 8), sun is at 0 (directly overhead)
        // At 6 PM (time 16), sun is at PI/2 (western horizon)
        let adjusted_sun_angle = sun_angle - std::f32::consts::FRAC_PI_2; // Shift to start from eastern horizon

        let x = adjusted_sun_angle.sin();
        let y = -adjusted_sun_angle.cos();
        let z = 0.3; // Slight Z offset to make the sun come from the east-west direction

        // Create a direction vector and normalize it
        let sun_direction = Vec3::new(x, y, z).normalize_or_zero();

        // Update sun rotation to point in the calculated direction
        sun_transform.rotation = Quat::from_rotation_arc(Vec3::Y, -sun_direction);

        // Calculate sun altitude (how high it is in the sky)
        let sun_altitude = (-sun_direction.y).max(0.05); // Minimum 0.05 to avoid complete darkness

        // Adjust sun intensity based on altitude (brighter when higher in sky)
        sun_light.illuminance = 10000.0 * sun_altitude;
        sun_light.shadows_enabled = sun_altitude > 0.1; // Disable shadows when sun is low

        // Adjust sun color based on time of day (warmer at sunrise/sunset)
        let hour_fraction = day_night_cycle.time / 16.0; // Normalize to 0-1 over the day period
        let warmth = (hour_fraction * std::f32::consts::PI).sin(); // Peaks at midday
        sun_light.color = Color::srgb(
            1.0,
            0.8 + 0.2 * warmth.abs(), // More yellow at midday
            0.6 + 0.2 * warmth.abs()  // More orange at sunrise/sunset
        );
    }

    // Update moon position and intensity
    if let Ok((_, mut moon_transform, mut moon_light)) = light_params.p1().get_single_mut() {
        // Calculate moon position in the sky based on time
        // Moon is opposite to sun during night
        let adjusted_moon_angle = moon_angle + std::f32::consts::PI; // Opposite side of the sky
        let x = adjusted_moon_angle.sin();
        let y = -adjusted_moon_angle.cos();
        let z = -0.3; // Slight Z offset in opposite direction

        // Create a direction vector and normalize it
        let moon_direction = Vec3::new(x, y, z).normalize_or_zero();

        // Update moon rotation to point in the calculated direction
        moon_transform.rotation = Quat::from_rotation_arc(Vec3::Y, -moon_direction);

        // Calculate moon altitude (how high it is in the sky)
        let moon_altitude = (-moon_direction.y).max(0.05); // Minimum 0.05 to avoid complete darkness

        // Adjust moon intensity based on altitude (much dimmer than sun)
        moon_light.illuminance = 1500.0 * moon_altitude;
        moon_light.color = Color::srgb(0.7, 0.7, 1.0); // Cool blue tint

        // Enable/disable moon shadows based on visibility
        moon_light.shadows_enabled = moon_altitude > 0.3;
    }

    // Update ambient light based on time of day
    if day_night_cycle.is_day {
        // Daytime - brighter ambient light
        let sun_altitude = if day_night_cycle.time >= 0.0 && day_night_cycle.time < 16.0 {
            let sun_time = day_night_cycle.time / 16.0 * std::f32::consts::PI;
            sun_time.sin().max(0.1) // Minimum ambient during day
        } else {
            0.1
        };

        ambient_light.brightness = 0.5 + 0.5 * sun_altitude; // Range from 0.5 to 1.0
        ambient_light.color = Color::srgb(1.0, 0.98, 0.95); // Slightly warm daylight
    } else {
        // Nighttime - dimmer ambient light
        let moon_altitude = if day_night_cycle.time >= 16.0 {
            let moon_time = (day_night_cycle.time - 16.0) / 8.0 * std::f32::consts::PI;
            moon_time.sin().max(0.05) // Minimum ambient at night
        } else {
            0.05
        };

        ambient_light.brightness = 0.1 + 0.1 * moon_altitude; // Range from 0.1 to 0.2
        ambient_light.color = Color::srgb(0.2, 0.2, 0.4); // Cool blue night light
    }
}
