use crate::Player;
use bevy::prelude::*;
use mlua::Table;
use rumpel_prelude::*;
use rumpel_world::physics::{Aabb, collide_aabb_with_voxels};

#[derive(Component)]
pub struct LuaMob {
    pub id: u32,
    pub mob_type: String,
    pub velocity: Vec3,
    pub size: Vec3,
    pub on_ground: bool,
}

#[derive(Resource, Default)]
pub struct LuaMobManager {
    pub next_id: u32,
}

pub fn build_cube_mesh(w: f32, h: f32, d: f32) -> Mesh {
    let w = w / 2.0;
    let h = h / 2.0;
    let d = d / 2.0;
    let vertices = [
        // Front
        [-w, -h, d],
        [w, -h, d],
        [w, h, d],
        [-w, h, d],
        // Back
        [-w, -h, -d],
        [-w, h, -d],
        [w, h, -d],
        [w, -h, -d],
        // Top
        [-w, h, d],
        [w, h, d],
        [w, h, -d],
        [-w, h, -d],
        // Bottom
        [-w, -h, d],
        [-w, -h, -d],
        [w, -h, -d],
        [w, -h, d],
        // Right
        [w, -h, d],
        [w, -h, -d],
        [w, h, -d],
        [w, h, d],
        // Left
        [-w, -h, d],
        [-w, h, d],
        [-w, h, -d],
        [-w, -h, -d],
    ];
    let normals = [
        [[0.0, 0.0, 1.0]; 4],
        [[0.0, 0.0, -1.0]; 4],
        [[0.0, 1.0, 0.0]; 4],
        [[0.0, -1.0, 0.0]; 4],
        [[1.0, 0.0, 0.0]; 4],
        [[-1.0, 0.0, 0.0]; 4],
    ]
    .concat();
    let indices = bevy::mesh::Indices::U32(vec![
        0, 1, 2, 0, 2, 3, // Front
        4, 5, 6, 4, 6, 7, // Back
        8, 9, 10, 8, 10, 11, // Top
        12, 13, 14, 12, 14, 15, // Bottom
        16, 17, 18, 16, 18, 19, // Right
        20, 21, 22, 20, 22, 23, // Left
    ]);
    Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertices.to_vec())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(indices)
}

