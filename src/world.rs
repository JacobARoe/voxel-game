use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::BufReader;
use noise::{NoiseFn, Perlin};
use bevy::render::{mesh::PrimitiveTopology, render_asset::RenderAssetUsages};
use crate::player::Player;
use crate::ui::GameState;

pub const CHUNK_SIZE: i32 = 16;

#[derive(Component)]
pub struct Particle {
    pub lifetime: Timer,
    pub velocity: Vec3,
}

#[derive(Component)]
pub struct DroppedItem {
    pub block_type: usize,
    pub velocity: Vec3,
}

#[derive(Component)]
pub struct BlockType(pub usize);

#[derive(Component)]
pub struct Liquid {
    pub level: u8, // 1 to 9
}

#[derive(Component)]
pub struct WaterSource;

#[derive(Component)]
pub struct WaterDrain;

#[derive(Component)]
pub struct NeedsMeshUpdate;

#[derive(Component)]
pub struct Furnace {
    pub timer: Timer,
    pub input: Option<usize>,
    pub output: Option<usize>,
}

#[derive(Component)]
pub struct Cloud {
    pub speed: f32,
}

#[derive(Serialize, Deserialize)]
struct SavedBlock {
    x: i32,
    y: i32,
    z: i32,
    type_index: usize,
    #[serde(default)]
    level: u8,
}

#[derive(Resource)]
pub struct VoxelAssets {
    pub mesh: Handle<Mesh>,
    pub water_meshes: Vec<Handle<Mesh>>,
    pub faces_meshes: Vec<Handle<Mesh>>,
    pub _material: Handle<StandardMaterial>,
    pub block_types: Vec<Handle<StandardMaterial>>,
    pub block_names: Vec<String>,
    pub snake_material: Handle<StandardMaterial>,
    pub snake_mesh: Handle<Mesh>,
    pub eye_mesh: Handle<Mesh>,
    pub eye_material: Handle<StandardMaterial>,
    pub segment_mesh: Handle<Mesh>,
    pub torch_mesh: Handle<Mesh>,
    pub crack_materials: Vec<Handle<StandardMaterial>>,
    pub cloud_material: Handle<StandardMaterial>,
    pub chimney_mesh: Handle<Mesh>,
}

#[derive(Resource)]
pub struct VoxelSounds {
    pub place: Handle<AudioSource>,
    pub break_sound: Handle<AudioSource>,
}

#[derive(Resource, Default)]
pub struct VoxelWorld {
    pub blocks: HashMap<IVec3, Entity>,
    pub chunks: HashMap<IVec2, Vec<IVec3>>,
    pub generated_chunks: HashSet<IVec2>,
}

#[derive(Resource)]
pub struct WorldGen {
    pub _seed: u32,
    pub perlin: Perlin,
}

#[derive(Resource)]
pub struct WorldTime {
    pub time: f32,
    pub speed: f32,
}

impl Default for WorldTime {
    fn default() -> Self {
        Self { time: 0.2, speed: 0.02 } // Start near noon
    }
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VoxelWorld::default())
           .insert_resource(WorldGen { _seed: 42, perlin: Perlin::new(42) })
           .insert_resource(WorldTime::default())
           .add_systems(Startup, (setup_world, spawn_clouds).chain())
           .add_systems(Update, (
               update_chunks,
               update_particles,
               update_dropped_items,
               save_load_world,
               water_dynamics,
               update_water_level,
               water_source_system,
               water_drain_system,
               sand_dynamics,
               furnace_system,
               day_night_cycle,
               update_clouds,
           ).run_if(in_state(GameState::Playing)))
           .add_systems(PostUpdate, update_mesh_system);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Biome {
    Plains,
    Desert,
    Snow,
    Mountain,
}

