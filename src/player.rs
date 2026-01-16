use bevy::prelude::*;
use bevy::input::mouse::MouseMotion;
use crate::world::{VoxelWorld, VoxelAssets, VoxelSounds, BlockType, Liquid, WaterSource, WaterDrain, NeedsMeshUpdate, Particle, CHUNK_SIZE};
use bevy::audio::Volume;
use crate::mobs::SandSnake;
use crate::ui::Inventory;
use crate::ui::GameState;
use rand::Rng;

#[derive(Component)]
pub struct Player {
    pub velocity: Vec3,
    pub flying: bool,
    pub footstep_timer: f32,
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
           .add_systems(Update, (move_player, interact_terrain, check_snake_collision).run_if(in_state(GameState::Playing)));
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
        Player { velocity: Vec3::ZERO, flying: false, footstep_timer: 0.0 },
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
                    start: 25.0,
                    end: 55.0,
                },
                ..default()
            },
            // SSAO disabled for better performance
        ));
    });
}

fn move_player(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &mut Player, &mut Health)>,
    mut camera_query: Query<(&mut Transform, &GlobalTransform), (With<MainCamera>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut windows: Query<&mut Window>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    voxel_world: Res<VoxelWorld>,
    voxel_sounds: Res<VoxelSounds>,
    block_type_query: Query<&BlockType>,
    world_gen: Res<crate::world::WorldGen>,
) {
    let mut window = windows.single_mut();

    // Lock cursor on click for FPS control
    if mouse_buttons.just_pressed(MouseButton::Left) {
        window.cursor.visible = false;
        window.cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
    }

    let (player_entity, mut transform, mut player, mut health) = query.single_mut();

    // Mouse Look
    if window.cursor.grab_mode == bevy::window::CursorGrabMode::Locked {
        let sensitivity = 0.003;
        for event in mouse_motion.read() {
            transform.rotate_y(-event.delta.x * sensitivity);
            if let Ok((mut camera_transform, _)) = camera_query.get_single_mut() {
                // Clamp vertical rotation to prevent looking past straight up/down
                let mut rotation_x = camera_transform.rotation.to_euler(EulerRot::XYZ).0;
                rotation_x -= event.delta.y * sensitivity;

                // Limit vertical look to 90 degrees up and down (PI/2 radians)
                rotation_x = rotation_x.clamp(-std::f32::consts::FRAC_PI_2 + 0.01, std::f32::consts::FRAC_PI_2 - 0.01);

                // Apply the clamped rotation
                camera_transform.rotation = Quat::from_rotation_x(rotation_x);
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
        let move_delta = direction * speed * time.delta_seconds();
        let new_pos = transform.translation + move_delta;

        // Horizontal collision detection - check at feet and head level
        let player_radius = 0.4;
        let feet_y = (transform.translation.y - 0.5).round() as i32;
        let head_y = (transform.translation.y + 0.5).round() as i32;

        let mut can_move_x = true;
        let mut can_move_z = true;

        // Check collision in X direction
        let check_x = (new_pos.x + player_radius * direction.x.signum()).round() as i32;
        let check_z_current = transform.translation.z.round() as i32;
        for y in feet_y..=head_y {
            let check_pos = IVec3::new(check_x, y, check_z_current);
            if let Some(&entity) = voxel_world.blocks.get(&check_pos) {
                if let Ok(block_type) = block_type_query.get(entity) {
                    if block_type.0 != 4 { // Not water
                        can_move_x = false;
                        break;
                    }
                }
            }
        }

        // Check collision in Z direction
        let check_x_current = transform.translation.x.round() as i32;
        let check_z = (new_pos.z + player_radius * direction.z.signum()).round() as i32;
        for y in feet_y..=head_y {
            let check_pos = IVec3::new(check_x_current, y, check_z);
            if let Some(&entity) = voxel_world.blocks.get(&check_pos) {
                if let Ok(block_type) = block_type_query.get(entity) {
                    if block_type.0 != 4 { // Not water
                        can_move_z = false;
                        break;
                    }
                }
            }
        }

        // Apply movement with collision response
        let moved = can_move_x || can_move_z;
        if can_move_x {
            transform.translation.x = new_pos.x;
        }
        if can_move_z {
            transform.translation.z = new_pos.z;
        }

        // Footstep sounds when walking on ground
        if moved && !player.flying && player.velocity.y.abs() < 0.1 {
            player.footstep_timer += time.delta_seconds();
            let footstep_interval = if keys.pressed(KeyCode::ShiftLeft) { 0.25 } else { 0.4 };
            if player.footstep_timer >= footstep_interval {
                player.footstep_timer = 0.0;
                commands.spawn(AudioBundle {
                    source: voxel_sounds.footstep.clone(),
                    settings: PlaybackSettings::DESPAWN.with_volume(Volume::new(1.0)),
                });
            }
        }
    } else {
        player.footstep_timer = 0.0;
    }

    // Toggle Fly Mode
    if keys.just_pressed(KeyCode::KeyF) {
        player.flying = !player.flying;
        player.velocity = Vec3::ZERO;
    }

    // Physics / Gravity Logic
    let x = transform.translation.x.round() as i32;
    let z = transform.translation.z.round() as i32;
    let feet_level = (transform.translation.y - 1.0).round() as i32;

    // Scan for ground ONLY below player (not above - prevents auto-jump)
    let mut ground_y = -50.0; // Default abyss
    for y in (feet_level - 20..=feet_level).rev() {
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
                // Jump height: v^2/(2g) = 9.5^2/(2*30) ≈ 1.5 blocks
                player.velocity.y = 9.5;
            }
        }
    }

    // Respawn if fell out of world
    if transform.translation.y < -30.0 {
        // If player is still alive, respawn at a random location
        let mut rng = rand::thread_rng();
        let rand_x = (rng.gen_range(0.0..1.0) - 0.5) * 100.0; // Random between -50 and 50
        let rand_z = (rng.gen_range(0.0..1.0) - 0.5) * 100.0; // Random between -50 and 50

        // Calculate spawn height based on terrain generation at the random location
        let (_, _, height) = crate::world::get_terrain_height(rand_x as i32, rand_z as i32, &world_gen.perlin);

        transform.translation = Vec3::new(rand_x, height as f32 + 5.0, rand_z);

        // Reset health when respawning due to falling
        health.value = 100;
        health.invulnerability_timer = Timer::from_seconds(2.0, TimerMode::Once);
    }

    // Camera collision detection - prevent camera from clipping into blocks
    if let Ok((_, camera_global)) = camera_query.get_single() {
        let camera_pos = camera_global.translation();
        let camera_block_pos = camera_pos.round().as_ivec3();

        // Check if camera is inside a solid block
        if let Some(&entity) = voxel_world.blocks.get(&camera_block_pos) {
            if let Ok(block_type) = block_type_query.get(entity) {
                // Only push back for solid blocks (not water/special blocks)
                if block_type.0 != 4 && block_type.0 != 9 && block_type.0 != 10 {
                    // Calculate push direction - move player away from block center
                    let block_center = camera_block_pos.as_vec3();
                    let offset = camera_pos - block_center;

                    // Find the smallest axis to push out on
                    let abs_offset = offset.abs();
                    let push_dist = 0.6; // Slightly more than half a block

                    if abs_offset.x <= abs_offset.y && abs_offset.x <= abs_offset.z {
                        // Push on X axis
                        let sign = if offset.x >= 0.0 { 1.0 } else { -1.0 };
                        transform.translation.x = block_center.x + sign * push_dist;
                    } else if abs_offset.y <= abs_offset.z {
                        // Push on Y axis
                        let sign = if offset.y >= 0.0 { 1.0 } else { -1.0 };
                        transform.translation.y = block_center.y + sign * push_dist;
                    } else {
                        // Push on Z axis
                        let sign = if offset.z >= 0.0 { 1.0 } else { -1.0 };
                        transform.translation.z = block_center.z + sign * push_dist;
                    }
                }
            }
        }
    }

    // Check if player died (health <= 0) and trigger respawn if needed
    if health.value <= 0 {
        respawn_player(&mut commands, player_entity, &world_gen);
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
    mut block_durability_query: Query<&mut crate::world::BlockDurability>,
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
                if let Some(&entity) = voxel_world.blocks.get(&block_pos) {
                    // Check if the block has durability component
                    if let Ok(mut durability) = block_durability_query.get_mut(entity) {
                        // Only process blocks that have durability (not water, sources, etc.)
                        if durability.max_durability > 0.0 {
                            // Damage the block
                            durability.current_durability -= 1.0; // Each hit does 1.0 damage

                            // Calculate crack level based on remaining durability
                            let crack_percentage = durability.current_durability / durability.max_durability;
                            let new_crack_level = if crack_percentage < 0.25 {
                                3  // Very damaged
                            } else if crack_percentage < 0.5 {
                                2  // Moderately damaged
                            } else if crack_percentage < 0.75 {
                                1  // Slightly damaged
                            } else {
                                0  // No cracks
                            };

                            // Only update if crack level changed
                            if durability.crack_level != new_crack_level {
                                durability.crack_level = new_crack_level;
                                // Add mesh update to refresh visual appearance
                                commands.entity(entity).insert(NeedsMeshUpdate);
                            }

                            // Play hit sound
                            commands.spawn(AudioBundle {
                                source: voxel_sounds.break_sound.clone(),
                                settings: PlaybackSettings::DESPAWN.with_volume(Volume::new(0.3)), // Quieter hit sound
                            });

                            // Check if block should be destroyed
                            if durability.current_durability <= 0.0 {
                                // Remove the block completely
                                if let Some(entity) = voxel_world.blocks.remove(&block_pos) {
                                    // Check block type for inventory addition
                                    if let Ok(block_type) = block_type_query.get(entity) {
                                        // Don't add water, sources, or drains to inventory
                                        if ![4, 5, 6].contains(&block_type.0) {
                                            *inventory.items.entry(block_type.0).or_insert(0) += 1;
                                        }
                                    }

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

                                    commands.entity(entity).despawn_recursive();
                                    commands.spawn(AudioBundle {
                                        source: voxel_sounds.break_sound.clone(),
                                        settings: PlaybackSettings::DESPAWN.with_volume(Volume::new(1.0)), // Louder break sound
                                    });

                                    // Update neighbors (safely handle despawned entities)
                                    // When a block is removed, all 6 neighboring blocks need to update their meshes to show newly exposed faces
                                    for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                        let neighbor_pos = block_pos + dir;
                                        if let Some(&e) = voxel_world.blocks.get(&neighbor_pos) {
                                            if let Some(mut ec) = commands.get_entity(e) {
                                                ec.insert(NeedsMeshUpdate);
                                            }
                                        }
                                    }

                                    // Additionally, update neighbors of the removed block's neighbors to handle edge cases
                                    for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                        let neighbor_pos = block_pos + dir;
                                        if let Some(&neighbor_entity) = voxel_world.blocks.get(&neighbor_pos) {
                                            // Update the neighbor itself (already done above, but let's make sure)
                                            if let Some(mut ec) = commands.get_entity(neighbor_entity) {
                                                ec.insert(NeedsMeshUpdate);
                                            }

                                            // Update the neighbor's neighbors to handle edge cases where they might also need updates
                                            for neighbor_dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                                let neighbor_neighbor_pos = neighbor_pos + neighbor_dir;
                                                if let Some(&nn_entity) = voxel_world.blocks.get(&neighbor_neighbor_pos) {
                                                    if let Some(mut ec) = commands.get_entity(nn_entity) {
                                                        ec.insert(NeedsMeshUpdate);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Update the chunk that contained the removed block to ensure proper mesh updates
                                    let chunk_coord = IVec2::new(
                                        (block_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                                        (block_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                                    );
                                    if let Some(chunk_entities) = voxel_world.chunks.get(&chunk_coord) {
                                        for &pos_in_chunk in chunk_entities {
                                            if let Some(&entity) = voxel_world.blocks.get(&pos_in_chunk) {
                                                if let Some(mut ec) = commands.get_entity(entity) {
                                                    ec.insert(NeedsMeshUpdate);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // For blocks without durability (water, etc.), handle normally
                        if let Ok(block_type) = block_type_query.get(entity) {
                            if block_type.0 == 8 { continue; } // Skip bedrock
                            // Add to inventory
                            *inventory.items.entry(block_type.0).or_insert(0) += 1;
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

                            commands.entity(entity).despawn_recursive();
                            commands.spawn(AudioBundle {
                                source: voxel_sounds.break_sound.clone(),
                                settings: PlaybackSettings::DESPAWN.with_volume(Volume::new(1.0)),
                            });

                            // Update neighbors (safely handle despawned entities)
                            // When a block is removed, all 6 neighboring blocks need to update their meshes to show newly exposed faces
                            for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                let neighbor_pos = block_pos + dir;
                                if let Some(&e) = voxel_world.blocks.get(&neighbor_pos) {
                                    if let Some(mut ec) = commands.get_entity(e) {
                                        ec.insert(NeedsMeshUpdate);
                                    }
                                }
                            }

                            // Additionally, update neighbors of the removed block's neighbors to handle edge cases
                            for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                let neighbor_pos = block_pos + dir;
                                if let Some(&neighbor_entity) = voxel_world.blocks.get(&neighbor_pos) {
                                    // Update the neighbor itself (already done above, but let's make sure)
                                    if let Some(mut ec) = commands.get_entity(neighbor_entity) {
                                        ec.insert(NeedsMeshUpdate);
                                    }

                                    // Update the neighbor's neighbors to handle edge cases where they might also need updates
                                    for neighbor_dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                                        let neighbor_neighbor_pos = neighbor_pos + neighbor_dir;
                                        if let Some(&nn_entity) = voxel_world.blocks.get(&neighbor_neighbor_pos) {
                                            if let Some(mut ec) = commands.get_entity(nn_entity) {
                                                ec.insert(NeedsMeshUpdate);
                                            }
                                        }
                                    }
                                }
                            }

                            // Update the chunk that contained the removed block to ensure proper mesh updates
                            let chunk_coord = IVec2::new(
                                (block_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                                (block_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                            );
                            if let Some(chunk_entities) = voxel_world.chunks.get(&chunk_coord) {
                                for &pos_in_chunk in chunk_entities {
                                    if let Some(&entity) = voxel_world.blocks.get(&pos_in_chunk) {
                                        if let Some(mut ec) = commands.get_entity(entity) {
                                            ec.insert(NeedsMeshUpdate);
                                        }
                                    }
                                }
                            }
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
                            NeedsMeshUpdate,
                        ));

                        // Add wireframe child for outline (skip for water)
                        if slot != 4 {
                            entity_cmds.with_children(|parent| {
                                parent.spawn(PbrBundle {
                                    mesh: voxel_assets.wireframe_mesh.clone(),
                                    material: voxel_assets.wireframe_material.clone(),
                                    ..default()
                                });
                            });
                        }

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
                            settings: PlaybackSettings::DESPAWN.with_volume(Volume::new(1.0)),
                        });
                        
                        // Update neighbors (safely handle despawned entities)
                        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                            if let Some(&e) = voxel_world.blocks.get(&(prev_pos + dir)) {
                                if let Some(mut ec) = commands.get_entity(e) {
                                    ec.insert(NeedsMeshUpdate);
                                }
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
    mut commands: Commands,
    mut player_query: Query<(Entity, &Transform, &mut Health, &mut Player)>,
    snake_query: Query<&Transform, With<SandSnake>>,
    time: Res<Time>,
    world_gen: Res<crate::world::WorldGen>,
) {
    if let Ok((player_entity, player_transform, mut health, mut player)) = player_query.get_single_mut() {
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

                // Check if player died
                if health.value <= 0 {
                    info!("Player died! Respawning...");
                    respawn_player(&mut commands, player_entity, &world_gen);
                }
            }
        }
    }
}

// Function to handle player respawn
fn respawn_player(
    commands: &mut Commands,
    player_entity: Entity,
    world_gen: &crate::world::WorldGen,
) {
    // Calculate a random spawn location
    let mut rng = rand::thread_rng();
    let rand_x = (rng.gen_range(0.0..1.0) - 0.5) * 100.0; // Random between -50 and 50
    let rand_z = (rng.gen_range(0.0..1.0) - 0.5) * 100.0; // Random between -50 and 50

    // Calculate spawn height based on terrain generation at the random location
    let (_, _, height) = crate::world::get_terrain_height(rand_x as i32, rand_z as i32, &world_gen.perlin);

    // Reset player position and health
    commands.entity(player_entity).insert((
        Transform::from_xyz(rand_x, height as f32 + 5.0, rand_z),
        Health { value: 100, invulnerability_timer: Timer::from_seconds(2.0, TimerMode::Once) },
        Player { velocity: Vec3::ZERO, flying: false, footstep_timer: 0.0 },
    ));
}
