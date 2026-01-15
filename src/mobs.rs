use bevy::prelude::*;
use crate::world::{VoxelWorld, VoxelAssets, BlockType, NeedsMeshUpdate, Particle, CHUNK_SIZE, update_voxel_map, WorldGen};
use crate::player::Player;
use crate::ui::{GameState, SnakeSettings};
use crate::world::Beehive;

#[derive(Component)]
pub struct SandSnake {
    pub move_timer: Timer,
    pub grow_timer: Timer,
    pub segments: Vec<Entity>,
    pub history: Vec<Vec3>,
}

#[derive(Component)]
pub struct Sheep {
    pub move_timer: Timer,
    pub target_dir: Vec3,
    pub velocity: Vec3,
}

#[derive(Component)]
pub struct Bee {
    pub home: IVec3,
    pub wander_target: Vec3,
    pub velocity: Vec3,
    pub change_dir_timer: Timer,
}

pub struct MobsPlugin;

impl Plugin for MobsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_snakes, sand_snake_ai, despawn_snakes_if_disabled, spawn_sheep, sheep_ai, spawn_bees, bee_ai).run_if(in_state(GameState::Playing)));
    }
}

fn spawn_snakes(
    mut commands: Commands,
    query: Query<&SandSnake>,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    snake_settings: Res<SnakeSettings>,
) {
    if !snake_settings.enabled { return; }
    if query.iter().count() < 3 {
        let pos = IVec3::new(0, 20, 0);
        let chunk_coord = IVec2::new(0, 0);
        
        if voxel_world.chunks.contains_key(&chunk_coord) && !voxel_world.blocks.contains_key(&pos) {
             let mut segments = Vec::new();
             let mut history = Vec::new();
             
             // Initialize history
             for i in 0..20 {
                 history.push(pos.as_vec3() + Vec3::new(0.0, -0.25, 0.0) + Vec3::Y * (i as f32 * 0.2));
             }
             
             // Spawn initial segments
             for i in 0..15 {
                 let seg_id = commands.spawn(PbrBundle {
                    mesh: voxel_assets.segment_mesh.clone(),
                    material: voxel_assets.snake_material.clone(),
                    transform: Transform::from_translation(history[i + 1]),
                    ..default()
                 }).id();
                 segments.push(seg_id);
             }

             let id = commands.spawn((
                PbrBundle {
                    mesh: voxel_assets.snake_mesh.clone(),
                    material: voxel_assets.snake_material.clone(),
                    transform: Transform::from_xyz(pos.x as f32, pos.y as f32 - 0.25, pos.z as f32),
                    ..default()
                },
                SandSnake { 
                    move_timer: Timer::from_seconds(0.2, TimerMode::Repeating),
                    grow_timer: Timer::from_seconds(5.0, TimerMode::Repeating),
                    segments,
                    history,
                },
                BlockType(9),
                NeedsMeshUpdate,
             )).with_children(|parent| {
                parent.spawn(PbrBundle {
                    mesh: voxel_assets.eye_mesh.clone(),
                    material: voxel_assets.eye_material.clone(),
                    transform: Transform::from_xyz(0.15, 0.25, -0.4),
                    ..default()
                });
                parent.spawn(PbrBundle {
                    mesh: voxel_assets.eye_mesh.clone(),
                    material: voxel_assets.eye_material.clone(),
                    transform: Transform::from_xyz(-0.15, 0.25, -0.4),
                    ..default()
                });
             }).id();
             voxel_world.blocks.insert(pos, id);
             voxel_world.chunks.get_mut(&chunk_coord).unwrap().push(pos);
        }
    }
}

