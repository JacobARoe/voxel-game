use bevy::prelude::*;
use crate::world::VoxelAssets;
use crate::player::{Health, Player};
use bevy::input::mouse::MouseWheel;
use std::collections::HashMap;

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    #[default]
    Playing,
    Paused,
    Crafting,
    Inventory,
    GameOver,
}

#[derive(Resource)]
pub struct SnakeSettings {
    pub enabled: bool,
}

impl Default for SnakeSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Component)]
pub struct SnakeToggleButton;

#[derive(Component)]
pub struct CraftingMenu;

#[derive(Component)]
pub struct CraftButton {
    pub input: usize,
    pub input_count: u32,
    pub output: usize,
    pub output_count: u32,
}

#[derive(Component)]
pub struct InventoryMenu;

#[derive(Component)]
pub struct InventoryItemButton {
    pub block_index: usize,
}

#[derive(Resource)]
pub struct Inventory {
    pub selected_slot: usize,
    pub hotbar: [Option<usize>; 9],
    pub items: HashMap<usize, u32>,
}

impl Default for Inventory {
    fn default() -> Self {
        let mut items = HashMap::new();
        // Start with 64 of each basic block type
        for i in 0..29 { items.insert(i, 64); }
        Self {
            selected_slot: 0,
            hotbar: [None; 9], // Default loadout
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

#[derive(Component)]
pub struct PauseMenu;

#[derive(Component)]
pub struct DamageOverlay;

#[derive(Component)]
pub struct DeathMenu;

#[derive(Component)]
pub struct RespawnButton;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .insert_resource(Inventory::default())
            .insert_resource(SnakeSettings::default())
            .add_systems(Startup, setup_ui)
            .add_systems(Update, (inventory_input.run_if(in_state(GameState::Playing).or_else(in_state(GameState::Inventory))), update_inventory_ui, update_health_ui, update_damage_overlay, toggle_pause, toggle_crafting, toggle_inventory, handle_snake_toggle, handle_crafting_click, handle_inventory_click, handle_respawn_click))
            .add_systems(OnEnter(GameState::Paused), spawn_pause_menu)
            .add_systems(OnExit(GameState::Paused), despawn_pause_menu)
            .add_systems(OnEnter(GameState::Crafting), spawn_crafting_menu)
            .add_systems(OnExit(GameState::Crafting), despawn_crafting_menu)
            .add_systems(OnEnter(GameState::Inventory), spawn_inventory_menu)
            .add_systems(OnExit(GameState::Inventory), despawn_inventory_menu)
            .add_systems(OnEnter(GameState::GameOver), spawn_death_menu)
            .add_systems(OnExit(GameState::GameOver), despawn_death_menu);
    }
}

fn setup_ui(mut commands: Commands) {
    // Damage Overlay
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            background_color: Color::srgba(1.0, 0.0, 0.0, 0.0).into(),
            z_index: ZIndex::Global(5),
            ..default()
        },
        DamageOverlay,
    ));

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
        for i in 0..9 {
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
    mut mouse_wheel: EventReader<MouseWheel>,
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
    if keys.just_pressed(KeyCode::Digit9) { inventory.selected_slot = 8; }

    for event in mouse_wheel.read() {
        if event.y > 0.0 {
            inventory.selected_slot = (inventory.selected_slot + 9 - 1) % 9;
        } else if event.y < 0.0 {
            inventory.selected_slot = (inventory.selected_slot + 1) % 9;
        }
    }
}

fn spawn_death_menu(mut commands: Commands, mut windows: Query<&mut Window>) {
    let mut window = windows.single_mut();
    window.cursor.visible = true;
    window.cursor.grab_mode = bevy::window::CursorGrabMode::None;

    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                position_type: PositionType::Absolute,
                ..default()
            },
            background_color: Color::srgba(0.5, 0.0, 0.0, 0.8).into(),
            z_index: ZIndex::Global(20),
            ..default()
        },
        DeathMenu,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section(
            "YOU DIED",
            TextStyle {
                font_size: 80.0,
                color: Color::WHITE,
                ..default()
            },
        ));

        parent.spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(200.0),
                    height: Val::Px(60.0),
                    margin: UiRect::top(Val::Px(40.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgb(0.3, 0.3, 0.3).into(),
                ..default()
            },
            RespawnButton,
        )).with_children(|btn| {
            btn.spawn(TextBundle::from_section(
                "Respawn",
                TextStyle { font_size: 30.0, color: Color::WHITE, ..default() }
            ));
        });
    });
}