pub fn get_terrain_height(x: i32, z: i32, perlin: &Perlin) -> (i32, i32, i32, Biome) {
    // 1. Biome Determination
    let temp_noise = perlin.get([x as f64 * 0.002, z as f64 * 0.002]);
    let humidity_noise = perlin.get([x as f64 * 0.002 + 100.0, z as f64 * 0.002 + 100.0]);
    
    let mut biome = Biome::Plains;
    if temp_noise > 0.2 && humidity_noise < 0.0 {
        biome = Biome::Desert;
    } else if temp_noise < -0.2 {
        biome = Biome::Snow;
    }

    // 2. Height Generation
    let mut height = 0.0;
    
    // Base terrain
    height += perlin.get([x as f64 * 0.01, z as f64 * 0.01]) * 5.0;
    height += perlin.get([x as f64 * 0.05, z as f64 * 0.05]) * 2.0;
    
    // Mountain influence
    let mountain_noise = perlin.get([x as f64 * 0.005 + 500.0, z as f64 * 0.005 + 500.0]);
    if mountain_noise > 0.2 {
        biome = Biome::Mountain;
        let intensity = (mountain_noise - 0.2) * 1.25;
        height += intensity * 40.0;
    }
    
    // River influence
    let river_noise = perlin.get([x as f64 * 0.005 + 200.0, z as f64 * 0.005 + 200.0]).abs();
    if river_noise < 0.05 {
        let depth = (0.05 - river_noise) * 20.0;
        height -= depth * 15.0;
    }

    let h = height.round() as i32;
    let dirt_thickness = if biome == Biome::Mountain { 1 } else { 3 };
    let stone_h = (h - dirt_thickness).max(-16) + 16;

    (stone_h, dirt_thickness, h, biome)
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Spawn a light
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            shadows_enabled: true,
            illuminance: 10000.0,
            ..default()
        },
        transform: Transform::from_xyz(50.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });

    // Create shared resources
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    
    // Generate 64 meshes for all face combinations (Greedy-ish meshing per block)
    // Bitmask: 1:+X, 2:-X, 4:+Y, 8:-Y, 16:+Z, 32:-Z
    let mut faces_meshes = Vec::new();
    for i in 0..64 {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        let mut v_idx = 0;

        let add_face = |pos: &mut Vec<[f32; 3]>, norm: &mut Vec<[f32; 3]>, uv: &mut Vec<[f32; 2]>, ind: &mut Vec<u32>, v: &mut u32, corners: [[f32; 3]; 4], normal: [f32; 3]| {
            pos.extend_from_slice(&corners);
            norm.extend_from_slice(&[normal; 4]);
            uv.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
            ind.extend_from_slice(&[*v, *v+1, *v+2, *v+2, *v+3, *v]);
            *v += 4;
        };

        // +X (Right)
        if (i & 1) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[0.5, 0.5, 0.5], [0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5]], [1.0, 0.0, 0.0]); }
        // -X (Left)
        if (i & 2) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-0.5, 0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]], [-1.0, 0.0, 0.0]); }
        // +Y (Top)
        if (i & 4) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]], [0.0, 1.0, 0.0]); }
        // -Y (Bottom)
        if (i & 8) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]], [0.0, -1.0, 0.0]); }
        // +Z (Back)
        if (i & 16) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[0.5, 0.5, 0.5], [-0.5, 0.5, 0.5], [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5]], [0.0, 0.0, 1.0]); }
        // -Z (Front)
        if (i & 32) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-0.5, 0.5, -0.5], [0.5, 0.5, -0.5], [0.5, -0.5, -0.5], [-0.5, -0.5, -0.5]], [0.0, 0.0, -1.0]); }

        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
        faces_meshes.push(meshes.add(mesh));
    }

    let mut water_meshes = Vec::new();
    for i in 0..9 {
        let height = (i as f32 + 1.0) / 9.0;
        water_meshes.push(meshes.add(Cuboid::new(1.0, height, 1.0)));
    }
    let grass = materials.add(Color::srgb(0.3, 0.8, 0.3));
    let dirt = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let stone = materials.add(Color::srgb(0.5, 0.5, 0.5));
    let wood = materials.add(Color::srgb(0.4, 0.2, 0.1));
    let sand = materials.add(Color::srgb(0.9, 0.8, 0.5));
    let water = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.4, 0.8, 0.5),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let source = materials.add(Color::srgb(0.0, 1.0, 1.0));
    let drain = materials.add(Color::srgb(0.2, 0.0, 0.0));
    let bedrock = materials.add(Color::srgb(0.1, 0.1, 0.1));
    let snake_mat = materials.add(Color::srgb(0.2, 0.8, 0.2));
    let snake_mesh = meshes.add(Cuboid::new(0.5, 0.5, 0.9));
    let eye_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.05));
    let segment_mesh = meshes.add(Cuboid::new(0.4, 0.4, 0.4));
    let torch_mesh = meshes.add(Cuboid::new(0.2, 0.6, 0.2));
    let eye_mat = materials.add(Color::BLACK);
    let leaves = materials.add(Color::srgb(0.2, 0.6, 0.2));
    let snow = materials.add(Color::WHITE);
    let ice = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.9, 1.0, 0.7),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let torch_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.2),
        emissive: LinearRgba::new(1.0, 0.8, 0.2, 1.0) * 10.0,
        ..default()
    });
    let iron_ore = materials.add(Color::srgb(0.6, 0.4, 0.3));
    let copper_ore = materials.add(Color::srgb(0.8, 0.5, 0.3));
    let silver_ore = materials.add(Color::srgb(0.9, 0.9, 1.0));
    let gold_ore = materials.add(Color::srgb(1.0, 0.8, 0.0));
    let furnace_mat = materials.add(Color::srgb(0.2, 0.2, 0.2));
    let iron_ingot = materials.add(Color::srgb(0.7, 0.7, 0.7));
    let copper_ingot = materials.add(Color::srgb(0.8, 0.4, 0.2));
    let silver_ingot = materials.add(Color::srgb(0.95, 0.95, 1.0));
    let gold_ingot = materials.add(Color::srgb(1.0, 0.9, 0.0));
    let pickaxe = materials.add(Color::srgb(0.4, 0.4, 0.5));
    let axe = materials.add(Color::srgb(0.6, 0.3, 0.1));
    let shovel = materials.add(Color::srgb(0.7, 0.7, 0.7));
    let chimney_mesh = meshes.add(Cuboid::new(0.4, 0.4, 0.4));
    
    let mut crack_materials = Vec::new();
    for i in 0..10 {
        crack_materials.push(materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, (i as f32 + 1.0) / 10.0 * 0.7),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }));
    }

    let cloud_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    
    commands.insert_resource(VoxelAssets { 
        mesh, 
        water_meshes,
        faces_meshes,
        _material: grass.clone(), 
        block_types: vec![grass, dirt, stone, wood, water, source, drain, sand, bedrock, snake_mat.clone(), snake_mat.clone(), leaves, snow, ice, torch_mat, iron_ore, copper_ore, silver_ore, gold_ore, furnace_mat, iron_ingot, copper_ingot, silver_ingot, gold_ingot, pickaxe, axe, shovel],
        block_names: vec!["Grass".to_string(), "Dirt".to_string(), "Stone".to_string(), "Wood".to_string(), "Water".to_string(), "Water Source".to_string(), "Water Drain".to_string(), "Sand".to_string(), "Bedrock".to_string(), "Snake".to_string(), "Snake Segment".to_string(), "Leaves".to_string(), "Snow".to_string(), "Ice".to_string(), "Torch".to_string(), "Iron Ore".to_string(), "Copper Ore".to_string(), "Silver Ore".to_string(), "Gold Ore".to_string(), "Furnace".to_string(), "Iron Ingot".to_string(), "Copper Ingot".to_string(), "Silver Ingot".to_string(), "Gold Ingot".to_string(), "Pickaxe".to_string(), "Axe".to_string(), "Shovel".to_string()],
        snake_material: snake_mat,
        snake_mesh,
        eye_mesh,
        eye_material: eye_mat,
        segment_mesh,
        torch_mesh,
        crack_materials,
        cloud_material,
        chimney_mesh,
    });

    commands.insert_resource(VoxelSounds {
        place: asset_server.load("sounds/place.mp3"),
        break_sound: asset_server.load("sounds/break.mp3"),
    });
}

fn update_particles(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &mut Particle)>,
    time: Res<Time>,
) {
    for (entity, mut transform, mut particle) in query.iter_mut() {
        particle.lifetime.tick(time.delta());
        if particle.lifetime.finished() {
            commands.entity(entity).despawn();
        } else {
            particle.velocity.y -= 25.0 * time.delta_seconds();
            transform.translation += particle.velocity * time.delta_seconds();
            transform.rotate_local_x(10.0 * time.delta_seconds());
            transform.rotate_local_y(10.0 * time.delta_seconds());
        }
    }
}

