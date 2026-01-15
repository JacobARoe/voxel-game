use bevy::prelude::*;
use bevy::input::mouse::MouseMotion;
use crate::world::{VoxelWorld, VoxelAssets, VoxelSounds, BlockType, Liquid, WaterSource, WaterDrain, NeedsMeshUpdate, Particle, DroppedItem, Furnace, CHUNK_SIZE};
use crate::mobs::SandSnake;
use crate::ui::Inventory;
use crate::ui::GameState;
use std::collections::HashSet;

#[derive(Component)]
pub struct Player {
    pub velocity: Vec3,
    pub flying: bool,
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct Health {
    pub value: i32,
    pub invulnerability_timer: Timer,
}

#[derive(Component, Default)]
pub struct MiningState {
    pub target: Option<IVec3>,
    pub progress: f32,
    pub crack_entity: Option<Entity>,
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_player)
           .add_systems(Update, (move_player, interact_terrain, check_snake_collision, pickup_items).run_if(in_state(GameState::Playing)));
    }
}

fn setup_player(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, world_gen: Res<crate::world::WorldGen>) {
    // Calculate spawn height based on terrain generation at (0,0)
    let (_, _, height, _) = crate::world::get_terrain_height(0, 0, &world_gen.perlin);

    // Spawn Player with a Camera child
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Capsule3d::new(0.4, 1.0)),
            material: materials.add(Color::srgb(0.8, 0.1, 0.1)),
            transform: Transform::from_xyz(0.0, height as f32 + 5.0, 0.0),
            ..default()
        },
        Player { velocity: Vec3::ZERO, flying: false },
        Health { value: 100, invulnerability_timer: Timer::from_seconds(1.0, TimerMode::Once) },
        MiningState::default(),
    )).with_children(|parent| {
        parent.spawn((
            Camera3dBundle {
                transform: Transform::from_xyz(0.0, 0.5, 0.0),
                ..default()
            },
            MainCamera,
            bevy::pbr::FogSettings {
                color: Color::srgb(0.5, 0.8, 0.9),
                falloff: bevy::pbr::FogFalloff::Linear {
                    start: 20.0,
                    end: 60.0,
                },
                ..default()
            },
            bevy::pbr::ScreenSpaceAmbientOcclusionSettings::default(),
        ));
    });
}

fn move_player(
    mut query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut windows: Query<&mut Window>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    voxel_world: Res<VoxelWorld>,
    block_type_query: Query<&BlockType>,
) {
    let mut window = windows.single_mut();

    // Lock cursor on click for FPS control
    if mouse_buttons.just_pressed(MouseButton::Left) {
        window.cursor.visible = false;
        window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
    }

    let (mut transform, mut player) = query.single_mut();

    // Mouse Look
    if window.cursor.grab_mode == bevy::window::CursorGrabMode::Locked {
        let sensitivity = 0.003;
        for event in mouse_motion.read() {
            transform.rotate_y(-event.delta.x * sensitivity);
            if let Ok(mut camera_transform) = camera_query.get_single_mut() {
                camera_transform.rotate_local_x(-event.delta.y * sensitivity);
            }
        }
    }

    let mut speed = 5.0;
    if keys.pressed(KeyCode::ShiftLeft) {
        speed = 10.0;
    }

    let mut direction = Vec3::ZERO;
    let forward = transform.forward();
    let right = transform.right();

    if keys.pressed(KeyCode::KeyW) { direction += *forward; }
    if keys.pressed(KeyCode::KeyS) { direction -= *forward; }
    if keys.pressed(KeyCode::KeyD) { direction += *right; }
    if keys.pressed(KeyCode::KeyA) { direction -= *right; }

    // Keep movement on the horizontal plane
    direction.y = 0.0;

    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        transform.translation += direction * speed * time.delta_seconds();
    }

    // Toggle Fly Mode
    if keys.just_pressed(KeyCode::KeyF) {
        player.flying = !player.flying;
        player.velocity = Vec3::ZERO;
    }

    // Physics / Gravity Logic
    let x = transform.translation.x.round() as i32;
    let z = transform.translation.z.round() as i32;

    // Scan for ground below player
    let mut ground_y = -50.0; // Default abyss
    let scan_start = transform.translation.y.floor() as i32;
    for y in (scan_start - 20..=scan_start).rev() {
        let check_pos = IVec3::new(x, y, z);
        if let Some(&entity) = voxel_world.blocks.get(&check_pos) {
            // Only collide if NOT water (index 4)
            if let Ok(block_type) = block_type_query.get(entity) {
                if block_type.0 != 4 {
                    ground_y = y as f32;
                    break;
                }
            }
        }
    }

    let target_y = ground_y + 1.5;

    if player.flying {
        if keys.pressed(KeyCode::Space) {
            transform.translation.y += speed * time.delta_seconds();
        }
        if keys.pressed(KeyCode::ControlLeft) {
            transform.translation.y -= speed * time.delta_seconds();
        }
    } else {
        // Apply Gravity
        player.velocity.y -= 30.0 * time.delta_seconds();
        transform.translation.y += player.velocity.y * time.delta_seconds();

        // Ground Collision & Jumping
        if transform.translation.y < target_y {
            transform.translation.y = target_y;
            player.velocity.y = 0.0;

            if keys.pressed(KeyCode::Space) {
                player.velocity.y = 12.0;
            }
        }
    }

    // Respawn if fell out of world
    if transform.translation.y < -30.0 {
        transform.translation = Vec3::new(0.0, 10.0, 0.0);
    }
}