pub fn spawn_lua_mobs_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mob_manager: ResMut<LuaMobManager>,
    lua_runtime: Option<Res<rumpel_modding::LuaRuntime>>,
) {
    let Some(lua_runtime) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_runtime.0.lock() else {
        return;
    };

    let globals = lua.globals();
    let Ok(queue) = globals.get::<Table>("MobSpawnQueue") else {
        return;
    };
    let Ok(mobs_spec) = globals.get::<Table>("Mobs") else {
        return;
    };

    let len = queue.len().unwrap_or(0);
    if len == 0 {
        return;
    }

    let mut spawns = Vec::new();
    for i in 1..=len {
        if let Ok(entry) = queue.get::<Table>(i)
            && let (Ok(mob_type), Ok(x), Ok(y), Ok(z)) = (
                entry.get::<String>("mob_type"),
                entry.get::<f32>("x"),
                entry.get::<f32>("y"),
                entry.get::<f32>("z"),
            )
        {
            spawns.push((mob_type, Vec3::new(x, y, z)));
        }
    }

    // Clear queue in Lua
    if let Err(e) = lua.load("MobSpawnQueue = {}").exec() {
        error!("MODS: Failed to clear MobSpawnQueue: {:?}", e);
        return;
    }

    for (mob_type, pos) in spawns {
        let mut size = Vec3::new(0.8, 0.8, 0.8);
        let mut color = Color::srgb(0.2, 0.8, 0.2);

        // Lookup specs inside dynamic Lua mob configuration
        if let Ok(spec) = mobs_spec.get::<Table>(mob_type.clone()) {
            if let Ok(color_table) = spec.get::<Table>("color")
                && let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                    color_table.get::<f32>(1),
                    color_table.get::<f32>(2),
                    color_table.get::<f32>(3),
                    color_table.get::<f32>(4),
                )
            {
                color = Color::srgba(r, g, b, a);
            }
            if let Ok(size_table) = spec.get::<Table>("size")
                && let (Ok(sx), Ok(sy), Ok(sz)) = (
                    size_table.get::<f32>(1),
                    size_table.get::<f32>(2),
                    size_table.get::<f32>(3),
                )
            {
                size = Vec3::new(sx, sy, sz);
            }
        }

        mob_manager.next_id += 1;
        let id = mob_manager.next_id;

        // Spawn standard PBR meshes
        commands.spawn((
            LuaMob {
                id,
                mob_type: mob_type.clone(),
                velocity: Vec3::ZERO,
                size,
                on_ground: false,
            },
            Mesh3d(meshes.add(build_cube_mesh(size.x, size.y, size.z))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: false,
                ..default()
            })),
            Transform::from_translation(pos),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));

        info!(
            "MODS: Spawned Lua mob '{}' with ID {} at {:?}",
            mob_type, id, pos
        );

        // Trigger spawn callback in Lua
        if let Ok(trigger_func) = globals.get::<mlua::Function>("trigger_mob_spawn") {
            let _ = trigger_func.call::<()>((id, mob_type));
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy mob update system coordinates ECS commands, world collision state, Lua runtime, and time."
)]
pub fn update_lua_mobs_system(
    mut commands: Commands,
    time: Res<Time>,
    chunk: Option<Res<SingleChunkExtract>>,
    registry: Option<Res<BlockRegistry>>,
    player_query: Query<&Transform, With<Player>>,
    mut mob_query: Query<(Entity, &mut Transform, &mut LuaMob), Without<Player>>,
    lua_runtime: Option<Res<rumpel_modding::LuaRuntime>>,
    mut rumpel_time: ResMut<RumpelTime>,
) {
    let Some(lua_runtime) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_runtime.0.lock() else {
        return;
    };

    let globals = lua.globals();
    let Ok(mob_states) = globals.get::<Table>("MobStates") else {
        return;
    };
    let Ok(player_state) = globals.get::<Table>("PlayerState") else {
        return;
    };
    let Ok(time_state) = globals.get::<Table>("TimeState") else {
        return;
    };

    // Calculate dynamic time ticking from Lua's state
    let dt = time.delta_secs();
    let current_elapsed: f32 = time_state
        .get("elapsed_time")
        .unwrap_or(rumpel_time.elapsed_time);
    let time_scale: f32 = time_state.get("time_scale").unwrap_or(1.0);

    let new_elapsed = current_elapsed + dt * time_scale * 0.052;
    let new_sun_angle = new_elapsed.sin();

    let _ = time_state.set("elapsed_time", new_elapsed);
    let _ = time_state.set("sun_angle", new_sun_angle);

    // Sync back to Rust resource
    rumpel_time.elapsed_time = new_elapsed;
    rumpel_time.sun_angle = new_sun_angle;

    let is_raining: bool = time_state.get("is_raining").unwrap_or(false);
    rumpel_time.is_raining = is_raining;

    let lightning_flash: f32 = time_state.get("lightning_flash").unwrap_or(0.0);
    if lightning_flash > 0.0 {
        rumpel_time.lightning_flash = lightning_flash;
        let _ = time_state.set("lightning_flash", 0.0);
    }

    // 1. Feed player pos to Lua
    if let Some(player_transform) = player_query.iter().next() {
        let _ = player_state.set("x", player_transform.translation.x);
        let _ = player_state.set("y", player_transform.translation.y);
        let _ = player_state.set("z", player_transform.translation.z);
    }

    // 2. Feed each active mob state to Lua, trigger update, and retrieve updated velocities
    let trigger_fn = globals.get::<mlua::Function>("trigger_mob_update").ok();

    for (_, transform, mut mob) in &mut mob_query {
        let mob_table = match mob_states.get::<Table>(mob.id) {
            Ok(t) => t,
            Err(_) => {
                let t = lua.create_table().unwrap();
                let _ = mob_states.set(mob.id, t.clone());
                t
            }
        };

        let _ = mob_table.set("x", transform.translation.x);
        let _ = mob_table.set("y", transform.translation.y);
        let _ = mob_table.set("z", transform.translation.z);
        let _ = mob_table.set("vx", mob.velocity.x);
        let _ = mob_table.set("vy", mob.velocity.y);
        let _ = mob_table.set("vz", mob.velocity.z);
        let _ = mob_table.set("on_ground", mob.on_ground);

        if let Some(ref func) = trigger_fn
            && let Err(e) = func.call::<()>((mob.id, mob.mob_type.clone()))
        {
            error!("MODS: Error updating mob {} AI: {:?}", mob.id, e);
        }

        // Retrieve updated velocity
        if let Ok(updated_vx) = mob_table.get::<f32>("vx") {
            mob.velocity.x = updated_vx;
        }
        if let Ok(updated_vy) = mob_table.get::<f32>("vy") {
            mob.velocity.y = updated_vy;
        }
        if let Ok(updated_vz) = mob_table.get::<f32>("vz") {
            mob.velocity.z = updated_vz;
        }
    }

    // 3. Trigger global world tick callback in Lua (handles downpour down rain particles & nighttime spawner)
    let delta_time = time.delta_secs();
    if let Ok(world_tick_fn) = globals.get::<mlua::Function>("trigger_world_tick")
        && let Err(e) = world_tick_fn.call::<()>(delta_time)
    {
        error!("MODS: Error triggering trigger_world_tick: {:?}", e);
    }

    // 4. Consume MobDespawnQueue from Lua to despawn entities in Bevy
    let mut despawned_entities = std::collections::HashSet::new();
    if let Ok(despawn_queue) = globals.get::<Table>("MobDespawnQueue") {
        let d_len = despawn_queue.len().unwrap_or(0);
        if d_len > 0 {
            let mut despawn_ids = Vec::new();
            for i in 1..=d_len {
                if let Ok(id) = despawn_queue.get::<u32>(i) {
                    despawn_ids.push(id);
                }
            }
            // Clear queue in Lua
            let _ = lua.load("MobDespawnQueue = {}").exec();

            // Despawn Bevy entities
            for id in despawn_ids {
                for (entity, _, mob) in &mut mob_query {
                    if mob.id == id {
                        commands.entity(entity).despawn();
                        despawned_entities.insert(entity);
                        info!(
                            "MODS: Despawned mob ID {} ({}) via Lua request",
                            id, mob.mob_type
                        );
                    }
                }
            }
        }
    }

    // 5. Apply standard gravity and voxel block collisions inside Rust
    let Some(chunk) = chunk else {
        return;
    };
    let Some(registry) = registry else {
        return;
    };

    let is_solid_fn = |pos: WorldBlockPos| {
        let x = pos.position.x;
        let y = pos.position.y;
        let z = pos.position.z;
        if (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z) {
            let idx = (x + y * 32 + z * 32 * 32) as usize;
            let id = chunk.blocks[idx] as BlockId;
            if id == 0 {
                return false;
            }
            if let Some(block) = registry.get_block(id) {
                return block.is_solid;
            }
        }
        false
    };

    for (entity, mut transform, mut mob) in &mut mob_query {
        // Skip physics for entities that were just marked as despawned
        if despawned_entities.contains(&entity) {
            continue;
        }

        // Apply gravity
        mob.velocity.y -= 18.0 * delta_time;

        // Limit terminal falling speed
        if mob.velocity.y < -35.0 {
            mob.velocity.y = -35.0;
        }

        let initial_pos = transform.translation;
        let movement = mob.velocity * delta_time;

        // Resolve collisions using AABB
        let mob_aabb = Aabb {
            min: initial_pos - mob.size / 2.0,
            max: initial_pos + mob.size / 2.0,
        };

        let allowed_movement = collide_aabb_with_voxels(&mob_aabb, movement, is_solid_fn);

        transform.translation += allowed_movement;

        // Check grounding (if y-velocity resolution results in stopping downward speed)
        if movement.y < 0.0 && allowed_movement.y == 0.0 {
            mob.on_ground = true;
            mob.velocity.y = 0.0;
        } else {
            mob.on_ground = false;
        }

        // Stop horizontal velocity if hit a wall
        if movement.x != 0.0 && allowed_movement.x == 0.0 {
            mob.velocity.x = 0.0;
        }
        if movement.z != 0.0 && allowed_movement.z == 0.0 {
            mob.velocity.z = 0.0;
        }
    }
}