fn spawn_clouds(mut commands: Commands, voxel_assets: Res<VoxelAssets>) {
    for i in 0..50 {
        let x = (i as f32 * 132.0) % 400.0 - 200.0;
        let z = (i as f32 * 317.0) % 400.0 - 200.0;
        let y = 80.0 + (i as f32 % 5.0) * 5.0;
        let scale_x = 10.0 + (i as f32 % 4.0) * 5.0;
        let scale_z = 10.0 + ((i + 2) as f32 % 4.0) * 5.0;
        
        commands.spawn((
            PbrBundle {
                mesh: voxel_assets.mesh.clone(),
                material: voxel_assets.cloud_material.clone(),
                transform: Transform::from_xyz(x, y, z).with_scale(Vec3::new(scale_x, 2.0, scale_z)),
                ..default()
            },
            Cloud { speed: 2.0 + (i as f32 % 3.0) },
        ));
    }
}

fn update_clouds(
    mut query: Query<(&mut Transform, &Cloud)>,
    time: Res<Time>,
    player_query: Query<&Transform, (With<Player>, Without<Cloud>)>,
) {
    let player_pos = if let Ok(t) = player_query.get_single() { t.translation } else { Vec3::ZERO };
    
    for (mut transform, cloud) in query.iter_mut() {
        transform.translation.x += cloud.speed * time.delta_seconds();
        
        // Wrap around player logic
        let range = 200.0;
        if transform.translation.x > player_pos.x + range {
            transform.translation.x -= range * 2.0;
        }
        if transform.translation.x < player_pos.x - range {
            transform.translation.x += range * 2.0;
        }
        if transform.translation.z > player_pos.z + range {
            transform.translation.z -= range * 2.0;
        }
        if transform.translation.z < player_pos.z - range {
            transform.translation.z += range * 2.0;
        }
    }
}

fn update_dropped_items(
    mut query: Query<(&mut Transform, &mut DroppedItem)>,
    voxel_world: Res<VoxelWorld>,
    time: Res<Time>,
) {
    for (mut transform, mut item) in query.iter_mut() {
        item.velocity.y -= 20.0 * time.delta_seconds();
        let next_pos = transform.translation + item.velocity * time.delta_seconds();
        
        let check_pos = IVec3::new(next_pos.x.round() as i32, (next_pos.y - 0.25).round() as i32, next_pos.z.round() as i32);
        
        if voxel_world.blocks.contains_key(&check_pos) {
            item.velocity = Vec3::ZERO;
            transform.translation.y = check_pos.y as f32 + 0.5 + 0.2;
        } else {
            transform.translation = next_pos;
        }
        transform.rotate_y(2.0 * time.delta_seconds());
    }
}