fn interact_terrain(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    voxel_sounds: Res<VoxelSounds>,
    mut inventory: ResMut<Inventory>,
    camera_query: Query<&GlobalTransform, With<MainCamera>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window>,
    block_material_query: Query<&Handle<StandardMaterial>>,
    block_type_query: Query<&BlockType>,
    mut furnace_query: Query<&mut Furnace>,
    mut mining_state_query: Query<&mut MiningState, With<Player>>,
    time: Res<Time>,
) {
    let window = window_query.single();
    if window.cursor.grab_mode != bevy::window::CursorGrabMode::Locked {
        return;
    }

    if !mouse_buttons.pressed(MouseButton::Left) && !mouse_buttons.just_pressed(MouseButton::Right) {
        // Reset mining state if not pressing left
        if let Ok(mut state) = mining_state_query.get_single_mut() {
             if let Some(entity) = state.crack_entity {
                 commands.entity(entity).despawn();
             }
             state.crack_entity = None;
             state.target = None;
             state.progress = 0.0;
        }
        return;
    }

    let mut mined_this_frame = false;

    let camera_transform = camera_query.single();
    let mut ray_pos = camera_transform.translation();
    let ray_dir = camera_transform.forward();

    // Raycast up to 10 units
    for _ in 0..100 {
        ray_pos += ray_dir * 0.1;
        let block_pos = ray_pos.round().as_ivec3();

        if voxel_world.blocks.contains_key(&block_pos) {
            if mouse_buttons.pressed(MouseButton::Left) {
                let mut mining_state = mining_state_query.single_mut();
                mined_this_frame = true;
                
                // Check if target changed
                if mining_state.target != Some(block_pos) {
                    if let Some(entity) = mining_state.crack_entity {
                        commands.entity(entity).despawn();
                    }
                    mining_state.crack_entity = None;
                    mining_state.target = Some(block_pos);
                    mining_state.progress = 0.0;
                }

                // Calculate Mining Speed
                let mut speed = 1.0;
                if let Some(&entity) = voxel_world.blocks.get(&block_pos) {
                    if let Ok(block_type) = block_type_query.get(entity) {
                        if block_type.0 == 8 { return; } // Bedrock is unbreakable

                        let held_item = inventory.hotbar[inventory.selected_slot];
                        let best_tool = match block_type.0 {
                            2 | 15..=19 => Some(24), // Stone, Ores, Furnace -> Pickaxe
                            3 => Some(25), // Wood -> Axe
                            0 | 1 | 7 | 12 => Some(26), // Grass, Dirt, Sand, Snow -> Shovel
                            _ => None,
                        };

                        if let Some(tool_id) = best_tool {
                            if held_item == Some(tool_id) {
                                speed = 5.0;
                            }
                        }

                        // Instant break blocks
                        if [4, 5, 6, 11, 13, 14].contains(&block_type.0) {
                            speed = 100.0;
                        }
                    }
                }

                mining_state.progress += speed * time.delta_seconds();

                // Update Crack Visuals
                if mining_state.progress < 1.0 {
                    let stage = (mining_state.progress * 10.0).clamp(0.0, 9.0) as usize;
                    let mat = voxel_assets.crack_materials[stage].clone();
                    
                    if let Some(entity) = mining_state.crack_entity {
                        commands.entity(entity).insert(mat);
                    } else {
                        let id = commands.spawn(PbrBundle {
                            mesh: voxel_assets.mesh.clone(),
                            material: mat,
                            transform: Transform::from_xyz(block_pos.x as f32, block_pos.y as f32, block_pos.z as f32).with_scale(Vec3::splat(1.01)),
                            ..default()
                        }).id();
                        mining_state.crack_entity = Some(id);
                    }
                }

                if mining_state.progress < 1.0 {
                    break; // Still mining
                }
                // Block Broken! Reset state and continue to break logic
                if let Some(entity) = mining_state.crack_entity {
                    commands.entity(entity).despawn();
                }
                mining_state.crack_entity = None;
                mining_state.target = None;
                mining_state.progress = 0.0;

                // Identify blocks to break (Tree Felling Logic)
                let mut blocks_to_break = vec![block_pos];
                if let Some(&entity) = voxel_world.blocks.get(&block_pos) {
                    if let Ok(block_type) = block_type_query.get(entity) {
                        if block_type.0 == 3 { // Wood
                            let mut queue = vec![block_pos + IVec3::Y];
                            let mut visited = HashSet::new();
                            visited.insert(block_pos);
                            
                            while let Some(pos) = queue.pop() {
                                if visited.contains(&pos) { continue; }
                                if blocks_to_break.len() > 200 { break; } // Safety limit
                                
                                if let Some(&e) = voxel_world.blocks.get(&pos) {
                                    if let Ok(bt) = block_type_query.get(e) {
                                        if bt.0 == 3 || bt.0 == 11 { // Wood or Leaves
                                            visited.insert(pos);
                                            blocks_to_break.push(pos);
                                            
                                            queue.push(pos + IVec3::Y);
                                            queue.push(pos + IVec3::X);
                                            queue.push(pos + IVec3::NEG_X);
                                            queue.push(pos + IVec3::Z);
                                            queue.push(pos + IVec3::NEG_Z);
                                            queue.push(pos + IVec3::NEG_Y);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut played_sound = false;

                for pos in blocks_to_break {
                    // Check for Bedrock (Index 8)
                    if let Some(&entity) = voxel_world.blocks.get(&pos) {
                        if let Ok(block_type) = block_type_query.get(entity) {
                            if block_type.0 == 8 { continue; }
                            
                            // Spawn Dropped Item
                            if let Ok(mat) = block_material_query.get(entity) {
                                let mesh = if block_type.0 == 14 {
                                    voxel_assets.torch_mesh.clone()
                                } else {
                                    voxel_assets.mesh.clone()
                                };

                                commands.spawn((
                                    PbrBundle {
                                        mesh,
                                        material: mat.clone(),
                                        transform: Transform::from_translation(pos.as_vec3() + Vec3::splat(0.5)).with_scale(Vec3::splat(0.25)),
                                        ..default()
                                    },
                                    DroppedItem { block_type: block_type.0, velocity: Vec3::new(0.0, 4.0, 0.0) },
                                ));
                            }
                        }
                    }

                    if let Some(entity) = voxel_world.blocks.remove(&pos) {
                        // Spawn Particles
                        if let Ok(mat_handle) = block_material_query.get(entity) {
                            for i in 0..8 {
                                let r1 = (pos.x as f32 + i as f32 * 0.23).sin();
                                let r2 = (pos.y as f32 + i as f32 * 0.45).cos();
                                let r3 = (pos.z as f32 + i as f32 * 0.67).sin();
                                
                                commands.spawn((
                                    PbrBundle {
                                        mesh: voxel_assets.mesh.clone(),
                                        material: mat_handle.clone(),
                                        transform: Transform::from_xyz(
                                            pos.x as f32 + r1 * 0.5, 
                                            pos.y as f32 + r2.abs() * 0.5, 
                                            pos.z as f32 + r3 * 0.5
                                        ).with_scale(Vec3::splat(0.2)),
                                        ..default()
                                    },
                                    Particle {
                                        lifetime: Timer::from_seconds(0.5 + r1.abs() * 0.3, TimerMode::Once),
                                        velocity: Vec3::new(r1 * 4.0, r2.abs() * 5.0 + 2.0, r3 * 4.0),
                                    }
                                ));
                            }
                        }

                        commands.entity(entity).despawn();
                        
                        if !played_sound {
                            commands.spawn(AudioBundle {
                                source: voxel_sounds.break_sound.clone(),
                                settings: PlaybackSettings::DESPAWN,
                            });
                            played_sound = true;
                        }

                        // Update neighbors
                        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                            if let Some(&e) = voxel_world.blocks.get(&(pos + dir)) {
                                commands.entity(e).insert(NeedsMeshUpdate);
                            }
                        }
                    }
                }
            } else if mouse_buttons.just_pressed(MouseButton::Right) {
                let prev_pos = (ray_pos - ray_dir * 0.1).round().as_ivec3();
                
                // Check if interacting with a Furnace
                if let Some(&entity) = voxel_world.blocks.get(&block_pos) {
                    if let Ok(mut furnace) = furnace_query.get_mut(entity) {
                        // Try to take output
                        if let Some(output) = furnace.output.take() {
                            *inventory.items.entry(output).or_insert(0) += 1;
                            commands.spawn(AudioBundle {
                                source: voxel_sounds.place.clone(), // Reuse sound
                                settings: PlaybackSettings::DESPAWN,
                            });
                            return;
                        }
                        // Try to put input
                        if furnace.input.is_none() {
                            let hotbar_slot = inventory.selected_slot;
                            if let Some(block_idx) = inventory.hotbar[hotbar_slot] {
                                if [15, 16, 17, 18].contains(&block_idx) { // Ores
                                    if let Some(count) = inventory.items.get_mut(&block_idx) {
                                        if *count > 0 {
                                            *count -= 1;
                                            furnace.input = Some(block_idx);
                                            commands.spawn(AudioBundle {
                                                source: voxel_sounds.place.clone(),
                                                settings: PlaybackSettings::DESPAWN,
                                            });
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                        return; // Interacted with furnace, don't place block
                    }
                }

                if !voxel_world.blocks.contains_key(&prev_pos) {
                    let hotbar_slot = inventory.selected_slot;
                    if let Some(block_idx) = inventory.hotbar[hotbar_slot] {
                        // Prevent placing tools (24-26)
                        if block_idx >= 24 { return; }

                        // Prevent placing ingots (20-23)
                        if block_idx >= 20 { return; }

                        let count = inventory.items.entry(block_idx).or_insert(0);
                    
                        if *count > 0 {
                            if let Some(mat) = voxel_assets.block_types.get(block_idx) {
                                *count -= 1;

                                let mesh = if block_idx == 4 {
                                    voxel_assets.water_meshes[8].clone()
                                } else if block_idx == 14 {
                                    voxel_assets.torch_mesh.clone()
                                } else {
                                    voxel_assets.mesh.clone()
                                };

                                let mut entity_cmds = commands.spawn((
                                    PbrBundle {
                                        mesh,
                                        material: mat.clone(),
                                        transform: Transform::from_xyz(prev_pos.x as f32, prev_pos.y as f32, prev_pos.z as f32),
                                        ..default()
                                    },
                                    BlockType(block_idx),
                                ));

                                entity_cmds.insert(NeedsMeshUpdate);
                                // If placing water (index 4), add Liquid component
                                if block_idx == 4 {
                                    entity_cmds.insert(Liquid { level: 1 });
                                }
                                if block_idx == 5 { entity_cmds.insert(WaterSource); }
                                if block_idx == 6 { entity_cmds.insert(WaterDrain); }

                                if block_idx == 19 {
                                    entity_cmds.insert(Furnace {
                                        timer: Timer::from_seconds(3.0, TimerMode::Once),
                                        input: None,
                                        output: None,
                                    });
                                    entity_cmds.with_children(|parent| {
                                        // Chimney
                                        parent.spawn(PbrBundle {
                                            mesh: voxel_assets.chimney_mesh.clone(),
                                            material: voxel_assets.block_types[19].clone(),
                                            transform: Transform::from_xyz(0.0, 0.6, 0.0),
                                            ..default()
                                        });
                                        // Smelting Light
                                        parent.spawn(PointLightBundle {
                                            point_light: PointLight {
                                                intensity: 0.0,
                                                range: 8.0,
                                                color: Color::srgb(1.0, 0.3, 0.1),
                                                shadows_enabled: true,
                                                ..default()
                                            },
                                            transform: Transform::from_xyz(0.0, 0.0, 0.6),
                                            ..default()
                                        });
                                    });
                                }

                                if block_idx == 14 {
                                    entity_cmds.with_children(|parent| {
                                        parent.spawn(PointLightBundle {
                                            point_light: PointLight {
                                                intensity: 15000.0,
                                                range: 100.0,
                                                color: Color::srgb(1.0, 0.8, 0.2),
                                                shadows_enabled: true,
                                                ..default()
                                            },
                                            transform: Transform::from_xyz(0.0, 0.2, 0.0),
                                            ..default()
                                        });
                                    });
                                }

                                let id = entity_cmds.id();

                                voxel_world.blocks.insert(prev_pos, id);
                                commands.spawn(AudioBundle {
                                    source: voxel_sounds.place.clone(),
                                    settings: PlaybackSettings::DESPAWN,
                                });
                                
                                // Update neighbors
                                for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                    if let Some(&e) = voxel_world.blocks.get(&(prev_pos + dir)) {
                                        commands.entity(e).insert(NeedsMeshUpdate);
                                    }
                                }
                                // Register new block in the chunk system so it gets cleaned up later
                                let chunk_coord = IVec2::new(
                                    (prev_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                                    (prev_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                                );
                                if let Some(chunk) = voxel_world.chunks.get_mut(&chunk_coord) {
                                    chunk.push(prev_pos);
                                }
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    if !mined_this_frame {
        if let Ok(mut state) = mining_state_query.get_single_mut() {
             if let Some(entity) = state.crack_entity {
                 commands.entity(entity).despawn();
             }
             state.crack_entity = None;
             state.target = None;
             state.progress = 0.0;
        }
    }
}

fn pickup_items(
    mut commands: Commands,
    mut inventory: ResMut<Inventory>,
    player_query: Query<&Transform, With<Player>>,
    item_query: Query<(Entity, &Transform, &DroppedItem)>,
) {
    if let Ok(player_transform) = player_query.get_single() {
        for (entity, item_transform, item) in item_query.iter() {
            if player_transform.translation.distance(item_transform.translation) < 1.5 {
                *inventory.items.entry(item.block_type).or_insert(0) += 1;

                // Auto-populate hotbar if empty slot exists and item not already in hotbar
                let in_hotbar = inventory.hotbar.iter().any(|slot| *slot == Some(item.block_type));
                if !in_hotbar {
                    if let Some(slot) = inventory.hotbar.iter_mut().find(|s| s.is_none()) {
                        *slot = Some(item.block_type);
                    }
                }

                commands.entity(entity).despawn();
            }
        }
    }
}

fn check_snake_collision(
    mut player_query: Query<(&Transform, &mut Health, &mut Player)>,
    snake_query: Query<&Transform, With<SandSnake>>,
    time: Res<Time>,
) {
    if let Ok((player_transform, mut health, mut player)) = player_query.get_single_mut() {
        health.invulnerability_timer.tick(time.delta());

        if !health.invulnerability_timer.finished() {
            return;
        }

        for snake_transform in snake_query.iter() {
            if player_transform.translation.distance(snake_transform.translation) < 1.2 {
                health.value -= 10;
                health.invulnerability_timer.reset();
                info!("Player hit by snake! Health: {}", health.value);
                
                // Knockback
                let dir = (player_transform.translation - snake_transform.translation).normalize_or_zero();
                player.velocity += dir * 15.0 + Vec3::Y * 5.0;
            }
        }
    }
}