fn spawn_bees(
    mut commands: Commands,
    mut query: Query<(&Transform, &mut Beehive)>,
    voxel_assets: Res<VoxelAssets>,
    time: Res<Time>,
) {
    for (transform, mut beehive) in query.iter_mut() {
        beehive.spawn_timer.tick(time.delta());
        if beehive.spawn_timer.finished() && beehive.spawn_count < 3 {
            beehive.spawn_count += 1;
            let pos = transform.translation + Vec3::new(0.0, -0.5, 0.0);
            
            commands.spawn((
                PbrBundle {
                    mesh: voxel_assets.bee_mesh.clone(),
                    material: voxel_assets.bee_material.clone(),
                    transform: Transform::from_translation(pos),
                    ..default()
                },
                Bee {
                    home: transform.translation.as_ivec3(),
                    wander_target: Vec3::ZERO,
                    velocity: Vec3::ZERO,
                    change_dir_timer: Timer::from_seconds(1.0, TimerMode::Repeating),
                }
            ));
        }
    }
}

fn bee_ai(
    mut query: Query<(&mut Transform, &mut Bee)>,
    mut player_query: Query<(&Transform, &mut crate::player::Health), (With<Player>, Without<Bee>)>,
    time: Res<Time>,
) {
    let (player_pos, mut player_health) = if let Ok((t, h)) = player_query.get_single_mut() {
        (Some(t.translation), Some(h))
    } else {
        (None, None)
    };

    for (mut transform, mut bee) in query.iter_mut() {
        bee.change_dir_timer.tick(time.delta());
        
        let mut target = bee.wander_target;
        let mut speed = 3.0;

        // Chase Logic
        if let Some(p_pos) = player_pos {
            let dist = transform.translation.distance(p_pos);
            if dist < 5.0 {
                target = p_pos + Vec3::new(0.0, 1.5, 0.0); // Target head
                speed = 6.0;

                if dist < 1.5 {
                    if let Some(ref mut health) = player_health {
                        if health.invulnerability_timer.finished() {
                            health.value -= 2;
                            health.invulnerability_timer.reset();
                        }
                    }
                }
            }
        }

        if bee.change_dir_timer.finished() {
            // Pick random target around home
            let seed = time.elapsed_seconds() + transform.translation.x;
            let r_x = (seed * 12.3).sin() * 3.0;
            let r_y = (seed * 45.6).cos() * 2.0;
            let r_z = (seed * 78.9).sin() * 3.0;
            
            bee.wander_target = bee.home.as_vec3() + Vec3::new(r_x, r_y, r_z);
        }
        
        // Move towards target
        let delta = target - transform.translation;
        let dist = delta.length();
        
        if dist > 0.1 {
            let dir = delta.normalize();
            bee.velocity = bee.velocity.lerp(dir * speed, time.delta_seconds() * 2.0);
            transform.translation += bee.velocity * time.delta_seconds();
            transform.look_to(bee.velocity, Vec3::Y);
        }
        
        // Bobbing
        transform.translation.y += (time.elapsed_seconds() * 10.0).sin() * 0.005;
    }
}