pub fn update_voxel_map(commands: &mut Commands, voxel_world: &mut VoxelWorld, old_pos: IVec3, new_pos: IVec3, entity: Entity) {
    voxel_world.blocks.remove(&old_pos);
    
    voxel_world.blocks.insert(new_pos, entity);
    commands.entity(entity).insert(NeedsMeshUpdate);
    
    let old_chunk = IVec2::new((old_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32, (old_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32);
    
    // Tag neighbors for update
    for pos in [old_pos, new_pos] {
        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            if let Some(&e) = voxel_world.blocks.get(&(pos + dir)) {
                commands.entity(e).insert(NeedsMeshUpdate);
            }
        }
    }

    let new_chunk = IVec2::new((new_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32, (new_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32);
    
    if old_chunk != new_chunk {
        if let Some(chunk) = voxel_world.chunks.get_mut(&old_chunk) {
            if let Some(idx) = chunk.iter().position(|&p| p == old_pos) { chunk.remove(idx); }
        }
        voxel_world.chunks.entry(new_chunk).or_default().push(new_pos);
    } else if let Some(chunk) = voxel_world.chunks.get_mut(&old_chunk) {
        if let Some(idx) = chunk.iter().position(|&p| p == old_pos) { chunk[idx] = new_pos; }
    }
}

fn update_chunks(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    world_gen: Res<WorldGen>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player_transform = player_query.single();
    let render_distance = 2;

    // Calculate the chunk the player is currently in
    let player_chunk = IVec2::new(
        (player_transform.translation.x / CHUNK_SIZE as f32).floor() as i32,
        (player_transform.translation.z / CHUNK_SIZE as f32).floor() as i32,
    );

    // Spawn chunks around player
    for x in -render_distance..=render_distance {
        for z in -render_distance..=render_distance {
            let chunk_coord = player_chunk + IVec2::new(x, z);
            
            if !voxel_world.generated_chunks.contains(&chunk_coord) {
                voxel_world.generated_chunks.insert(chunk_coord);
                let mut chunk_blocks = Vec::new();
                
                for bx in 0..CHUNK_SIZE {
                    for bz in 0..CHUNK_SIZE {
                        let world_x = chunk_coord.x * CHUNK_SIZE + bx;
                        let world_z = chunk_coord.y * CHUNK_SIZE + bz;
                        
                        // Layered generation
                        let (stone_h, _, height, biome) = get_terrain_height(world_x, world_z, &world_gen.perlin);
                        let water_level = -8;
                        
                        // Generate column from Bedrock up to max(height, water_level)
                        for y in -16..=std::cmp::max(height, water_level) {
                            let pos = IVec3::new(world_x, y, world_z);
                            
                            let mut is_water = false;

                            // Cave Generation
                            if y <= height && y > -16 {
                                let cave_scale = 0.05;
                                let cave_noise = world_gen.perlin.get([world_x as f64 * cave_scale, y as f64 * cave_scale, world_z as f64 * cave_scale + 400.0]);
                                let threshold = if biome == Biome::Mountain { 0.2 } else { 0.4 };
                                if cave_noise > threshold {
                                    continue;
                                }
                            }

                            // Determine Block Type
                            let mut block_type_idx = if y <= height {
                                if y == -16 {
                                    8 // Bedrock
                                } else if y <= -16 + stone_h {
                                    2 // Stone
                                } else if y < height {
                                    match biome {
                                        Biome::Desert => 7, // Sand
                                        Biome::Mountain => 2, // Stone
                                        _ => 1, // Dirt
                                    }
                                } else {
                                    if y < water_level { 
                                        7 // Sand underwater
                                    } else {
                                        match biome {
                                            Biome::Desert => 7, // Sand
                                            Biome::Snow => 12, // Snow
                                            Biome::Mountain => if y > 12 { 12 } else { 2 }, // Snow caps or Stone
                                            Biome::Plains => 0, // Grass
                                        }
                                    }
                                }
                            } else {
                                is_water = true;
                                if biome == Biome::Snow && y == water_level {
                                    13 // Ice
                                } else {
                                    4 // Water
                                }
                            };

                            // Ore Generation
                            if block_type_idx == 2 {
                                let scale = 0.08;
                                let p = [world_x as f64 * scale, y as f64 * scale, world_z as f64 * scale];
                                
                                if world_gen.perlin.get([p[0], p[1], p[2]]) > 0.5 { block_type_idx = 15; } // Iron
                                else if y > 0 && world_gen.perlin.get([p[0] + 100.0, p[1], p[2]]) > 0.55 { block_type_idx = 16; } // Copper
                                else if world_gen.perlin.get([p[0] + 200.0, p[1], p[2]]) > 0.65 { block_type_idx = 17; } // Silver
                                else if y < -5 && world_gen.perlin.get([p[0] + 300.0, p[1], p[2]]) > 0.7 { block_type_idx = 18; } // Gold
                            }

                            if let Some(mat) = voxel_assets.block_types.get(block_type_idx) {
                                let mesh = if is_water { voxel_assets.water_meshes[8].clone() } else { voxel_assets.mesh.clone() };

                                let mut entity_cmds = commands.spawn((
                                    PbrBundle {
                                        mesh,
                                        material: mat.clone(),
                                        transform: Transform::from_xyz(world_x as f32, y as f32, world_z as f32),
                                        ..default()
                                    },
                                    BlockType(block_type_idx),
                                    NeedsMeshUpdate, // Calculate visibility on first frame
                                ));

                                if is_water {
                                    entity_cmds.insert(Liquid { level: 9 });
                                }

                                let id = entity_cmds.id();
                                voxel_world.blocks.insert(pos, id);
                                chunk_blocks.push(pos);
                            }
                        }

                        // Trees
                        let ground_pos = IVec3::new(world_x, height, world_z);
                        if height > water_level && voxel_world.blocks.contains_key(&ground_pos) {
                            let seed = (world_x as f32 * 12.9898 + world_z as f32 * 78.233).sin().abs();
                            
                            let mut tree_type = -1;
                            
                            if biome == Biome::Plains {
                                if seed < 0.005 { tree_type = 1; } // Large Oak (0.5%)
                                else if seed < 0.015 { tree_type = 0; } // Small Oak (1.0%)
                            } else if (biome == Biome::Mountain || biome == Biome::Snow) && seed < 0.02 {
                                tree_type = 2; // Pine (2.0%)
                            }

                            if tree_type != -1 {
                                let mut trunk_h = 4;
                                
                                if tree_type == 0 { trunk_h = 4 + (seed * 100.0) as i32 % 2; }
                                else if tree_type == 1 { trunk_h = 6 + (seed * 100.0) as i32 % 3; }
                                else if tree_type == 2 { trunk_h = 7 + (seed * 100.0) as i32 % 4; }

                                // Trunk
                                for i in 1..=trunk_h {
                                    let pos = IVec3::new(world_x, height + i, world_z);
                                    if voxel_world.blocks.contains_key(&pos) { continue; }
                                    
                                    let id = commands.spawn((
                                        PbrBundle {
                                            mesh: voxel_assets.mesh.clone(),
                                            material: voxel_assets.block_types[3].clone(), // Wood
                                            transform: Transform::from_xyz(pos.x as f32, pos.y as f32, pos.z as f32),
                                            ..default()
                                        },
                                        BlockType(3),
                                        NeedsMeshUpdate,
                                    )).id();
                                    voxel_world.blocks.insert(pos, id);
                                    chunk_blocks.push(pos);
                                }

                                // Leaves
                                if tree_type == 0 || tree_type == 1 { // Oak Types
                                    let radius = if tree_type == 1 { 3 } else { 2 };
                                    let leaves_start = if tree_type == 1 { height + trunk_h - 3 } else { height + trunk_h - 1 };
                                    
                                    for ly in leaves_start..=height + trunk_h + 1 {
                                        for lx in world_x - radius..=world_x + radius {
                                            for lz in world_z - radius..=world_z + radius {
                                                let pos = IVec3::new(lx, ly, lz);
                                                if voxel_world.blocks.contains_key(&pos) { continue; }
                                                
                                                // Blob check
                                                let d = ((lx - world_x).pow(2) + (ly - (height + trunk_h - 1)).pow(2) + (lz - world_z).pow(2)) as f32;
                                                if d > (radius as f32 + 0.5).powi(2) { continue; }

                                                let id = commands.spawn((
                                                    PbrBundle {
                                                        mesh: voxel_assets.mesh.clone(),
                                                        material: voxel_assets.block_types[11].clone(), // Leaves
                                                        transform: Transform::from_xyz(pos.x as f32, pos.y as f32, pos.z as f32),
                                                        ..default()
                                                    },
                                                    BlockType(11),
                                                    NeedsMeshUpdate,
                                                )).id();
                                                voxel_world.blocks.insert(pos, id);
                                                
                                                let c_coord = IVec2::new((lx as f32 / CHUNK_SIZE as f32).floor() as i32, (lz as f32 / CHUNK_SIZE as f32).floor() as i32);
                                                if c_coord == chunk_coord {
                                                    chunk_blocks.push(pos);
                                                } else {
                                                    voxel_world.chunks.entry(c_coord).or_default().push(pos);
                                                }
                                            }
                                        }
                                    }
                                } else if tree_type == 2 { // Pine
                                    for i in 0..trunk_h {
                                        let ly = height + trunk_h + 1 - i;
                                        let radius = (i as f32 * 0.4).floor() as i32;
                                        
                                        for lx in world_x - radius..=world_x + radius {
                                            for lz in world_z - radius..=world_z + radius {
                                                let pos = IVec3::new(lx, ly, lz);
                                                if voxel_world.blocks.contains_key(&pos) { continue; }
                                                
                                                if (lx - world_x).abs() + (lz - world_z).abs() > radius + 1 { continue; }

                                            let id = commands.spawn((
                                                PbrBundle {
                                                    mesh: voxel_assets.mesh.clone(),
                                                    material: voxel_assets.block_types[11].clone(), // Leaves
                                                    transform: Transform::from_xyz(pos.x as f32, pos.y as f32, pos.z as f32),
                                                    ..default()
                                                },
                                                BlockType(11),
                                                NeedsMeshUpdate,
                                            )).id();
                                            voxel_world.blocks.insert(pos, id);
                                            
                                            let c_coord = IVec2::new((lx as f32 / CHUNK_SIZE as f32).floor() as i32, (lz as f32 / CHUNK_SIZE as f32).floor() as i32);
                                            if c_coord == chunk_coord {
                                                chunk_blocks.push(pos);
                                            } else {
                                                voxel_world.chunks.entry(c_coord).or_default().push(pos);
                                            }
                                        }
                                    }
                                }
                                }
                            }
                        }
                    }
                }
                voxel_world.chunks.entry(chunk_coord).or_default().append(&mut chunk_blocks);
            }
        }
    }

    // Despawn chunks far away
    let mut chunks_to_remove = Vec::new();
    for &chunk_coord in voxel_world.chunks.keys() {
        let dist = (chunk_coord - player_chunk).abs();
        if dist.x > render_distance + 1 || dist.y > render_distance + 1 {
            chunks_to_remove.push(chunk_coord);
        }
    }

    for chunk_coord in chunks_to_remove {
        voxel_world.generated_chunks.remove(&chunk_coord);
        if let Some(blocks) = voxel_world.chunks.remove(&chunk_coord) {
            for pos in blocks {
                if let Some(entity) = voxel_world.blocks.remove(&pos) {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

fn update_mesh_system(
    mut commands: Commands,
    voxel_world: Res<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    mut query: Query<(Entity, &Transform, &BlockType), With<NeedsMeshUpdate>>,
    block_type_query: Query<&BlockType>,
) {
    for (entity, transform, block_type) in query.iter_mut() {
        let pos = transform.translation.round().as_ivec3();
        if voxel_world.blocks.get(&pos) != Some(&entity) {
            continue;
        }

        commands.entity(entity).remove::<NeedsMeshUpdate>();
        
        // Don't cull faces for water (complex) or special blocks, just solid ones
        if block_type.0 == 4 || block_type.0 == 9 || block_type.0 == 10 || block_type.0 == 13 || block_type.0 == 14 || block_type.0 == 19 { continue; }

        let mut mask = 0;

        // Check 6 neighbors
        let dirs = [
            (IVec3::X, 1), (IVec3::NEG_X, 2),
            (IVec3::Y, 4), (IVec3::NEG_Y, 8),
            (IVec3::Z, 16), (IVec3::NEG_Z, 32)
        ];

        for (dir, bit) in dirs {
            if let Some(&neighbor) = voxel_world.blocks.get(&(pos + dir)) {
                if let Ok(n_type) = block_type_query.get(neighbor) {
                    // If neighbor is solid (not water), hide face
                    if n_type.0 != 4 && n_type.0 != 9 && n_type.0 != 10 && n_type.0 != 13 && n_type.0 != 14 && n_type.0 != 19 { mask |= bit; }
                }
            }
        }

        if mask == 63 {
            // Fully occluded, remove mesh to save draw calls
            commands.entity(entity).remove::<Handle<Mesh>>();
        } else {
            commands.entity(entity).insert(voxel_assets.faces_meshes[mask].clone());
        }
    }
}

fn water_dynamics(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    mut query: Query<(Entity, &mut Transform, &mut Liquid)>,
    block_type_query: Query<&BlockType>,
    block_material_query: Query<&Handle<StandardMaterial>>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_seconds();
    if *timer < 0.05 { return; } // Run faster for smoother flow
    *timer = 0.0;

    // Collect liquid entities with their positions
    let mut liquid_entities: Vec<(Entity, IVec3)> = query.iter()
        .map(|(e, t, _)| {
            let p = t.translation;
            (e, IVec3::new(p.x.round() as i32, (p.y + 0.5).floor() as i32, p.z.round() as i32))
        })
        .filter(|(e, pos)| voxel_world.blocks.get(pos) == Some(e))
        .collect();
    
    // Sort by Y ascending (bottom-up) so lower blocks move first
    liquid_entities.sort_by_key(|(_, pos)| pos.y);

    for (entity, mut pos) in liquid_entities {
        // Optimization: Only update if exposed to air (at least one empty neighbor)
        let mut exposed = false;
        for offset in [IVec3::Y, IVec3::NEG_Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
            if !voxel_world.blocks.contains_key(&(pos + offset)) {
                exposed = true;
                break;
            }
        }
        if !exposed { continue; }

        let mut my_level = 0;
        if let Ok((_, _, liq)) = query.get(entity) { my_level = liq.level; }
        if my_level == 0 { continue; }

        // Erosion Logic: Small chance to erode block below if it is soft (Grass=0, Dirt=1, Sand=7)
        let pseudo_rand = (pos.x as f32 * 13.0 + pos.y as f32 * 37.0 + pos.z as f32 * 19.0 + time.elapsed_seconds() * 100.0).sin().abs();
        if pseudo_rand < 0.005 { // 0.5% chance per tick
            let down = pos - IVec3::Y;
            if let Some(&neighbor) = voxel_world.blocks.get(&down) {
                if let Ok(block_type) = block_type_query.get(neighbor) {
                    if [0, 1, 7].contains(&block_type.0) {
                        // Spawn Particles
                        if let Ok(mat_handle) = block_material_query.get(neighbor) {
                            for i in 0..5 {
                                let r1 = (down.x as f32 + i as f32 * 0.23).sin();
                                let r2 = (down.y as f32 + i as f32 * 0.45).cos();
                                let r3 = (down.z as f32 + i as f32 * 0.67).sin();
                                commands.spawn((
                                    PbrBundle {
                                        mesh: voxel_assets.mesh.clone(),
                                        material: mat_handle.clone(),
                                        transform: Transform::from_xyz(down.x as f32 + 0.5, down.y as f32 + 0.5, down.z as f32 + 0.5).with_scale(Vec3::splat(0.2)),
                                        ..default()
                                    },
                                    Particle {
                                        lifetime: Timer::from_seconds(0.5, TimerMode::Once),
                                        velocity: Vec3::new(r1 * 2.0, r2.abs() * 3.0, r3 * 2.0),
                                    }
                                ));
                            }
                        }

                        // Destroy Block
                        commands.entity(neighbor).despawn();
                        voxel_world.blocks.remove(&down);
                        let chunk_coord = IVec2::new((down.x as f32 / CHUNK_SIZE as f32).floor() as i32, (down.z as f32 / CHUNK_SIZE as f32).floor() as i32);
                        if let Some(chunk) = voxel_world.chunks.get_mut(&chunk_coord) {
                            if let Some(idx) = chunk.iter().position(|&p| p == down) { chunk.remove(idx); }
                        }
                    }
                }
            }
        }

        // 1. Vertical Logic
        let down = pos - IVec3::Y;
        let mut moved_down = false;

        if let Some(&below_entity) = voxel_world.blocks.get(&down) {
            // Merge Down
            if let Ok([(_, _, mut my_liq), (_, _, mut below_liq)]) = query.get_many_mut([entity, below_entity]) {
                let space = 9 - below_liq.level;
                if space > 0 {
                    let transfer = std::cmp::min(my_liq.level, space);
                    below_liq.level += transfer;
                    my_liq.level -= transfer;
                }
            }
        } else {
            // Move Down
            if let Ok((_, mut transform, _)) = query.get_mut(entity) {
                transform.translation.y -= 1.0;
                update_voxel_map(&mut commands, &mut voxel_world, pos, down, entity);
                pos = down;
                moved_down = true;
            }
        }

        // Refresh level
        if let Ok((_, _, liq)) = query.get(entity) { my_level = liq.level; }
        if my_level == 0 || moved_down { continue; }

        // 2. Horizontal Logic
        let directions = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
        let mut shuffled_dirs = directions.to_vec();
        let offset = (time.elapsed_seconds() * 100.0) as usize;
        shuffled_dirs.rotate_left((pos.x.abs() as usize + pos.z.abs() as usize + offset) % 4);

        let mut flow_target = None;

        for dir in shuffled_dirs {
            let neighbor = pos + dir;
            let target_chunk = IVec2::new(
                (neighbor.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                (neighbor.z as f32 / CHUNK_SIZE as f32).floor() as i32,
            );
            if !voxel_world.generated_chunks.contains(&target_chunk) { continue; }

            if let Some(&neighbor_entity) = voxel_world.blocks.get(&neighbor) {
                // Merge Sideways
                if let Ok([(_, _, mut my_liq), (_, _, mut neighbor_liq)]) = query.get_many_mut([entity, neighbor_entity]) {
                    if neighbor_liq.level < 9 {
                        let space = 9 - neighbor_liq.level;
                        let transfer = std::cmp::min(my_liq.level, space);
                        neighbor_liq.level += transfer;
                        my_liq.level -= transfer;
                        if my_liq.level == 0 { break; }
                    }
                }
            } else if flow_target.is_none() {
                flow_target = Some(neighbor);
            }
        }

        // Refresh level
        if let Ok((_, _, liq)) = query.get(entity) { my_level = liq.level; }
        if my_level == 0 { continue; }

        // 3. Flow Sideways (Split or Move)
        if let Some(target) = flow_target {
             if let Ok((_, mut transform, mut liq)) = query.get_mut(entity) {
                // Randomly choose between splitting and flowing (moving)
                let pseudo_rand = (pos.x + pos.y + pos.z) as f32 + time.elapsed_seconds() * 10.0;
                let do_split = (pseudo_rand as i32) % 2 == 0;

                if do_split && liq.level > 1 {
                    let split = liq.level / 2;
                    if split > 0 {
                        liq.level -= split;
                        
                        let height = split as f32 / 9.0;
                        let id = commands.spawn((
                            PbrBundle {
                                mesh: voxel_assets.water_meshes[split as usize - 1].clone(),
                                material: voxel_assets.block_types[4].clone(),
                                transform: Transform::from_xyz(target.x as f32, target.y as f32 - 0.5 + height / 2.0, target.z as f32),
                                ..default()
                            },
                            BlockType(4),
                            Liquid { level: split },
                        )).id();
                        
                        voxel_world.blocks.insert(target, id);
                        let chunk_coord = IVec2::new(
                            (target.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                            (target.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                        );
                        voxel_world.chunks.entry(chunk_coord).or_default().push(target);
                        
                        // Update neighbors
                        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                            if let Some(&e) = voxel_world.blocks.get(&(target + dir)) {
                                commands.entity(e).insert(NeedsMeshUpdate);
                            }
                        }
                    }
                } else {
                    // Flow (Move entire block)
                    transform.translation.x = target.x as f32;
                    transform.translation.z = target.z as f32;
                    update_voxel_map(&mut commands, &mut voxel_world, pos, target, entity);
                }
            }
        }
    }

    // 3. Cleanup Empty Blocks
    for (entity, _, liq) in query.iter() {
        if liq.level == 0 {
            let pos = query.get(entity).unwrap().1.translation.round().as_ivec3();
            voxel_world.blocks.remove(&pos);
            let chunk_coord = IVec2::new(
                (pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                (pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
            );
            if let Some(chunk) = voxel_world.chunks.get_mut(&chunk_coord) {
                if let Some(idx) = chunk.iter().position(|&p| p == pos) { chunk.remove(idx); }
            }
            // Update neighbors
            for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                if let Some(&e) = voxel_world.blocks.get(&(pos + dir)) {
                    commands.entity(e).insert(NeedsMeshUpdate);
                }
            }
            commands.entity(entity).despawn();
        }
    }
}

fn sand_dynamics(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    mut query: Query<(Entity, &mut Transform, &BlockType)>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_seconds();
    if *timer < 0.05 { return; }
    *timer = 0.0;

    let mut sand_entities: Vec<(Entity, IVec3)> = query.iter()
        .filter(|(_, _, block_type)| block_type.0 == 7) // Sand is index 7
        .map(|(e, t, _)| {
            let p = t.translation;
            (e, IVec3::new(p.x.round() as i32, p.y.round() as i32, p.z.round() as i32))
        })
        .filter(|(e, pos)| voxel_world.blocks.get(pos) == Some(e))
        .collect();
    
    // Sort by Y ascending so we process bottom blocks first
    sand_entities.sort_by_key(|(_, pos)| pos.y);

    for (entity, pos) in sand_entities {
        let down = pos - IVec3::Y;
        
        let down_chunk = IVec2::new(
            (down.x as f32 / CHUNK_SIZE as f32).floor() as i32,
            (down.z as f32 / CHUNK_SIZE as f32).floor() as i32,
        );

        if !voxel_world.blocks.contains_key(&down) {
            // Fall straight down
            if voxel_world.chunks.contains_key(&down_chunk) {
                if let Ok((_, mut transform, _)) = query.get_mut(entity) {
                    transform.translation.y -= 1.0;
                    update_voxel_map(&mut commands, &mut voxel_world, pos, down, entity);
                }
            }
        } else {
            // Try to slide down diagonals (piling effect)
            let directions = [IVec3::new(1, -1, 0), IVec3::new(-1, -1, 0), IVec3::new(0, -1, 1), IVec3::new(0, -1, -1)];
            
            let time_offset = (time.elapsed_seconds() * 10.0) as i32;
            let pos_sum = pos.x.wrapping_add(pos.y).wrapping_add(pos.z);
            let offset = (time_offset.wrapping_add(pos_sum)).rem_euclid(4) as usize;
            
            for i in 0..4 {
                let dir = directions[(i + offset) % 4];
                let target = pos + dir;
                let target_chunk = IVec2::new(
                    (target.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                    (target.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                );

                if !voxel_world.blocks.contains_key(&target) {
                    if voxel_world.chunks.contains_key(&target_chunk) {
                        if let Ok((_, mut transform, _)) = query.get_mut(entity) {
                            transform.translation = target.as_vec3();
                            update_voxel_map(&mut commands, &mut voxel_world, pos, target, entity);
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn water_source_system(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    query: Query<&Transform, With<WaterSource>>,
    mut liquid_query: Query<&mut Liquid>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_seconds();
    if *timer < 0.2 { return; }
    *timer = 0.0;

    for transform in query.iter() {
        let pos = transform.translation.round().as_ivec3();
        let up = pos + IVec3::Y;
        
        if let Some(&entity) = voxel_world.blocks.get(&up) {
            // If water exists above, keep it full
            if let Ok(mut liquid) = liquid_query.get_mut(entity) {
                if liquid.level < 9 { liquid.level = 9; }
            }
        } else {
            // Spawn new water
            let id = commands.spawn((
                PbrBundle {
                    mesh: voxel_assets.water_meshes[8].clone(),
                    material: voxel_assets.block_types[4].clone(),
                    transform: Transform::from_xyz(up.x as f32, up.y as f32, up.z as f32),
                    ..default()
                },
                BlockType(4),
                Liquid { level: 9 },
            )).id();
            voxel_world.blocks.insert(up, id);
            
            let chunk_coord = IVec2::new(
                (up.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                (up.z as f32 / CHUNK_SIZE as f32).floor() as i32,
            );
            voxel_world.chunks.entry(chunk_coord).or_default().push(up);
        }
    }
}

fn water_drain_system(
    voxel_world: Res<VoxelWorld>,
    query: Query<&Transform, With<WaterDrain>>,
    mut liquid_query: Query<&mut Liquid>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_seconds();
    if *timer < 0.1 { return; }
    *timer = 0.0;

    for transform in query.iter() {
        let pos = transform.translation.round().as_ivec3();
        let up = pos + IVec3::Y;
        
        if let Some(&entity) = voxel_world.blocks.get(&up) {
            if let Ok(mut liquid) = liquid_query.get_mut(entity) {
                liquid.level = 0;
            }
        }
    }
}

fn update_water_level(
    voxel_assets: Res<VoxelAssets>,
    mut query: Query<(&mut Transform, &mut Handle<Mesh>, &Liquid), Changed<Liquid>>,
) {
    for (mut transform, mut mesh_handle, liquid) in query.iter_mut() {
        if liquid.level > 0 && liquid.level <= 9 {
            *mesh_handle = voxel_assets.water_meshes[liquid.level as usize - 1].clone();
            
            // Calculate visual height offset so the block sits on the floor
            let height = liquid.level as f32 / 9.0;
            let base_y = (transform.translation.y + 0.5).floor(); // Get the integer Y of the cell floor
            transform.translation.y = base_y + (height / 2.0) - 0.5;
        }
    }
}

fn furnace_system(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Furnace, &Transform)>,
    mut light_query: Query<&mut PointLight>,
    children_query: Query<&Children>,
    time: Res<Time>,
    voxel_assets: Res<VoxelAssets>,
) {
    for (entity, mut furnace, transform) in query.iter_mut() {
        let is_smelting = furnace.input.is_some() && furnace.output.is_none();

        // Update Light
        if let Ok(children) = children_query.get(entity) {
            for &child in children.iter() {
                if let Ok(mut light) = light_query.get_mut(child) {
                    light.intensity = if is_smelting { 
                        3000.0 + (time.elapsed_seconds() * 20.0).sin() * 500.0 
                    } else { 0.0 };
                }
            }
        }

        if is_smelting {
            furnace.timer.tick(time.delta());
            if furnace.timer.finished() {
                let input = furnace.input.take().unwrap();
                let output = match input {
                    15 => 20, // Iron Ore -> Iron Ingot
                    16 => 21, // Copper Ore -> Copper Ingot
                    17 => 22, // Silver Ore -> Silver Ingot
                    18 => 23, // Gold Ore -> Gold Ingot
                    _ => 0,
                };
                furnace.output = Some(output);
                furnace.timer.reset();
            } else {
                // Particles
                if time.elapsed_seconds() % 0.2 < 0.05 {
                     commands.spawn((
                        PbrBundle {
                            mesh: voxel_assets.mesh.clone(),
                            material: voxel_assets.block_types[14].clone(), // Torch material (emissive)
                            transform: Transform::from_translation(transform.translation + Vec3::new(0.0, 0.5, 0.0)).with_scale(Vec3::splat(0.1)),
                            ..default()
                        },
                        Particle {
                            lifetime: Timer::from_seconds(0.5, TimerMode::Once),
                            velocity: Vec3::new(0.0, 1.0, 0.0),
                        }
                    ));
                }
            }
        }
    }
}

fn day_night_cycle(
    mut time_res: ResMut<WorldTime>,
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut DirectionalLight)>,
    mut ambient_light: ResMut<AmbientLight>,
    mut clear_color: ResMut<ClearColor>,
    player_query: Query<&Transform, (With<Player>, Without<DirectionalLight>)>,
) {
    time_res.time += time.delta_seconds() * time_res.speed;
    if time_res.time > 1.0 { time_res.time -= 1.0; }

    // Get player height for cave darkness
    let player_y = player_query.get_single().map(|t| t.translation.y).unwrap_or(0.0);
    let cave_factor = ((10.0 - player_y) / 20.0).clamp(0.0, 1.0); // 0.0 at Y=10, 1.0 at Y=-10

    // 0.0 = Sunrise, 0.25 = Noon, 0.5 = Sunset, 0.75 = Midnight
    let angle = (time_res.time - 0.25) * std::f32::consts::PI * 2.0;
    
    for (mut transform, mut light) in query.iter_mut() {
        transform.rotation = Quat::from_rotation_z(angle);
        transform.translation = transform.rotation * Vec3::Y * 100.0;
        transform.look_at(Vec3::ZERO, Vec3::Y);
        
        let sun_height = transform.translation.y;
        
        if sun_height > 0.0 {
             light.illuminance = 10000.0 * (sun_height / 100.0).max(0.1);
             light.shadows_enabled = true;
        } else {
             light.illuminance = 0.0;
             light.shadows_enabled = false;
        }
        
        // Ambient & Sky
        let t = (sun_height / 100.0).clamp(-0.2, 0.2);
        let t_norm = (t + 0.2) / 0.4; // 0.0 to 1.0
        
        let day_color = LinearRgba::new(0.5, 0.8, 0.9, 1.0);
        let night_color = LinearRgba::new(0.05, 0.05, 0.1, 1.0);
        let sunset_color = LinearRgba::new(0.8, 0.4, 0.2, 1.0);
        let cave_color = LinearRgba::new(0.01, 0.01, 0.01, 1.0);
        
        let sky_color = if t_norm < 0.5 {
             night_color.mix(&sunset_color, t_norm * 2.0)
        } else {
             sunset_color.mix(&day_color, (t_norm - 0.5) * 2.0)
        };
        
        // Blend based on cave factor
        let final_color = sky_color.mix(&cave_color, cave_factor);
        clear_color.0 = final_color.into();
        
        let base_brightness = 50.0 + 300.0 * t_norm.max(0.0);
        ambient_light.brightness = base_brightness * (1.0 - cave_factor) + 5.0 * cave_factor;
        ambient_light.color = final_color.into();
    }
}

fn save_load_world(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    block_query: Query<(Entity, &Transform, &BlockType)>,
) {
    // Save (F5)
    if keys.just_pressed(KeyCode::F5) {
        let mut saved_blocks = Vec::new();
        for (_, transform, block_type) in block_query.iter() {
            let pos = transform.translation.as_ivec3();
            saved_blocks.push(SavedBlock {
                x: pos.x,
                y: pos.y,
                z: pos.z,
                type_index: block_type.0,
                level: if block_type.0 == 4 { 9 } else { 0 }, // Default saved blocks to full
            });
        }
        
        if let Ok(file) = File::create("world.json") {
            let _ = serde_json::to_writer(file, &saved_blocks);
            info!("World saved to world.json");
        }
    }

    // Load (F9)
    if keys.just_pressed(KeyCode::F9) {
        if let Ok(file) = File::open("world.json") {
            // Clear existing world
            for (entity, _, _) in block_query.iter() {
                commands.entity(entity).despawn();
            }
            voxel_world.blocks.clear();
            voxel_world.chunks.clear();
            voxel_world.generated_chunks.clear();

            let reader = BufReader::new(file);
            if let Ok(saved_blocks) = serde_json::from_reader::<_, Vec<SavedBlock>>(reader) {
                for block in saved_blocks {
                    let pos = IVec3::new(block.x, block.y, block.z);
                    if let Some(mat) = voxel_assets.block_types.get(block.type_index) {
                        let mesh = if block.type_index == 4 && block.level > 0 {
                            voxel_assets.water_meshes[8].clone()
                        } else if block.type_index == 14 {
                            voxel_assets.torch_mesh.clone()
                        } else {
                            voxel_assets.mesh.clone()
                        };

                        let mut entity_cmds = commands.spawn((
                            PbrBundle {
                                mesh,
                                material: mat.clone(),
                                transform: Transform::from_xyz(pos.x as f32, pos.y as f32, pos.z as f32),
                                ..default()
                            },
                            BlockType(block.type_index),
                            NeedsMeshUpdate,
                        ));

                        if block.type_index == 4 {
                            let level = if block.level == 0 { 9 } else { block.level };
                            entity_cmds.insert(Liquid { level });
                        }
                        if block.type_index == 5 { entity_cmds.insert(WaterSource); }
                        if block.type_index == 6 { entity_cmds.insert(WaterDrain); }

                        if block.type_index == 19 {
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

                        if block.type_index == 14 {
                            entity_cmds.with_children(|parent| {
                                parent.spawn(PointLightBundle {
                                    point_light: PointLight {
                                        intensity: 6000.0,
                                        range: 30.0,
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
                        
                        voxel_world.blocks.insert(pos, id);
                        
                        let chunk_coord = IVec2::new(
                            (pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                            (pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                        );
                        voxel_world.chunks.entry(chunk_coord).or_default().push(pos);
                        voxel_world.generated_chunks.insert(chunk_coord);
                    }
                }
                info!("World loaded from world.json");
            }
        }
    }
}
