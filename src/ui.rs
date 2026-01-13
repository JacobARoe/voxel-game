use bevy::prelude::*;
use crate::world::VoxelAssets;
use crate::player::{Health, Player};
use std::collections::HashMap;

#[derive(Resource)]
pub struct Inventory {
    pub selected_slot: usize,
    pub items: HashMap<usize, u32>,
}

impl Default for Inventory {
    fn default() -> Self {
        let mut items = HashMap::new();
        // Start with 64 of each basic block type
        for i in 0..10 { items.insert(i, 64); }
        Self {
            selected_slot: 0,
            items,
        }
    }
}

#[derive(Component)]
pub struct HotbarSlot {
    pub index: usize,
}

#[derive(Component)]
pub struct SelectedBlockText;

#[derive(Component)]
pub struct HealthText;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Inventory::default())
            .add_systems(Startup, setup_ui)
            .add_systems(Update, (inventory_input, update_inventory_ui, update_health_ui));
    }
}

fn setup_ui(mut commands: Commands) {
    // Crosshair UI
    commands.spawn(NodeBundle {
        style: Style {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        background_color: Color::NONE.into(),
        ..default()
    }).with_children(|parent| {
        parent.spawn(NodeBundle {
            style: Style {
                width: Val::Px(5.0),
                height: Val::Px(5.0),
                ..default()
            },
            background_color: Color::WHITE.into(),
            ..default()
        });
    });

    // Hotbar UI
    commands.spawn(NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        background_color: Color::NONE.into(),
        ..default()
    }).with_children(|parent| {
        for i in 0..8 {
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Px(50.0),
                        height: Val::Px(50.0),
                        margin: UiRect::all(Val::Px(2.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    border_color: Color::BLACK.into(),
                    background_color: Color::srgba(0.2, 0.2, 0.2, 0.8).into(),
                    ..default()
                },
                HotbarSlot { index: i },
            )).with_children(|slot| {
                // Icon
                slot.spawn(NodeBundle {
                    style: Style {
                        width: Val::Px(30.0),
                        height: Val::Px(30.0),
                        ..default()
                    },
                    background_color: Color::WHITE.into(),
                    ..default()
                });
                // Count
                slot.spawn(TextBundle::from_section(
                    "0",
                    TextStyle {
                        font_size: 16.0,
                        color: Color::WHITE,
                        ..default()
                    },
                ).with_style(Style {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(2.0),
                    right: Val::Px(2.0),
                    ..default()
                }));
            });
        }
    });

    // Block Name Text
    commands.spawn((
        TextBundle::from_section(
            "Grass",
            TextStyle {
                font_size: 30.0,
                color: Color::WHITE,
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            bottom: Val::Px(80.0),
            right: Val::Px(80.0),
            ..default()
        }),
        SelectedBlockText,
    ));

    // Health Text
    commands.spawn((
        TextBundle::from_section(
            "Health: 100",
            TextStyle {
                font_size: 30.0,
                color: Color::srgb(1.0, 0.0, 0.0),
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(20.0),
            left: Val::Px(20.0),
            ..default()
        }),
        HealthText,
    ));
}

fn inventory_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut inventory: ResMut<Inventory>,
) {
    if keys.just_pressed(KeyCode::Digit1) { inventory.selected_slot = 0; }
    if keys.just_pressed(KeyCode::Digit2) { inventory.selected_slot = 1; }
    if keys.just_pressed(KeyCode::Digit3) { inventory.selected_slot = 2; }
    if keys.just_pressed(KeyCode::Digit4) { inventory.selected_slot = 3; }
    if keys.just_pressed(KeyCode::Digit5) { inventory.selected_slot = 4; }
    if keys.just_pressed(KeyCode::Digit6) { inventory.selected_slot = 5; }
    if keys.just_pressed(KeyCode::Digit7) { inventory.selected_slot = 6; }
    if keys.just_pressed(KeyCode::Digit8) { inventory.selected_slot = 7; }
}

fn update_inventory_ui(
    inventory: Res<Inventory>,
    voxel_assets: Res<VoxelAssets>,
    materials: Res<Assets<StandardMaterial>>,
    mut hotbar_query: Query<(&HotbarSlot, &mut BorderColor, &Children)>,
    mut bg_query: Query<&mut BackgroundColor>,
    mut text_query: Query<&mut Text, Without<SelectedBlockText>>,
    mut selected_text_query: Query<&mut Text, With<SelectedBlockText>>,
) {
    for (slot, mut border, children) in hotbar_query.iter_mut() {
        if slot.index == inventory.selected_slot {
            border.0 = Color::WHITE;
        } else {
            border.0 = Color::BLACK;
        }

        if let Some(&child) = children.get(0) {
            if let Ok(mut bg) = bg_query.get_mut(child) {
                if let Some(handle) = voxel_assets.block_types.get(slot.index) {
                    if let Some(mat) = materials.get(handle) {
                        bg.0 = mat.base_color;
                    }
                }
            }
        }

        if let Some(&child) = children.get(1) {
            if let Ok(mut text) = text_query.get_mut(child) {
                let count = inventory.items.get(&slot.index).unwrap_or(&0);
                text.sections[0].value = format!("{}", count);
            }
        }
    }

    if let Ok(mut text) = selected_text_query.get_single_mut() {
        if let Some(name) = voxel_assets.block_names.get(inventory.selected_slot) {
            text.sections[0].value = name.clone();
        }
    }
}

fn update_health_ui(
    mut text_query: Query<&mut Text, With<HealthText>>,
    player_query: Query<&Health, With<Player>>,
) {
    if let Ok(health) = player_query.get_single() {
        for mut text in text_query.iter_mut() {
            text.sections[0].value = format!("Health: {}", health.value);
        }
    }
}
