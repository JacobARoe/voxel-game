use bevy::prelude::*;
use bevy::pbr::NotShadowCaster;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
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
pub struct BlockDurability {
    pub current_durability: f32,
    pub max_durability: f32,
    pub crack_level: u32, // 0-3 crack levels for visual effects
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

#[derive(Resource, Clone)]
pub struct VoxelAssets {
    pub mesh: Handle<Mesh>,
    pub water_meshes: Vec<Handle<Mesh>>,
    pub faces_meshes: Vec<Handle<Mesh>>,
    pub wireframe_mesh: Handle<Mesh>,
    pub wireframe_material: Handle<StandardMaterial>,
    pub _material: Handle<StandardMaterial>,
    pub block_types: Vec<Handle<StandardMaterial>>,
    pub block_names: Vec<String>,
    pub snake_material: Handle<StandardMaterial>,
    pub snake_mesh: Handle<Mesh>,
    pub eye_mesh: Handle<Mesh>,
    pub eye_material: Handle<StandardMaterial>,
    pub segment_mesh: Handle<Mesh>,
    // Materials for cracked blocks
    pub cracked_block_types: Vec<Handle<StandardMaterial>>,
}

#[derive(Resource)]
pub struct VoxelSounds {
    pub place: Handle<AudioSource>,
    pub break_sound: Handle<AudioSource>,
    pub footstep: Handle<AudioSource>,
}

#[derive(Resource, Default)]
pub struct VoxelWorld {
    pub blocks: HashMap<IVec3, Entity>,
    pub chunks: HashMap<IVec2, Vec<IVec3>>,
}

#[derive(Resource)]
pub struct WorldGen {
    pub _seed: u32,
    pub perlin: Perlin,
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VoxelWorld::default())
           .insert_resource(WorldGen { _seed: 42, perlin: Perlin::new(42) })
           .add_systems(Startup, setup_world)
           .add_systems(Update, (
               update_chunks,
               update_particles,
               save_load_world,
               water_dynamics,
               update_water_level,
               water_source_system,
               water_drain_system,
               sand_dynamics,
               update_block_cracks,
           ).run_if(in_state(GameState::Playing)))
           .add_systems(PostUpdate, update_mesh_system);
    }
}

pub fn get_terrain_height(x: i32, z: i32, perlin: &Perlin) -> (i32, i32, i32) {
    let stone_noise = perlin.get([x as f64 * 0.1, z as f64 * 0.1]);
    let stone_h = ((stone_noise * 0.3 + 0.3) * 6.0).clamp(0.0, 6.0).round() as i32; // Reduced from 10 to 6

    let dirt_noise = perlin.get([x as f64 * 0.1 + 100.0, z as f64 * 0.1 + 100.0]);
    let dirt_h = ((dirt_noise * 0.3 + 0.3) * 6.0).clamp(0.0, 6.0).round() as i32; // Reduced from 10 to 6

    (stone_h, dirt_h, -16 + stone_h + dirt_h)
}