fn despawn_death_menu(mut commands: Commands, query: Query<Entity, With<DeathMenu>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn handle_respawn_click(
    mut interaction_query: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<RespawnButton>)>,
    mut next_state: ResMut<NextState<GameState>>,
    mut player_query: Query<(&mut Transform, &mut Health), With<Player>>,
    mut windows: Query<&mut Window>,
    world_gen: Res<crate::world::WorldGen>,
) {
    for (interaction, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                if let Ok((mut transform, mut health)) = player_query.get_single_mut() {
                    health.value = 100;
                    let (_, _, height, _) = crate::world::get_terrain_height(0, 0, &world_gen.perlin);
                    transform.translation = Vec3::new(0.0, height as f32 + 5.0, 0.0);
                }
                next_state.set(GameState::Playing);
                
                let mut window = windows.single_mut();
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            Interaction::Hovered => bg.0 = Color::srgb(0.4, 0.4, 0.4).into(),
            Interaction::None => bg.0 = Color::srgb(0.3, 0.3, 0.3).into(),
        }
    }
}

fn update_damage_overlay(
    mut query: Query<&mut BackgroundColor, With<DamageOverlay>>,
    player_query: Query<&Health, With<Player>>,
) {
    if let Ok(health) = player_query.get_single() {
        if let Ok(mut bg) = query.get_single_mut() {
            let alpha = if !health.invulnerability_timer.finished() {
                let t = health.invulnerability_timer.fraction_remaining();
                t * 0.5 // Max 0.5 alpha
            } else {
                0.0
            };
            bg.0 = Color::srgba(1.0, 0.0, 0.0, alpha).into();
        }
    }
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
                if let Some(block_idx) = inventory.hotbar[slot.index] {
                    if let Some(handle) = voxel_assets.block_types.get(block_idx) {
                        if let Some(mat) = materials.get(handle) {
                            bg.0 = mat.base_color;
                        }
                    }
                } else {
                    bg.0 = Color::NONE.into();
                }
            }
        }

        if let Some(&child) = children.get(1) {
            if let Ok(mut text) = text_query.get_mut(child) {
                let count = if let Some(block_idx) = inventory.hotbar[slot.index] { *inventory.items.get(&block_idx).unwrap_or(&0) } else { 0 };
                text.sections[0].value = format!("{}", count);
            }
        }
    }

    if let Ok(mut text) = selected_text_query.get_single_mut() {
        if let Some(block_idx) = inventory.hotbar[inventory.selected_slot] {
            if let Some(name) = voxel_assets.block_names.get(block_idx) {
                text.sections[0].value = name.clone();
            }
        } else {
            text.sections[0].value = "".to_string();
        }
    }
}

fn toggle_crafting(
    mut next_state: ResMut<NextState<GameState>>,
    state: Res<State<GameState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        let mut window = windows.single_mut();
        match state.get() {
            GameState::Playing => {
                next_state.set(GameState::Crafting);
                window.cursor.visible = true;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::None;
            },
            GameState::Crafting => {
                next_state.set(GameState::Playing);
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            _ => {}
        }
    }
}

fn toggle_inventory(
    mut next_state: ResMut<NextState<GameState>>,
    state: Res<State<GameState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        let mut window = windows.single_mut();
        match state.get() {
            GameState::Playing => {
                next_state.set(GameState::Inventory);
                window.cursor.visible = true;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::None;
            },
            GameState::Inventory => {
                next_state.set(GameState::Playing);
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            _ => {}
        }
    }
}