fn spawn_sheep(
    mut commands: Commands,
    query: Query<&Sheep>,
    player_query: Query<&Transform, With<Player>>,
    world_gen: Res<WorldGen>,
    voxel_world: Res<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    time: Res<Time>,
) {
    if query.iter().count() >= 10 { return; }

    if let Ok(player_transform) = player_query.get_single() {
        let player_pos = player_transform.translation;
        
        // Pseudo-random based on time
        let seed = time.elapsed_seconds();
        let r_x = (seed * 123.4).sin() * 50.0;
        let r_z = (seed * 567.8).cos() * 50.0;
        
        let x = (player_pos.x + r_x) as i32;
        let z = (player_pos.z + r_z) as i32;
        
        let (_, _, height, _) = crate::world::get_terrain_height(x, z, &world_gen.perlin);
        
        let pos = IVec3::new(x, height, z);
        // Check if chunk is generated (block exists)
        if voxel_world.blocks.contains_key(&pos) {
             // Check if space above is empty
             if !voxel_world.blocks.contains_key(&(pos + IVec3::Y)) {
                 commands.spawn((
                     PbrBundle {
                         mesh: voxel_assets.sheep_mesh.clone(),
                         material: voxel_assets.sheep_material.clone(),
                         transform: Transform::from_xyz(x as f32, height as f32 + 1.0, z as f32),
                         ..default()
                     },
                     Sheep {
                         move_timer: Timer::from_seconds(3.0, TimerMode::Repeating),
                         target_dir: Vec3::ZERO,
                         velocity: Vec3::ZERO,
                     }
                 )).with_children(|parent| {
                     // Head
                     parent.spawn(PbrBundle {
                         mesh: voxel_assets.sheep_head_mesh.clone(),
                         material: voxel_assets.sheep_material.clone(),
                         transform: Transform::from_xyz(0.0, 0.4, -0.4),
                         ..default()
                     });
                     // Legs
                     let leg_y = -0.5;
                     let leg_x = 0.2;
                     let leg_z = 0.35;
                     for (lx, lz) in [(leg_x, leg_z), (-leg_x, leg_z), (leg_x, -leg_z), (-leg_x, -leg_z)] {
                         parent.spawn(PbrBundle {
                             mesh: voxel_assets.sheep_leg_mesh.clone(),
                             material: voxel_assets.sheep_material.clone(),
                             transform: Transform::from_xyz(lx, leg_y, lz),
                             ..default()
                         });
                     }
                 });
             }
        }
    }
}

fn sheep_ai(
    mut query: Query<(&mut Transform, &mut Sheep)>,
    voxel_world: Res<VoxelWorld>,
    time: Res<Time>,
) {
    for (mut transform, mut sheep) in query.iter_mut() {
        sheep.move_timer.tick(time.delta());
        
        // Gravity
        sheep.velocity.y -= 20.0 * time.delta_seconds();
        
        if sheep.move_timer.finished() {
            // Pick new random direction
            let seed = time.elapsed_seconds() + transform.translation.x;
            let angle = (seed * 10.0).sin() * std::f32::consts::PI * 2.0;
            sheep.target_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            
            // Chance to stop
            if (seed * 20.0).cos() > 0.5 {
                sheep.target_dir = Vec3::ZERO;
            }
        }
        
        // Move
        if sheep.target_dir != Vec3::ZERO {
             let speed = 2.0;
             let movement = sheep.target_dir * speed * time.delta_seconds();
             let next_pos = transform.translation + movement;
             
             // Wall collision check
             let block_pos = IVec3::new(next_pos.x.round() as i32, next_pos.y.round() as i32, next_pos.z.round() as i32);
             if voxel_world.blocks.contains_key(&block_pos) {
                 // Try to jump
                 if !voxel_world.blocks.contains_key(&(block_pos + IVec3::Y)) && !voxel_world.blocks.contains_key(&(block_pos + IVec3::Y * 2)) {
                      // Only jump if on ground
                      let ground_check = IVec3::new(transform.translation.x.round() as i32, (transform.translation.y - 0.6).round() as i32, transform.translation.z.round() as i32);
                      if voxel_world.blocks.contains_key(&ground_check) {
                          sheep.velocity.y = 6.0;
                      }
                 } else {
                      sheep.target_dir = Vec3::ZERO; // Stop
                 }
             } else {
                 transform.translation.x = next_pos.x;
                 transform.translation.z = next_pos.z;
                 transform.look_to(sheep.target_dir, Vec3::Y);
             }
        }
        
        // Apply Y velocity
        transform.translation.y += sheep.velocity.y * time.delta_seconds();
        
        // Ground collision
        let feet_y = transform.translation.y - 0.7;
        let block_below = IVec3::new(transform.translation.x.round() as i32, (feet_y - 0.1).floor() as i32, transform.translation.z.round() as i32);
        
        if voxel_world.blocks.contains_key(&block_below) {
            transform.translation.y = block_below.y as f32 + 0.5 + 0.7;
            sheep.velocity.y = 0.0;
        }
    }
}

