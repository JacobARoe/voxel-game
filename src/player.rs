use bevy::prelude::*;
use bevy::input::mouse::MouseMotion;
use crate::world::{VoxelWorld, VoxelAssets, VoxelSounds, BlockType, Liquid, WaterSource, WaterDrain, NeedsMeshUpdate, Particle, SandSnake, CHUNK_SIZE};
use crate::ui::Inventory;

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

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_player)
           .add_systems(Update, (move_player, interact_terrain, check_snake_collision));
    }
}

fn setup_player(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, world_gen: Res<crate::world::WorldGen>) {
    // Calculate spawn height based on terrain generation at (0,0)
    let (_, _, height) = crate::world::get_terrain_height(0, 0, &world_gen.perlin);

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
    if keys.just_pressed(KeyCode::Escape) {
        window.cursor.visible = true;
        window.cursor.grab_mode = bevy::window::CursorGrabMode::None;
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
    let current_y = transform.translation.y.round() as i32;

    // Scan for ground below player
    let mut ground_y = -50.0; // Default abyss
    for y in (current_y - 20..=current_y + 2).rev() {
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
) {
    let window = window_query.single();
    if window.cursor.grab_mode != bevy::window::CursorGrabMode::Locked {
        return;
    }

    if !mouse_buttons.just_pressed(MouseButton::Left) && !mouse_buttons.just_pressed(MouseButton::Right) {
        return;
    }

    let camera_transform = camera_query.single();
    let mut ray_pos = camera_transform.translation();
    let ray_dir = camera_transform.forward();

    // Raycast up to 10 units
    for _ in 0..100 {
        ray_pos += ray_dir * 0.1;
        let block_pos = ray_pos.round().as_ivec3();

        if voxel_world.blocks.contains_key(&block_pos) {
            if mouse_buttons.just_pressed(MouseButton::Left) {
                // Check for Bedrock (Index 8)
                if let Some(&entity) = voxel_world.blocks.get(&block_pos) {
                    if let Ok(block_type) = block_type_query.get(entity) {
                        if block_type.0 == 8 { continue; }
                        // Add to inventory
                        *inventory.items.entry(block_type.0).or_insert(0) += 1;
                    }
                }

                if let Some(entity) = voxel_world.blocks.remove(&block_pos) {
                    // Spawn Particles
                    if let Ok(mat_handle) = block_material_query.get(entity) {
                        for i in 0..8 {
                            let r1 = (block_pos.x as f32 + i as f32 * 0.23).sin();
                            let r2 = (block_pos.y as f32 + i as f32 * 0.45).cos();
                            let r3 = (block_pos.z as f32 + i as f32 * 0.67).sin();
                            
                            commands.spawn((
                                PbrBundle {
                                    mesh: voxel_assets.mesh.clone(),
                                    material: mat_handle.clone(),
                                    transform: Transform::from_xyz(
                                        block_pos.x as f32 + r1 * 0.5, 
                                        block_pos.y as f32 + r2.abs() * 0.5, 
                                        block_pos.z as f32 + r3 * 0.5
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
                    commands.spawn(AudioBundle {
                        source: voxel_sounds.break_sound.clone(),
                        settings: PlaybackSettings::DESPAWN,
                    });

                    // Update neighbors
                    for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                        if let Some(&e) = voxel_world.blocks.get(&(block_pos + dir)) {
                            commands.entity(e).insert(NeedsMeshUpdate);
                        }
                    }
                }
            } else if mouse_buttons.just_pressed(MouseButton::Right) {
                let prev_pos = (ray_pos - ray_dir * 0.1).round().as_ivec3();
                if !voxel_world.blocks.contains_key(&prev_pos) {
                    let slot = inventory.selected_slot;
                    let count = inventory.items.entry(slot).or_insert(0);
                    
                    if *count > 0 {
                        if let Some(mat) = voxel_assets.block_types.get(slot) {
                            *count -= 1;

                            let mesh = if slot == 4 {
                                voxel_assets.water_meshes[8].clone()
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
                            BlockType(slot),
                        ));

                        entity_cmds.insert(NeedsMeshUpdate);
                        // If placing water (index 4), add Liquid component
                        if slot == 4 {
                            entity_cmds.insert(Liquid { level: 1 });
                        }
                        if slot == 5 { entity_cmds.insert(WaterSource); }
                        if slot == 6 { entity_cmds.insert(WaterDrain); }

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
            break;
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