fn toggle_pause(
    mut next_state: ResMut<NextState<GameState>>,
    state: Res<State<GameState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        let mut window = windows.single_mut();
        match state.get() {
            GameState::Playing => {
                next_state.set(GameState::Paused);
                window.cursor.visible = true;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::None;
            },
            GameState::Paused => {
                next_state.set(GameState::Playing);
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            GameState::Crafting => {
                next_state.set(GameState::Playing);
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            GameState::Inventory => {
                next_state.set(GameState::Playing);
                window.cursor.visible = false;
                window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
            },
            _ => {}
        }
    }
}

fn spawn_pause_menu(mut commands: Commands, snake_settings: Res<SnakeSettings>) {
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                position_type: PositionType::Absolute,
                ..default()
            },
            background_color: Color::srgba(0.0, 0.0, 0.0, 0.5).into(),
            z_index: ZIndex::Global(10),
            ..default()
        },
        PauseMenu,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section(
            "PAUSED",
            TextStyle {
                font_size: 60.0,
                color: Color::WHITE,
                ..default()
            },
        ));
        
        // Snake Toggle Button
        parent.spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(250.0),
                    height: Val::Px(60.0),
                    margin: UiRect::top(Val::Px(20.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgb(0.2, 0.2, 0.2).into(),
                ..default()
            },
            SnakeToggleButton,
        )).with_children(|btn| {
            let text = if snake_settings.enabled { "Snakes: ON" } else { "Snakes: OFF" };
            btn.spawn(TextBundle::from_section(
                text,
                TextStyle { font_size: 30.0, color: Color::WHITE, ..default() }
            ));
        });
    });
}

fn despawn_pause_menu(mut commands: Commands, query: Query<Entity, With<PauseMenu>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn handle_snake_toggle(
    mut interaction_query: Query<(&Interaction, &Children), (Changed<Interaction>, With<SnakeToggleButton>)>,
    mut text_query: Query<&mut Text>,
    mut snake_settings: ResMut<SnakeSettings>,
) {
    for (interaction, children) in interaction_query.iter_mut() {
        if *interaction == Interaction::Pressed {
            snake_settings.enabled = !snake_settings.enabled;
            if let Ok(mut text) = text_query.get_mut(children[0]) {
                text.sections[0].value = format!("Snakes: {}", if snake_settings.enabled { "ON" } else { "OFF" });
            }
        }
    }
}

fn spawn_crafting_menu(mut commands: Commands, voxel_assets: Res<VoxelAssets>) {
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                position_type: PositionType::Absolute,
                ..default()
            },
            background_color: Color::srgba(0.0, 0.0, 0.0, 0.8).into(),
            z_index: ZIndex::Global(10),
            ..default()
        },
        CraftingMenu,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section(
            "CRAFTING (Press C to Close)",
            TextStyle {
                font_size: 40.0,
                color: Color::WHITE,
                ..default()
            },
        ));

        let recipes = vec![
            (1, 2, 0, 1), // 2 Dirt -> 1 Grass
            (2, 2, 7, 1), // 2 Stone -> 1 Sand
            (7, 2, 1, 1), // 2 Sand -> 1 Dirt
            (3, 1, 11, 4), // 1 Wood -> 4 Leaves
            (3, 1, 14, 4), // 1 Wood -> 4 Torches
            (2, 8, 19, 1), // 8 Stone -> 1 Furnace
            (3, 2, 24, 1), // 2 Wood -> 1 Pickaxe
            (3, 2, 25, 1), // 2 Wood -> 1 Axe
            (3, 2, 26, 1), // 2 Wood -> 1 Shovel
        ];

        for (in_idx, in_count, out_idx, out_count) in recipes {
             let in_name = voxel_assets.block_names.get(in_idx).unwrap_or(&"?".to_string()).clone();
             let out_name = voxel_assets.block_names.get(out_idx).unwrap_or(&"?".to_string()).clone();
             
             parent.spawn((
                ButtonBundle {
                    style: Style {
                        width: Val::Px(400.0),
                        height: Val::Px(50.0),
                        margin: UiRect::all(Val::Px(5.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgb(0.3, 0.3, 0.3).into(),
                    ..default()
                },
                CraftButton { input: in_idx, input_count: in_count, output: out_idx, output_count: out_count },
             )).with_children(|btn| {
                 btn.spawn(TextBundle::from_section(
                     format!("{} {} -> {} {}", in_count, in_name, out_count, out_name),
                     TextStyle { font_size: 20.0, color: Color::WHITE, ..default() }
                 ));
             });
        }
    });
}