fn get_block_durability(block_type: usize) -> f32 {
    match block_type {
        0 => 3.0,  // Grass - medium durability
        1 => 2.0,  // Dirt - low durability
        2 => 8.0,  // Stone - high durability
        3 => 6.0,  // Wood - medium-high durability
        4 => 0.0,  // Water - not breakable
        5 => 0.0,  // Water Source - not breakable
        6 => 0.0,  // Water Drain - not breakable
        7 => 1.0,  // Sand - very low durability
        8 => 20.0, // Bedrock - very high durability (practically unbreakable)
        9 => 4.0,  // Cobblestone - high durability
        10 => 2.0, // Gravel - low durability
        11 => 1.0, // Snow - very low durability
        12 => 3.0, // Clay - medium durability
        13 => 6.0, // Coal - medium-high durability
        14 => 7.0, // Iron - high durability
        15 => 6.5, // Copper - high durability
        16 => 8.0, // Gold - high durability
        17 => 10.0,// Diamond - very high durability
        18 => 5.0, // Emerald - high durability
        19 => 4.0, // Redstone - medium durability
        20 => 5.0, // Lapis - medium-high durability
        21 => 9.0, // Obsidian - very high durability
        22 => 5.0, // Moss Stone - medium durability
        23 => 5.0, // Brick - medium durability
        _ => 3.0,  // Default durability
    }
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {

    // Dark background plane - makes gaps between blocks appear as black outlines
    let dark_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.02),
        unlit: true, // Don't respond to lighting
        ..default()
    });
    commands.spawn(PbrBundle {
        mesh: meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(500.0))),
        material: dark_mat,
        transform: Transform::from_xyz(0.0, -20.0, 0.0),
        ..default()
    });

    // Create shared resources
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    
    // Generate 64 meshes for all face combinations (Greedy-ish meshing per block)
    // Bitmask: 1:+X, 2:-X, 4:+Y, 8:-Y, 16:+Z, 32:-Z
    // Full-size blocks (0.5) eliminate gaps between them
    let mut faces_meshes = Vec::new();
    for i in 0..64 {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        let mut v_idx = 0;

        const S: f32 = 0.5; // Full size to eliminate gaps

        let add_face = |pos: &mut Vec<[f32; 3]>, norm: &mut Vec<[f32; 3]>, uv: &mut Vec<[f32; 2]>, ind: &mut Vec<u32>, v: &mut u32, corners: [[f32; 3]; 4], normal: [f32; 3]| {
            pos.extend_from_slice(&corners);
            norm.extend_from_slice(&[normal; 4]);
            uv.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
            ind.extend_from_slice(&[*v, *v+1, *v+2, *v+2, *v+3, *v]);
            *v += 4;
        };

        // +X (Right)
        if (i & 1) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[S, S, S], [S, -S, S], [S, -S, -S], [S, S, -S]], [1.0, 0.0, 0.0]); }
        // -X (Left)
        if (i & 2) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-S, S, -S], [-S, -S, -S], [-S, -S, S], [-S, S, S]], [-1.0, 0.0, 0.0]); }
        // +Y (Top)
        if (i & 4) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-S, S, S], [S, S, S], [S, S, -S], [-S, S, -S]], [0.0, 1.0, 0.0]); }
        // -Y (Bottom)
        if (i & 8) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-S, -S, -S], [S, -S, -S], [S, -S, S], [-S, -S, S]], [0.0, -1.0, 0.0]); }
        // +Z (Back)
        if (i & 16) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[S, S, S], [-S, S, S], [-S, -S, S], [S, -S, S]], [0.0, 0.0, 1.0]); }
        // -Z (Front)
        if (i & 32) == 0 { add_face(&mut positions, &mut normals, &mut uvs, &mut indices, &mut v_idx, [[-S, S, -S], [S, S, -S], [S, -S, -S], [-S, -S, -S]], [0.0, 0.0, -1.0]); }

        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
        faces_meshes.push(meshes.add(mesh));
    }

    // Create wireframe mesh for block outlines (12 edges of a cube)
    let wireframe_mesh = {
        const W: f32 = 0.5; // Match the full block size
        let positions: Vec<[f32; 3]> = vec![
            // Bottom face edges
            [-W, -W, -W], [W, -W, -W],
            [W, -W, -W], [W, -W, W],
            [W, -W, W], [-W, -W, W],
            [-W, -W, W], [-W, -W, -W],
            // Top face edges
            [-W, W, -W], [W, W, -W],
            [W, W, -W], [W, W, W],
            [W, W, W], [-W, W, W],
            [-W, W, W], [-W, W, -W],
            // Vertical edges
            [-W, -W, -W], [-W, W, -W],
            [W, -W, -W], [W, W, -W],
            [W, -W, W], [W, W, W],
            [-W, -W, W], [-W, W, W],
        ];
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        meshes.add(mesh)
    };

    // Black unlit material for wireframes
    let wireframe_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.0, 0.0, 0.0),
        unlit: true,
        ..default()
    });

    let mut water_meshes = Vec::new();
    for i in 0..9 {
        let height = (i as f32 + 1.0) / 9.0;
        water_meshes.push(meshes.add(Cuboid::new(1.0, height, 1.0)));
    }
    let grass = materials.add(Color::srgb(0.3, 0.8, 0.3));
    let grass_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.8, 0.3),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let dirt = materials.add(Color::srgb(0.8, 0.7, 0.6));
    let dirt_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.7, 0.6),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let stone = materials.add(Color::srgb(0.5, 0.5, 0.5));
    let stone_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.45, 0.45),
        perceptual_roughness: 0.95,
        metallic: 0.1,
        ..default()
    });
    let wood = materials.add(Color::srgb(0.4, 0.2, 0.1));
    let wood_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.18, 0.09),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let sand = materials.add(Color::srgb(0.9, 0.8, 0.5));
    let sand_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.75, 0.45),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let water = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.4, 0.8, 0.5),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let source = materials.add(Color::srgb(0.0, 1.0, 1.0));
    let drain = materials.add(Color::srgb(0.2, 0.0, 0.0));
    let bedrock = materials.add(Color::srgb(0.1, 0.1, 0.1));
    let bedrock_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.95,
        metallic: 0.1,
        ..default()
    });
    // Add new block types
    let cobblestone = materials.add(Color::srgb(0.4, 0.4, 0.4));
    let cobblestone_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.35),
        perceptual_roughness: 0.95,
        metallic: 0.1,
        ..default()
    });
    let gravel = materials.add(Color::srgb(0.6, 0.6, 0.6));
    let gravel_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.55, 0.55),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let snow = materials.add(Color::srgb(0.9, 0.95, 1.0));
    let snow_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.9, 0.95),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let clay = materials.add(Color::srgb(0.6, 0.6, 0.8));
    let clay_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.55, 0.75),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let coal = materials.add(Color::srgb(0.2, 0.2, 0.2));
    let coal_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.18, 0.18),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let iron = materials.add(Color::srgb(0.6, 0.5, 0.4));
    let iron_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.45, 0.35),
        perceptual_roughness: 0.9,
        metallic: 0.15,
        ..default()
    });
    let copper = materials.add(Color::srgb(0.8, 0.5, 0.3));
    let copper_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.45, 0.28),
        perceptual_roughness: 0.9,
        metallic: 0.15,
        ..default()
    });
    let gold = materials.add(Color::srgb(0.9, 0.8, 0.2));
    let gold_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.75, 0.18),
        perceptual_roughness: 0.9,
        metallic: 0.2,
        ..default()
    });
    let diamond = materials.add(Color::srgb(0.3, 0.8, 0.9));
    let diamond_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.75, 0.85),
        perceptual_roughness: 0.85,
        metallic: 0.15,
        ..default()
    });
    let emerald = materials.add(Color::srgb(0.2, 0.9, 0.4));
    let emerald_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.85, 0.35),
        perceptual_roughness: 0.85,
        metallic: 0.15,
        ..default()
    });
    let redstone = materials.add(Color::srgb(0.9, 0.2, 0.2));
    let redstone_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.18, 0.18),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let lapis = materials.add(Color::srgb(0.2, 0.3, 0.8));
    let lapis_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.25, 0.75),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let obsidian = materials.add(Color::srgb(0.2, 0.0, 0.3));
    let obsidian_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.0, 0.25),
        perceptual_roughness: 0.95,
        metallic: 0.2,
        ..default()
    });
    let moss_stone = materials.add(Color::srgb(0.3, 0.5, 0.3));
    let moss_stone_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.45, 0.28),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let brick = materials.add(Color::srgb(0.7, 0.3, 0.3));
    let brick_cracked = materials.add(StandardMaterial {
        base_color: Color::srgb(0.65, 0.28, 0.28),
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let snake_mat = materials.add(Color::srgb(0.2, 0.8, 0.2));
    let snake_mesh = meshes.add(Cuboid::new(0.5, 0.5, 0.9));
    let eye_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.05));
    let segment_mesh = meshes.add(Cuboid::new(0.4, 0.4, 0.4));
    let eye_mat = materials.add(Color::BLACK);

    commands.insert_resource(VoxelAssets {
        mesh,
        water_meshes,
        faces_meshes,
        wireframe_mesh,
        wireframe_material,
        _material: grass.clone(),
        block_types: vec![
            grass, dirt, stone, wood, water.clone(), source.clone(), drain.clone(), sand, bedrock,
            cobblestone, gravel, snow, clay, coal, iron, copper, gold, diamond,
            emerald, redstone, lapis, obsidian, moss_stone, brick, snake_mat.clone(), snake_mat.clone()
        ],
        // Create cracked versions of all block types
        cracked_block_types: vec![
            grass_cracked, dirt_cracked, stone_cracked, wood_cracked, water.clone(), source.clone(), drain.clone(), sand_cracked, bedrock_cracked,
            cobblestone_cracked, gravel_cracked, snow_cracked, clay_cracked, coal_cracked, iron_cracked, copper_cracked, gold_cracked, diamond_cracked,
            emerald_cracked, redstone_cracked, lapis_cracked, obsidian_cracked, moss_stone_cracked, brick_cracked, snake_mat.clone(), snake_mat.clone()
        ],
        block_names: vec![
            "Grass".to_string(), "Dirt".to_string(), "Stone".to_string(), "Wood".to_string(),
            "Water".to_string(), "Water Source".to_string(), "Water Drain".to_string(),
            "Sand".to_string(), "Bedrock".to_string(), "Cobblestone".to_string(),
            "Gravel".to_string(), "Snow".to_string(), "Clay".to_string(), "Coal".to_string(),
            "Iron".to_string(), "Copper".to_string(), "Gold".to_string(), "Diamond".to_string(),
            "Emerald".to_string(), "Redstone".to_string(), "Lapis".to_string(),
            "Obsidian".to_string(), "Moss Stone".to_string(), "Brick".to_string(),
            "Snake".to_string(), "Snake Segment".to_string()
        ],
        snake_material: snake_mat,
        snake_mesh,
        eye_mesh,
        eye_material: eye_mat,
        segment_mesh,
    });

    commands.insert_resource(VoxelSounds {
        place: asset_server.load("sounds/place.mp3"),
        break_sound: asset_server.load("sounds/break.mp3"),
        footstep: asset_server.load("sounds/footstep.mp3"),
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

pub fn update_voxel_map(commands: &mut Commands, voxel_world: &mut VoxelWorld, old_pos: IVec3, new_pos: IVec3, entity: Entity) {
    voxel_world.blocks.remove(&old_pos);

    voxel_world.blocks.insert(new_pos, entity);
    // Use get_entity to safely handle despawned entities
    if let Some(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.insert(NeedsMeshUpdate);
    }

    let old_chunk = IVec2::new((old_pos.x as f32 / CHUNK_SIZE as f32).floor() as i32, (old_pos.z as f32 / CHUNK_SIZE as f32).floor() as i32);

    // Tag neighbors for update (safely - they may have been despawned)
    for pos in [old_pos, new_pos] {
        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            if let Some(&e) = voxel_world.blocks.get(&(pos + dir)) {
                if let Some(mut entity_commands) = commands.get_entity(e) {
                    entity_commands.insert(NeedsMeshUpdate);
                }
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
    mut last_player_chunk: Local<Option<IVec2>>,
    mut chunk_update_timer: Local<Option<Timer>>,
) {
    // Initialize the timer if it hasn't been initialized yet
    if chunk_update_timer.is_none() {
        *chunk_update_timer = Some(Timer::from_seconds(0.1, TimerMode::Repeating));
    }

    let timer = chunk_update_timer.as_mut().unwrap();

    // Only update chunks every 0.1 seconds to reduce performance impact
    if !timer.tick(Duration::from_secs_f32(0.1)).just_finished() {
        return;
    }

    let player_transform = player_query.single();
    let render_distance = 5; // Reduced from 6 to improve performance

    // Calculate the chunk the player is currently in
    let player_chunk = IVec2::new(
        (player_transform.translation.x / CHUNK_SIZE as f32).floor() as i32,
        (player_transform.translation.z / CHUNK_SIZE as f32).floor() as i32,
    );

    // Only update if player moved to a new chunk
    if let Some(last_chunk) = *last_player_chunk {
        if last_chunk == player_chunk {
            return; // No need to update if player hasn't moved to a new chunk
        }
    }

    *last_player_chunk = Some(player_chunk);

    // Spawn chunks around player
    for x in -render_distance..=render_distance {
        for z in -render_distance..=render_distance {
            let chunk_coord = player_chunk + IVec2::new(x, z);

            if !voxel_world.chunks.contains_key(&chunk_coord) {
                let mut chunk_blocks = Vec::new();

                for bx in 0..CHUNK_SIZE {
                    for bz in 0..CHUNK_SIZE {
                        let world_x = chunk_coord.x * CHUNK_SIZE + bx;
                        let world_z = chunk_coord.y * CHUNK_SIZE + bz;

                        // Enhanced layered generation with more variety
                        let (stone_h, _, height) = get_terrain_height(world_x, world_z, &world_gen.perlin);

                        // Additional noise for surface features
                        let surface_noise = world_gen.perlin.get([world_x as f64 * 0.02, world_z as f64 * 0.02]);

                        // Generate column from Bedrock up to Height
                        for y in -16..=height {
                            let pos = IVec3::new(world_x, y, world_z);

                            // Additional noise for underground ore distribution (calculated per Y level)
                            let ore_noise = world_gen.perlin.get([world_x as f64 * 0.05, y as f64 * 0.05, world_z as f64 * 0.05]);

                            // Determine Block Type with more variety
                            let block_type_idx = if y == -16 {
                                8 // Bedrock
                            } else if y <= -16 + stone_h {
                                // Underground stone layer with ore veins
                                if ore_noise > 0.8 && y < -5 {
                                    // Coal ore in upper underground
                                    13
                                } else if ore_noise > 0.85 && y < -10 {
                                    // Iron ore deeper underground
                                    14
                                } else if ore_noise > 0.9 && y < -12 {
                                    // Copper ore deeper
                                    15
                                } else if ore_noise > 0.95 && y < -14 {
                                    // Gold ore very deep
                                    16
                                } else if ore_noise > 0.98 && y < -15 {
                                    // Diamond ore very rare and deep
                                    17
                                } else if ore_noise < -0.8 {
                                    // Lapis lazuli ore
                                    20
                                } else if ore_noise < -0.7 && y > -10 {
                                    // Redstone ore
                                    19
                                } else {
                                    // Regular stone with some variation
                                    if ore_noise > 0.5 { 9 } else { 2 } // Cobblestone or regular stone
                                }
                            } else if y == height {
                                // Surface layer - determine by height and surface noise
                                if height > 15 {
                                    // High altitudes - snow
                                    11
                                } else if height > 10 {
                                    // Mid-altitudes - moss stone or grass
                                    if surface_noise > 0.3 { 22 } else { 0 } // Moss stone or grass
                                } else if height <= 2 {
                                    // Low areas near water level - sand or clay
                                    if surface_noise > 0.2 { 7 } else { 12 } // Sand or clay
                                } else {
                                    // Regular surface - grass, dirt, or other
                                    if surface_noise > 0.4 { 0 } else if surface_noise > 0.1 { 1 } else { 22 } // Grass, dirt, or moss stone
                                }
                            } else if y > height - 3 {
                                // Subsurface - dirt, clay, gravel, etc.
                                if surface_noise > 0.6 { 1 } // Dirt
                                else if surface_noise < -0.6 { 10 } // Gravel
                                else { 1 } // Mostly dirt
                            } else {
                                // Deep underground - mostly stone
                                2
                            };

                            if let Some(mat) = voxel_assets.block_types.get(block_type_idx) {
                                // All blocks have shadow casting disabled for performance
                                let durability = get_block_durability(block_type_idx);
                                // Select material based on crack level (initially 0)
                                let block_material = if block_type_idx < voxel_assets.block_types.len() && block_type_idx < voxel_assets.cracked_block_types.len() {
                                    // For now, use the regular material for initial blocks
                                    voxel_assets.block_types[block_type_idx].clone()
                                } else {
                                    mat.clone()
                                };

                                let id = commands.spawn((
                                    PbrBundle {
                                        mesh: voxel_assets.mesh.clone(),
                                        material: block_material,
                                        transform: Transform::from_xyz(world_x as f32, y as f32, world_z as f32),
                                        ..default()
                                    },
                                    BlockType(block_type_idx),
                                    NeedsMeshUpdate,
                                    BlockDurability {
                                        current_durability: durability,
                                        max_durability: durability,
                                        crack_level: 0,
                                    },
                                    NotShadowCaster,
                                )).with_children(|parent| {
                                    // Wireframe outline child - only add for visible blocks
                                    if block_type_idx != 4 { // Skip wireframe for water
                                        parent.spawn((
                                            PbrBundle {
                                                mesh: voxel_assets.wireframe_mesh.clone(),
                                                material: voxel_assets.wireframe_material.clone(),
                                                ..default()
                                            },
                                            NotShadowCaster,
                                        ));
                                    }
                                }).id();
                                voxel_world.blocks.insert(pos, id);
                                chunk_blocks.push(pos);
                            }
                        }
                    }
                }

                // Add natural water pools in low-lying areas
                // Check for potential pool locations in this chunk
                for bx in 0..CHUNK_SIZE {
                    for bz in 0..CHUNK_SIZE {
                        let world_x = chunk_coord.x * CHUNK_SIZE + bx;
                        let world_z = chunk_coord.y * CHUNK_SIZE + bz;

                        // Calculate terrain height at this position
                        let (_, _, height) = get_terrain_height(world_x, world_z, &world_gen.perlin);

                        // Check if this is a low area surrounded by higher terrain (potential pool location)
                        let mut surrounding_heights = Vec::new();
                        for dx in -2..=2 {
                            for dz in -2..=2 {
                                if dx == 0 && dz == 0 { continue; } // Skip center
                                let (_, _, neighbor_height) = get_terrain_height(world_x + dx, world_z + dz, &world_gen.perlin);
                                surrounding_heights.push(neighbor_height);
                            }
                        }

                        // Calculate average surrounding height
                        let avg_surrounding_height: i32 = surrounding_heights.iter().sum::<i32>() / surrounding_heights.len() as i32;

                        // If this is a low area compared to surroundings and not too high, add a water pool
                        if height <= 2 && avg_surrounding_height > height + 1 && height < 5 {
                            // Add water pool at this location
                            let pool_center_x = world_x as f32;
                            let pool_center_z = world_z as f32;

                            // Create a small water pool (3x3 area typically)
                            for px in -1..=1 {
                                for pz in -1..=1 {
                                    let pool_x = pool_center_x as i32 + px;
                                    let pool_z = pool_center_z as i32 + pz;

                                    // Calculate distance from center to make circular pool
                                    let dist = ((px as f32).powi(2) + (pz as f32).powi(2)).sqrt();

                                    // Only place water if within circular area and not too far from center
                                    if dist <= 1.5 {
                                        let pool_pos = IVec3::new(pool_x, height, pool_z);

                                        // Remove any existing blocks at this position
                                        if let Some(existing_entity) = voxel_world.blocks.remove(&pool_pos) {
                                            commands.entity(existing_entity).despawn_recursive();
                                        }

                                        // Place water block
                                        let water_id = commands.spawn((
                                            PbrBundle {
                                                mesh: voxel_assets.water_meshes[8].clone(), // Full water height
                                                material: voxel_assets.block_types[4].clone(), // Water material
                                                transform: Transform::from_xyz(pool_x as f32, height as f32, pool_z as f32),
                                                ..default()
                                            },
                                            BlockType(4), // Water
                                            Liquid { level: 9 }, // Full water level
                                            NeedsMeshUpdate,
                                        )).id();

                                        voxel_world.blocks.insert(pool_pos, water_id);
                                        chunk_blocks.push(pool_pos);
                                    }
                                }
                            }
                        }
                    }
                }

                voxel_world.chunks.insert(chunk_coord, chunk_blocks);
            }
        }
    }

    // Despawn chunks far away (using a buffer to prevent flickering)
    let mut chunks_to_remove = Vec::new();
    for &chunk_coord in voxel_world.chunks.keys() {
        let dist = (chunk_coord - player_chunk).abs();
        if dist.x > render_distance + 2 || dist.y > render_distance + 2 {
            chunks_to_remove.push(chunk_coord);
        }
    }

    for chunk_coord in chunks_to_remove {
        if let Some(blocks) = voxel_world.chunks.remove(&chunk_coord) {
            for pos in blocks {
                if let Some(entity) = voxel_world.blocks.remove(&pos) {
                    commands.entity(entity).despawn_recursive();
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
    mut mesh_update_timer: Local<Option<Timer>>,
    time: Res<Time>,
) {
    // Initialize the timer if it hasn't been initialized yet
    if mesh_update_timer.is_none() {
        *mesh_update_timer = Some(Timer::from_seconds(0.016, TimerMode::Repeating)); // ~60 times per second max
    }

    let timer = mesh_update_timer.as_mut().unwrap();

    // Only update a limited number of meshes per frame to maintain performance
    if !timer.tick(time.delta()).just_finished() {
        return;
    }

    // Process blocks that need updates, with dynamic limits based on how many need updating
    let total_needing_updates = query.iter().len();
    let mut processed_count = 0;

    // Increase the limit when there are many pending updates to ensure responsiveness
    let max_updates_per_frame = if total_needing_updates > 100 {
        150  // Higher limit when there are many pending updates
    } else if total_needing_updates > 50 {
        100  // Medium limit
    } else {
        50   // Standard limit
    };

    for (entity, transform, block_type) in query.iter_mut() {
        if processed_count >= max_updates_per_frame {
            break;
        }

        let pos = transform.translation.round().as_ivec3();
        if voxel_world.blocks.get(&pos) != Some(&entity) {
            continue;
        }

        commands.entity(entity).remove::<NeedsMeshUpdate>();

        // Don't cull faces for water (complex) or special blocks, just solid ones
        if block_type.0 == 4 || block_type.0 == 9 || block_type.0 == 10 { continue; }

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
                    if n_type.0 != 4 && n_type.0 != 9 && n_type.0 != 10 { mask |= bit; }
                }
            }
        }

        if mask == 63 {
            // Fully occluded, remove mesh to save draw calls
            commands.entity(entity).remove::<Handle<Mesh>>();
        } else {
            commands.entity(entity).insert(voxel_assets.faces_meshes[mask].clone());
        }

        processed_count += 1;
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
    mut water_update_timer: Local<Option<Timer>>,
) {
    // Initialize the timer if it hasn't been initialized yet
    if water_update_timer.is_none() {
        *water_update_timer = Some(Timer::from_seconds(0.2, TimerMode::Repeating)); // Slower update rate
    }

    let timer = water_update_timer.as_mut().unwrap();

    // Only update water dynamics periodically to reduce performance impact
    if !timer.tick(time.delta()).just_finished() {
        return;
    }

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

    // Limit the number of water blocks processed per update to maintain performance
    let max_processed = (liquid_entities.len() / 3).max(1); // Process 33% of water blocks per update
    let mut processed_count = 0;

    for (entity, mut pos) in liquid_entities {
        if processed_count >= max_processed {
            break;
        }

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

        // Reduce erosion chance significantly to improve performance
        let pseudo_rand = (pos.x as f32 * 13.0 + pos.y as f32 * 37.0 + pos.z as f32 * 19.0 + time.elapsed_seconds() * 100.0).sin().abs();
        if pseudo_rand < 0.001 { // 0.1% chance per tick (was 0.5%)
            let down = pos - IVec3::Y;
            if let Some(&neighbor) = voxel_world.blocks.get(&down) {
                if let Ok(block_type) = block_type_query.get(neighbor) {
                    if [0, 1, 7].contains(&block_type.0) {
                        // Spawn Particles
                        if let Ok(mat_handle) = block_material_query.get(neighbor) {
                            for i in 0..3 { // Reduced from 5 to 3 particles
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
                        commands.entity(neighbor).despawn_recursive();
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

                if do_split && liq.level > 1 { // Only split if level > 1
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

                        // Update neighbors (safely handle despawned entities)
                        for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                            if let Some(&e) = voxel_world.blocks.get(&(target + dir)) {
                                if let Some(mut ec) = commands.get_entity(e) {
                                    ec.insert(NeedsMeshUpdate);
                                }
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

        processed_count += 1;
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
            // Update neighbors (safely handle despawned entities)
            for dir in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                if let Some(&e) = voxel_world.blocks.get(&(pos + dir)) {
                    if let Some(mut ec) = commands.get_entity(e) {
                        ec.insert(NeedsMeshUpdate);
                    }
                }
            }
            commands.entity(entity).despawn_recursive();
        }
    }
}

fn sand_dynamics(
    mut commands: Commands,
    mut voxel_world: ResMut<VoxelWorld>,
    voxel_assets: Res<VoxelAssets>,
    mut query: Query<(Entity, &mut Transform, &BlockType)>,
    time: Res<Time>,
    mut sand_update_timer: Local<Option<Timer>>,
) {
    // Initialize the timer if it hasn't been initialized yet
    if sand_update_timer.is_none() {
        *sand_update_timer = Some(Timer::from_seconds(0.016, TimerMode::Repeating)); // ~60 FPS update rate for better responsiveness
    }

    let timer = sand_update_timer.as_mut().unwrap();

    // Only update sand dynamics periodically to reduce performance impact
    if !timer.tick(time.delta()).just_finished() {
        return;
    }

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

    // Process all sand blocks for proper physics simulation
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

                    // Add visual feedback for sand movement
                    spawn_sand_movement_particles(&mut commands, (*voxel_assets).clone(), pos.as_vec3());
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

                            // Add visual feedback for sand movement
                            spawn_sand_movement_particles(&mut commands, (*voxel_assets).clone(), pos.as_vec3());

                            break;
                        }
                    }
                }
            }
        }
    }
}

// Function to spawn particles when sand blocks move
fn spawn_sand_movement_particles(
    commands: &mut Commands,
    voxel_assets: VoxelAssets,
    position: Vec3,
) {
    // Spawn a few particles to indicate sand movement
    for i in 0..3 {
        let r1 = (position.x + i as f32 * 0.23).sin();
        let r2 = (position.y + i as f32 * 0.45).cos();
        let r3 = (position.z + i as f32 * 0.67).sin();

        commands.spawn((
            PbrBundle {
                mesh: voxel_assets.mesh.clone(),
                material: voxel_assets.block_types[7].clone(), // Sand material
                transform: Transform::from_xyz(
                    position.x + 0.5 + r1 * 0.3,
                    position.y + 0.5 + r2.abs() * 0.3,
                    position.z + 0.5 + r3 * 0.3
                ).with_scale(Vec3::splat(0.15)),
                ..default()
            },
            Particle {
                lifetime: Timer::from_seconds(0.3, TimerMode::Once),
                velocity: Vec3::new(r1 * 1.0, r2.abs() * 2.0 + 1.0, r3 * 1.0),
            }
        ));
    }
}

fn update_block_cracks(
    mut commands: Commands,
    mut query: Query<(Entity, &BlockType, &BlockDurability), Changed<BlockDurability>>,
    voxel_assets: Res<VoxelAssets>,
    mut material_query: Query<&mut Handle<StandardMaterial>>,
) {
    for (entity, block_type, durability) in query.iter_mut() {
        // Only update blocks that have durability and can be damaged
        if durability.max_durability > 0.0 {
            // Determine which material to use based on crack level
            let material_to_use = if durability.crack_level > 0 {
                // Use cracked material if crack level > 0
                if block_type.0 < voxel_assets.cracked_block_types.len() {
                    voxel_assets.cracked_block_types[block_type.0].clone()
                } else {
                    // Fallback to original material if no cracked version exists
                    if block_type.0 < voxel_assets.block_types.len() {
                        voxel_assets.block_types[block_type.0].clone()
                    } else {
                        continue; // Skip if invalid block type
                    }
                }
            } else {
                // Use original material if no cracks
                if block_type.0 < voxel_assets.block_types.len() {
                    voxel_assets.block_types[block_type.0].clone()
                } else {
                    continue; // Skip if invalid block type
                }
            };

            // Update the material of the block
            if let Ok(mut material_handle) = material_query.get_mut(entity) {
                *material_handle = material_to_use;
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
                commands.entity(entity).despawn_recursive();
            }
            voxel_world.blocks.clear();
            voxel_world.chunks.clear();

            let reader = BufReader::new(file);
            if let Ok(saved_blocks) = serde_json::from_reader::<_, Vec<SavedBlock>>(reader) {
                for block in saved_blocks {
                    let pos = IVec3::new(block.x, block.y, block.z);
                    if let Some(mat) = voxel_assets.block_types.get(block.type_index) {
                        let mesh = if block.type_index == 4 && block.level > 0 {
                            voxel_assets.water_meshes[8].clone()
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

                        let id = entity_cmds.id();
                        
                        voxel_world.blocks.insert(pos, id);
                        
                        let chunk_coord = IVec2::new(
                            (pos.x as f32 / CHUNK_SIZE as f32).floor() as i32,
                            (pos.z as f32 / CHUNK_SIZE as f32).floor() as i32,
                        );
                        voxel_world.chunks.entry(chunk_coord).or_default().push(pos);
                    }
                }
                info!("World loaded from world.json");
            }
        }
    }
}