fn despawn_snakes_if_disabled(
    mut commands: Commands,
    snake_settings: Res<SnakeSettings>,
    query: Query<Entity, With<SandSnake>>,
) {
    if !snake_settings.enabled {
        for entity in query.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

fn sand_snake_ai(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    mut snake_query: Query<(Entity, &mut SandSnake), Without<Player>>,
    mut transform_query: Query<&mut Transform, Without<Player>>,
    block_type_query: Query<&BlockType>,
    player_query: Query<&Transform, With<Player>>,
    voxel_assets: Res<VoxelAssets>,
    block_material_query: Query<&Handle<StandardMaterial>>,
    time: Res<Time>,
) {
    let player_transform = player_query.get_single().ok();

    for (entity, mut snake) in snake_query.iter_mut() {
        snake.move_timer.tick(time.delta());
        snake.grow_timer.tick(time.delta());
        if !snake.move_timer.finished() { continue; }

        // We need to get the transform separately to avoid borrow conflicts with segments
        let mut transform = transform_query.get_mut(entity).unwrap();
        let pos = transform.translation.round().as_ivec3();
        let mut target_opt = None;
        
        // 1. Gravity
        let down = pos - IVec3::Y;
        if !voxel_world.blocks.contains_key(&down) {
             target_opt = Some(down);
        }

        if target_opt.is_none() {
        // 2. Move/Burrow
        let mut directions = vec![IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y];
        let mut tracking = false;

        if let Some(player_tf) = player_transform {
            if transform.translation.distance(player_tf.translation) < 10.0 {
                let player_pos = player_tf.translation.round().as_ivec3();
                // Sort directions by distance to player (ascending)
                directions.sort_by_key(|dir| {
                    let target = pos + *dir;
                    (target.x - player_pos.x).pow(2) + (target.y - player_pos.y).pow(2) + (target.z - player_pos.z).pow(2)
                });
                tracking = true;
            }
        }
        
        if !tracking {
            let seed = (time.elapsed_seconds() * 100.0 + pos.x as f32 + pos.y as f32) as usize;
            directions.rotate_left(seed % 6);
        }
        
        for dir in &directions {
            let target = pos + *dir;

            if let Some(&target_entity) = voxel_world.blocks.get(&target) {
                if let Ok(block_type) = block_type_query.get(target_entity) {
                    if block_type.0 == 7 { // Sand
                        target_opt = Some(target);
                        // Particles
                        if let Ok(mat_handle) = block_material_query.get(target_entity) {
                            for i in 0..5 {
                                let r1 = (target.x as f32 + i as f32 * 0.23).sin();
                                let r2 = (target.y as f32 + i as f32 * 0.45).cos();
                                let r3 = (target.z as f32 + i as f32 * 0.67).sin();
                                commands.spawn((
                                    PbrBundle {
                                        mesh: voxel_assets.mesh.clone(),
                                        material: mat_handle.clone(),
                                        transform: Transform::from_xyz(
                                            target.x as f32 + 0.5 + r1 * 0.2, 
                                            target.y as f32 + 0.5 + r2 * 0.2, 
                                            target.z as f32 + 0.5 + r3 * 0.2
                                        ).with_scale(Vec3::splat(0.1)),
                                        ..default()
                                    },
                                    Particle {
                                        lifetime: Timer::from_seconds(0.3, TimerMode::Once),
                                        velocity: Vec3::new(r1 * 2.0, r2.abs() * 3.0, r3 * 2.0),
                                    }
                                ));
                            }
                        }
                        break;
                    }
                }
            } else {
                let target_chunk = IVec2::new((target.x as f32 / CHUNK_SIZE as f32).floor() as i32, (target.z as f32 / CHUNK_SIZE as f32).floor() as i32);
                if voxel_world.chunks.contains_key(&target_chunk) {
                    target_opt = Some(target);
                    break;
                }
            }
        }
        }

        if let Some(target) = target_opt {
            // Rotate Head
            let target_vec = target.as_vec3() + Vec3::new(0.0, -0.25, 0.0);
            let up = if (target - pos).y == 0 { Vec3::Y } else { Vec3::X };
            transform.look_at(target_vec, up);

            // Move Head
            transform.translation = target_vec;
            
            // Update History
            snake.history.insert(0, target_vec);
            
            // Update Segments Visuals
            let spacing = 0.2;
            let mut prev_pos = snake.history[0];

            for (i, &seg_entity) in snake.segments.iter().enumerate() {
                if let Ok(mut seg_transform) = transform_query.get_mut(seg_entity) {
                    // Find position along spine
                    let dist = (i + 1) as f32 * spacing;
                    let mut current_dist = 0.0;
                    let mut seg_pos = snake.history[0];
                    
                    for j in 0..snake.history.len() - 1 {
                        let p1 = snake.history[j];
                        let p2 = snake.history[j+1];
                        let d = p1.distance(p2);
                        if current_dist + d >= dist {
                            let t = (dist - current_dist) / d;
                            seg_pos = p1.lerp(p2, t);
                            break;
                        }
                        current_dist += d;
                        seg_pos = p2; // Fallback
                    }
                    
                    seg_transform.translation = seg_pos;
                    // Look at previous point (towards head)
                    seg_transform.look_at(prev_pos, Vec3::Y);
                    prev_pos = seg_pos;
                }
            }

            // Handle Growth or Sand Displacement
            let growing = snake.grow_timer.finished();
            if growing {
                // Add new segment
                let seg_id = commands.spawn((
                    PbrBundle {
                        mesh: voxel_assets.segment_mesh.clone(),
                        material: voxel_assets.snake_material.clone(),
                        transform: Transform::from_translation(snake.history.last().copied().unwrap_or(target_vec)),
                        ..default()
                    },
                )).id();
                
                snake.segments.push(seg_id);
                snake.grow_timer.reset();

                // If we grew, we consumed the space. If target was sand, it's eaten (destroyed).
                if let Some(&target_entity) = voxel_world.blocks.get(&target) {
                    if target_entity != entity { // Don't despawn self
                        commands.entity(target_entity).despawn();
                        voxel_world.blocks.remove(&target); // Remove sand from map so head can take it
                    }
                }
            } else {
                // Not growing: Move sand to tail's position (end of history)
                // Trim history to reasonable length
                let needed_len = (snake.segments.len() as f32 * 0.2) + 2.0;
                let mut current_len = 0.0;
                let mut cut_idx = snake.history.len();
                for j in 0..snake.history.len() - 1 {
                    current_len += snake.history[j].distance(snake.history[j+1]);
                    if current_len > needed_len {
                        cut_idx = j + 2;
                        break;
                    }
                }
                if cut_idx < snake.history.len() {
                    snake.history.truncate(cut_idx);
                }

                let tail_pos = snake.history.last().unwrap().round().as_ivec3();

                if let Some(&target_entity) = voxel_world.blocks.get(&target) {
                     if target_entity != entity {
                        // Move sand block to tail position if empty
                        if !voxel_world.blocks.contains_key(&tail_pos) {
                            commands.entity(target_entity).insert(Transform::from_xyz(tail_pos.x as f32, tail_pos.y as f32, tail_pos.z as f32));
                            update_voxel_map(&mut commands, &mut voxel_world, target, tail_pos, target_entity);
                        } else {
                            // Tail blocked? Just destroy sand (eaten)
                            commands.entity(target_entity).despawn();
                            voxel_world.blocks.remove(&target);
                        }
                     }
                }
            }

            // Finally update head in map
            update_voxel_map(&mut commands, &mut voxel_world, pos, target, entity);
        }
    }
}