fn despawn_crafting_menu(mut commands: Commands, query: Query<Entity, With<CraftingMenu>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn handle_crafting_click(
    mut interaction_query: Query<(&Interaction, &CraftButton, &mut BackgroundColor), (Changed<Interaction>, With<CraftButton>)>,
    mut inventory: ResMut<Inventory>,
) {
    for (interaction, recipe, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                let has_count = *inventory.items.get(&recipe.input).unwrap_or(&0);
                if has_count >= recipe.input_count {
                    *inventory.items.entry(recipe.input).or_insert(0) -= recipe.input_count;
                    *inventory.items.entry(recipe.output).or_insert(0) += recipe.output_count;
                    bg.0 = Color::srgb(0.2, 0.8, 0.2).into(); // Flash green
                } else {
                    bg.0 = Color::srgb(0.8, 0.2, 0.2).into(); // Flash red
                }
            },
            Interaction::Hovered => {
                bg.0 = Color::srgb(0.4, 0.4, 0.4).into();
            },
            Interaction::None => {
                bg.0 = Color::srgb(0.3, 0.3, 0.3).into();
            }
        }
    }
}

fn spawn_inventory_menu(mut commands: Commands, voxel_assets: Res<VoxelAssets>, materials: Res<Assets<StandardMaterial>>) {
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                position_type: PositionType::Absolute,
                ..default()
            },
            background_color: Color::srgba(0.0, 0.0, 0.0, 0.8).into(),
            z_index: ZIndex::Global(10),
            ..default()
        },
        InventoryMenu,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section(
            "INVENTORY (Tab to Close)",
            TextStyle { font_size: 40.0, color: Color::WHITE, ..default() },
        ));
        
        parent.spawn(TextBundle::from_section(
            "Click a block to assign it to the selected hotbar slot",
            TextStyle { font_size: 20.0, color: Color::srgb(0.5, 0.5, 0.5), ..default() },
        ));

        parent.spawn(NodeBundle {
            style: Style {
                display: Display::Grid,
                grid_template_columns: vec![GridTrack::auto(); 8], // 8 columns
                margin: UiRect::top(Val::Px(20.0)),
                ..default()
            },
            ..default()
        }).with_children(|grid| {
            for (i, name) in voxel_assets.block_names.iter().enumerate() {
                grid.spawn((
                    ButtonBundle {
                        style: Style {
                            width: Val::Px(80.0),
                            height: Val::Px(80.0),
                            margin: UiRect::all(Val::Px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        background_color: Color::srgb(0.2, 0.2, 0.2).into(),
                        ..default()
                    },
                    InventoryItemButton { block_index: i },
                )).with_children(|btn| {
                    // Color preview
                    if let Some(handle) = voxel_assets.block_types.get(i) {
                         if let Some(mat) = materials.get(handle) {
                             btn.spawn(NodeBundle {
                                 style: Style {
                                     width: Val::Px(30.0),
                                     height: Val::Px(30.0),
                                     margin: UiRect::bottom(Val::Px(5.0)),
                                     ..default()
                                 },
                                 background_color: mat.base_color.into(),
                                 ..default()
                             });
                         }
                    }
                    
                    btn.spawn(TextBundle::from_section(
                        name,
                        TextStyle { font_size: 14.0, color: Color::WHITE, ..default() }
                    ));
                });
            }
        });
    });
}

fn despawn_inventory_menu(mut commands: Commands, query: Query<Entity, With<InventoryMenu>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn handle_inventory_click(
    mut interaction_query: Query<(&Interaction, &InventoryItemButton, &mut BackgroundColor), (Changed<Interaction>, With<InventoryItemButton>)>,
    mut inventory: ResMut<Inventory>,
) {
    for (interaction, button, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                let slot = inventory.selected_slot;
                inventory.hotbar[slot] = Some(button.block_index);
                bg.0 = Color::srgb(0.2, 0.8, 0.2).into();
            },
            Interaction::Hovered => {
                bg.0 = Color::srgb(0.4, 0.4, 0.4).into();
            },
            Interaction::None => {
                bg.0 = Color::srgb(0.2, 0.2, 0.2).into();
            }
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
