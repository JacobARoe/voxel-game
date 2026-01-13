use bevy::prelude::*;
use crate::world::VoxelAssets;

#[derive(Resource, Default)]
pub struct Inventory {
    pub selected_slot: usize,
}

#[derive(Component)]
pub struct SelectedBlockUI;

#[derive(Component)]
pub struct SelectedBlockText;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Inventory::default())
            .add_systems(Startup, setup_ui)
            .add_systems(Update, (inventory_input, update_inventory_ui));
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

    // Inventory UI
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                right: Val::Px(20.0),
                width: Val::Px(50.0),
                height: Val::Px(50.0),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            border_color: Color::WHITE.into(),
            ..default()
        },
        SelectedBlockUI,
    ));

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
            bottom: Val::Px(30.0),
            right: Val::Px(80.0),
            ..default()
        }),
        SelectedBlockText,
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
    mut ui_query: Query<&mut BackgroundColor, With<SelectedBlockUI>>,
    mut text_query: Query<&mut Text, With<SelectedBlockText>>,
) {
    if let Ok(mut bg_color) = ui_query.get_single_mut() {
        if let Some(handle) = voxel_assets.block_types.get(inventory.selected_slot) {
            if let Some(material) = materials.get(handle) {
                bg_color.0 = material.base_color;
            }
        }
    }
    if let Ok(mut text) = text_query.get_single_mut() {
        if let Some(name) = voxel_assets.block_names.get(inventory.selected_slot) {
            text.sections[0].value = name.clone();
        }
    }
}
