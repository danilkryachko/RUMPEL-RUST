use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowMode};
use rumpel_prelude::*;

pub mod chat;
pub mod mobs;
pub mod particles;

const PLAYER_MOVE_SPEED: f32 = 60.0;
const PLAYER_SURFACE_CLEARANCE: f32 = 3.0;
const CAMERA_LOCK_ENV: &str = "RUMPEL_CAMERA_LOCK";
const PACKED_CAMERA_LOCK_ENV: &str = "RUMPEL_PACKED_CAMERA_LOCK";

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Component)]
pub struct PlayerPhysics {
    pub is_flying: bool,
    pub velocity: Vec3,
    pub is_grounded: bool,
    pub selected_block: u32,
    pub selected_tool: String,
    pub mining_target: Option<IVec3>,
    pub mining_progress: f32,
}

pub struct RumpelPlayerPlugin;

impl Plugin for RumpelPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WorldBlockEdit>()
            .init_resource::<WorldEditStore>()
            .init_resource::<mobs::LuaMobManager>()
            .add_plugins(chat::RumpelChatPlugin)
            .add_systems(
                Update,
                (
                    init_player_components,
                    player_look,
                    player_move,
                    player_physics_toggle,
                    player_interaction,
                    sand_gravity_system,
                    water_spread_system,
                    cursor_grab_system,
                    toggle_fullscreen_system,
                    mobs::spawn_lua_mobs_system,
                    mobs::update_lua_mobs_system,
                    particles::spawn_voxel_particles_system,
                    particles::update_voxel_particles_system,
                )
                    .run_if(in_state(GameState::InGame)),
            )
            .add_systems(
                Update,
                record_world_block_edits
                    .after(player_move)
                    .after(player_interaction)
                    .after(sand_gravity_system)
                    .after(water_spread_system)
                    .run_if(in_state(GameState::InGame)),
            );
    }
}

pub fn init_player_components(
    mut commands: Commands,
    query: Query<Entity, (With<Player>, Without<PlayerPhysics>)>,
) {
    for entity in query.iter() {
        commands.entity(entity).insert(PlayerPhysics {
            is_flying: true,
            velocity: Vec3::ZERO,
            is_grounded: false,
            selected_block: 3, // Default to stone (id 3)
            selected_tool: "hand".to_string(),
            mining_target: None,
            mining_progress: 0.0,
        });
        info!("PLAYER: Initialized PlayerPhysics and block selection on Player entity");
    }
}

pub fn player_look(
    mut mouse_motion_events: MessageReader<MouseMotion>,
    cursor_query: Query<&CursorOptions, With<PrimaryWindow>>,
    mut query: Query<&mut Transform, With<PlayerCamera>>,
) {
    if camera_lock_enabled() {
        return;
    }

    let Ok(cursor_options) = cursor_query.single() else {
        return;
    };

    if cursor_options.grab_mode != CursorGrabMode::Locked {
        return;
    }

    let mut delta = Vec2::ZERO;
    for event in mouse_motion_events.read() {
        delta += event.delta;
    }

    if delta == Vec2::ZERO {
        return;
    }

    let sensitivity = 0.003;
    for mut transform in query.iter_mut() {
        let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        yaw -= delta.x * sensitivity;
        pitch -= delta.y * sensitivity;

        // Ограничиваем наклон вверх/вниз
        pitch = pitch.clamp(-1.54, 1.54);

        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }
}

pub fn player_physics_toggle(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut PlayerPhysics, With<Player>>,
    chat_state: Option<Res<chat::ChatState>>,
) {
    if camera_lock_enabled() {
        return;
    }

    if let Some(chat) = chat_state.as_ref()
        && chat.is_open
    {
        return;
    }

    let Ok(mut physics) = query.single_mut() else {
        return;
    };

    if keyboard_input.just_pressed(KeyCode::KeyF) {
        physics.is_flying = !physics.is_flying;
        physics.velocity = Vec3::ZERO;
        info!("PLAYER: Toggled fly mode to: {}", physics.is_flying);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy movement system needs independent ECS params for input, world state, Lua hooks, and chat focus."
)]
pub fn player_move(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut PlayerPhysics), With<Player>>,
    camera_query: Query<&Transform, (With<PlayerCamera>, Without<Player>)>,
    single_chunk: Option<ResMut<SingleChunkExtract>>,
    registry: Option<Res<BlockRegistry>>,
    lua_runtime: Option<Res<rumpel_modding::LuaRuntime>>,
    chat_state: Option<Res<chat::ChatState>>,
    mut world_edits: MessageWriter<WorldBlockEdit>,
) {
    if camera_lock_enabled() {
        return;
    }

    if let Some(chat) = chat_state.as_ref()
        && chat.is_open
    {
        return; // Freeze movement while typing!
    }

    let Ok((mut transform, mut physics)) = query.single_mut() else {
        return;
    };
    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    let delta = time.delta_secs();

    if physics.is_flying {
        // Creative flight keeps look direction separate from altitude control.
        let mut direction = Vec3::ZERO;
        let forward: Vec3 = *camera_transform.forward();
        let right: Vec3 = *camera_transform.right();
        let forward_flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        let right_flat = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();

        if keyboard_input.pressed(KeyCode::KeyW) {
            direction += forward_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyS) {
            direction -= forward_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyA) {
            direction -= right_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyD) {
            direction += right_flat;
        }

        if keyboard_input.pressed(KeyCode::Space) {
            direction += Vec3::Y;
        }
        if keyboard_input.pressed(KeyCode::ShiftLeft) {
            direction -= Vec3::Y;
        }

        if direction != Vec3::ZERO {
            transform.translation += direction.normalize() * PLAYER_MOVE_SPEED * delta;
        }

        let surface_y = rumpel_world::world_gen::terrain_height_at(
            transform.translation.x.floor() as i32,
            transform.translation.z.floor() as i32,
        ) as f32
            + PLAYER_SURFACE_CLEARANCE;
        if transform.translation.y < surface_y {
            transform.translation.y = surface_y;
        }
    } else {
        // Voxel Physics Mode (Gravity & Collisions)
        let forward: Vec3 = *camera_transform.forward();
        let right: Vec3 = *camera_transform.right();
        let mut move_dir = Vec3::ZERO;

        // Project movement onto the horizontal XZ plane
        let forward_flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        let right_flat = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();

        // Create player AABB centered on translation
        let width = 0.6;
        let height = 1.8;
        let current_aabb = rumpel_world::physics::Aabb {
            min: transform.translation - Vec3::new(width / 2.0, 0.0, width / 2.0),
            max: transform.translation + Vec3::new(width / 2.0, height, width / 2.0),
        };

        // Check if player is currently submerged in water
        let is_in_water = {
            let mut in_water = false;
            if let Some(chunk) = single_chunk.as_ref()
                && let Some(water_id) = registry.as_ref().and_then(|reg| reg.get_id("water"))
            {
                let min_x = current_aabb.min.x.floor() as i32;
                let max_x = (current_aabb.max.x - 1e-4).floor() as i32;
                let min_y = current_aabb.min.y.floor() as i32;
                let max_y = (current_aabb.max.y - 1e-4).floor() as i32;
                let min_z = current_aabb.min.z.floor() as i32;
                let max_z = (current_aabb.max.z - 1e-4).floor() as i32;

                for x in min_x..=max_x {
                    for y in min_y..=max_y {
                        for z in min_z..=max_z {
                            if (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z)
                            {
                                let idx = (x + y * 32 + z * 32 * 32) as usize;
                                if chunk.blocks[idx] as BlockId == water_id {
                                    in_water = true;
                                    break;
                                }
                            }
                        }
                        if in_water {
                            break;
                        }
                    }
                    if in_water {
                        break;
                    }
                }
            }
            in_water
        };

        if keyboard_input.pressed(KeyCode::KeyW) {
            move_dir += forward_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyS) {
            move_dir -= forward_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyA) {
            move_dir -= right_flat;
        }
        if keyboard_input.pressed(KeyCode::KeyD) {
            move_dir += right_flat;
        }

        // Apply horizontal movement (water movement is slower)
        let target_speed = if is_in_water { 3.5 } else { 8.0 };
        if move_dir != Vec3::ZERO {
            let desired_vel = move_dir.normalize() * target_speed;
            physics.velocity.x = desired_vel.x;
            physics.velocity.z = desired_vel.z;
        } else {
            // Apply drag (water drag could also be applied here if desired)
            physics.velocity.x -= physics.velocity.x * 10.0 * delta;
            physics.velocity.z -= physics.velocity.z * 10.0 * delta;
        }

        // Swimming & Gravity forces
        if is_in_water {
            if keyboard_input.pressed(KeyCode::Space) {
                physics.velocity.y = 2.5; // Swim up
            } else if keyboard_input.pressed(KeyCode::ShiftLeft) {
                physics.velocity.y = -2.5; // Swim down
            } else {
                // Gentle passive sinking inside water
                physics.velocity.y -= 6.0 * delta;
                physics.velocity.y = physics.velocity.y.max(-1.2);
            }
            physics.is_grounded = false;
        } else {
            // Jump
            if keyboard_input.pressed(KeyCode::Space) && physics.is_grounded {
                physics.velocity.y = 8.5; // Jump strength
                physics.is_grounded = false;
            }

            // Gravity
            if !physics.is_grounded {
                physics.velocity.y -= 26.0 * delta; // Gravity strength
                physics.velocity.y = physics.velocity.y.max(-45.0); // Terminal velocity
            }
        }

        // Single chunk voxel lookup function for collision detection
        let single_chunk_ref = single_chunk.as_ref().map(|r| r.as_ref());
        let registry_ref = registry.as_ref().map(|r| r.as_ref());

        let is_solid_fn = |pos: WorldBlockPos| {
            if let Some(chunk) = single_chunk_ref {
                let x = pos.position.x;
                let y = pos.position.y;
                let z = pos.position.z;
                if (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z) {
                    let block_idx = (x + y * 32 + z * 32 * 32) as usize;
                    let block_id = chunk.blocks[block_idx] as BlockId;
                    if block_id == 0 {
                        return false;
                    }
                    if let Some(data) = registry_ref.and_then(|reg| reg.get_block(block_id)) {
                        return data.is_solid;
                    }
                }
            }
            false
        };

        let movement_step = physics.velocity * delta;
        let allowed_step = rumpel_world::physics::collide_aabb_with_voxels(
            &current_aabb,
            movement_step,
            is_solid_fn,
        );

        // Update player translation based on collision results
        transform.translation += allowed_step;

        // Ground detection: if tried to move down but stopped, we are grounded
        if movement_step.y < 0.0 && allowed_step.y >= 0.0 {
            physics.is_grounded = true;
            physics.velocity.y = 0.0;
        } else {
            // If we moved down successfully or are flying upwards, we aren't grounded
            if allowed_step.y != movement_step.y && movement_step.y < 0.0 {
                physics.is_grounded = true;
                physics.velocity.y = 0.0;
            } else if allowed_step.y.abs() > 0.0001 {
                physics.is_grounded = false;
            }
        }

        // Wall collisions
        if movement_step.x != 0.0 && allowed_step.x == 0.0 {
            physics.velocity.x = 0.0;
        }
        if movement_step.z != 0.0 && allowed_step.z == 0.0 {
            physics.velocity.z = 0.0;
        }

        // Trigger Lua Step Behaviors
        let feet_x = transform.translation.x.floor() as i32;
        let feet_y = (transform.translation.y - 0.1).floor() as i32;
        let feet_z = transform.translation.z.floor() as i32;

        if (0..32).contains(&feet_x) && (0..32).contains(&feet_y) && (0..32).contains(&feet_z) {
            let feet_idx = (feet_x + feet_y * 32 + feet_z * 32 * 32) as usize;
            if let Some(mut chunk) = single_chunk {
                let feet_block_id = chunk.blocks[feet_idx] as BlockId;
                if feet_block_id != 0
                    && let Some(ref_reg) = registry.as_ref()
                {
                    let block_name = ref_reg
                        .get_block(feet_block_id)
                        .map(|b| b.id.as_str())
                        .unwrap_or("air")
                        .to_string();

                    if block_name != "air"
                        && let Some(lua_runtime) = lua_runtime.as_ref()
                        && let Ok(lua) = lua_runtime.0.lock()
                    {
                        let globals = lua.globals();

                        // Copy block data to static buffer for safety in Lua closures
                        let blocks_before = *chunk.blocks.clone();
                        let blocks_cell = std::rc::Rc::new(std::cell::RefCell::new(blocks_before));

                        // Build lookup tables
                        let mut name_to_id = std::collections::HashMap::new();
                        let mut id_to_name = std::collections::HashMap::new();
                        for id in 0..150 {
                            if let Some(block_data) = ref_reg.get_block(id) {
                                name_to_id.insert(block_data.id.clone(), id as u32);
                                id_to_name.insert(id as u32, block_data.id.clone());
                            }
                        }
                        name_to_id.insert("air".to_string(), 0);
                        id_to_name.insert(0, "air".to_string());

                        let get_block_buffer = std::rc::Rc::clone(&blocks_cell);
                        let get_block_names = id_to_name.clone();
                        let get_block =
                            lua.create_function(move |_, (x, y, z): (usize, usize, usize)| {
                                if x < 32 && y < 32 && z < 32 {
                                    let idx = x + y * 32 + z * 32 * 32;
                                    let id = get_block_buffer.borrow()[idx];
                                    let name = get_block_names
                                        .get(&id)
                                        .map(|s| s.as_str())
                                        .unwrap_or("air");
                                    Ok(name.to_string())
                                } else {
                                    Ok("air".to_string())
                                }
                            });

                        if let Ok(f) = get_block {
                            let _ = globals.set("get_block", f);
                        }

                        let set_block_buffer = std::rc::Rc::clone(&blocks_cell);
                        let set_block_ids = name_to_id.clone();
                        let set_block = lua.create_function(
                            move |_, (x, y, z, name): (usize, usize, usize, String)| {
                                if x < 32 && y < 32 && z < 32 {
                                    let idx = x + y * 32 + z * 32 * 32;
                                    let id = set_block_ids.get(&name).copied().unwrap_or(0);
                                    set_block_buffer.borrow_mut()[idx] = id;
                                }
                                Ok(())
                            },
                        );

                        if let Ok(f) = set_block {
                            let _ = globals.set("set_block", f);
                        }

                        // Call trigger_behavior(block_name, "on_step_on", feet_x, feet_y, feet_z)
                        let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
                        if let Ok(trigger_fn) = trigger {
                            let _ = trigger_fn.call::<()>((
                                block_name.clone(),
                                "on_step_on",
                                feet_x,
                                feet_y,
                                feet_z,
                            ));
                        }

                        // Copy changes back and trigger meshing if changed.
                        let final_blocks = *blocks_cell.borrow();
                        if blocks_before != final_blocks {
                            emit_single_chunk_block_edits(
                                &blocks_before,
                                &final_blocks,
                                &mut world_edits,
                            );
                            *chunk.blocks = final_blocks;
                            chunk.has_changes = true;
                        }
                    }
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy systems receive independent ECS params directly for scheduler access."
)]
pub fn player_interaction(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    camera_query: Query<&Transform, With<PlayerCamera>>,
    chunk: Option<ResMut<SingleChunkExtract>>,
    registry: Option<Res<BlockRegistry>>,
    mut physics_query: Query<&mut PlayerPhysics, With<Player>>,
    lua_runtime: Option<Res<rumpel_modding::LuaRuntime>>,
    time: Res<Time>,
    chat_state: Option<Res<chat::ChatState>>,
    mut world_edits: MessageWriter<WorldBlockEdit>,
) {
    if camera_lock_enabled() {
        return;
    }

    if let Some(chat) = chat_state.as_ref()
        && chat.is_open
    {
        return; // Freeze interactions while typing!
    }

    let Some(mut chunk) = chunk else {
        return;
    };
    let Some(registry) = registry else {
        return;
    };
    let Ok(camera_transform) = camera_query.single() else {
        return;
    };
    let Ok(mut physics) = physics_query.single_mut() else {
        return;
    };

    // Hotbar block selection (1-6)
    if keyboard_input.just_pressed(KeyCode::Digit1) {
        physics.selected_block = 2; // Grass
        info!("PLAYER: Selected block Grass (ID: 2)");
    } else if keyboard_input.just_pressed(KeyCode::Digit2) {
        physics.selected_block = 1; // Dirt
        info!("PLAYER: Selected block Dirt (ID: 1)");
    } else if keyboard_input.just_pressed(KeyCode::Digit3) {
        physics.selected_block = 3; // Stone
        info!("PLAYER: Selected block Stone (ID: 3)");
    } else if keyboard_input.just_pressed(KeyCode::Digit4) {
        physics.selected_block = 4; // Sand
        info!("PLAYER: Selected block Sand (ID: 4)");
    } else if keyboard_input.just_pressed(KeyCode::Digit5) {
        physics.selected_block = 5; // Wood (Log)
        info!("PLAYER: Selected block Wood Log (ID: 5)");
    } else if keyboard_input.just_pressed(KeyCode::Digit6) {
        if let Some(water_id) = registry.get_id("water") {
            physics.selected_block = water_id as u32;
            info!("PLAYER: Selected block Water (ID: {})", water_id);
        } else {
            warn!("PLAYER: Water block not found in BlockRegistry!");
        }
    }

    // Tool selection (7: Pickaxe, 8: Axe, 9: Shovel)
    if keyboard_input.just_pressed(KeyCode::Digit7) {
        physics.selected_tool = "pickaxe".to_string();
        info!("PLAYER: Selected tool: Pickaxe");
    } else if keyboard_input.just_pressed(KeyCode::Digit8) {
        physics.selected_tool = "axe".to_string();
        info!("PLAYER: Selected tool: Axe");
    } else if keyboard_input.just_pressed(KeyCode::Digit9) {
        physics.selected_tool = "shovel".to_string();
        info!("PLAYER: Selected tool: Shovel");
    }

    let origin = camera_transform.translation;
    let direction: Vec3 = *camera_transform.forward();

    let is_solid_fn = |pos: WorldBlockPos| {
        let x = pos.position.x;
        let y = pos.position.y;
        let z = pos.position.z;
        if (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z) {
            let block_idx = (x + y * 32 + z * 32 * 32) as usize;
            let block_id = chunk.blocks[block_idx] as BlockId;
            if block_id == 0 {
                return false;
            }
            if let Some(data) = registry.get_block(block_id) {
                return data.is_solid;
            }
        }
        false
    };

    // Raycast up to 5 blocks
    let hit = rumpel_world::physics::raycast_voxels(origin, direction, 5.0, is_solid_fn);

    let mut block_broken = false;
    let mut broken_pos = IVec3::ZERO;
    let mut broken_val = 0u32;

    if let Some(hit) = hit {
        let pos = hit.position.position;
        let block_idx = (pos.x + pos.y * 32 + pos.z * 32 * 32) as usize;
        let block_val = chunk.blocks[block_idx];

        if mouse_input.pressed(MouseButton::Left) && block_val != 0 {
            let target = IVec3::new(pos.x, pos.y, pos.z);
            if physics.mining_target != Some(target) {
                physics.mining_target = Some(target);
                physics.mining_progress = 0.0;
            }

            let block_name = registry
                .get_block(block_val as BlockId)
                .map(|b| b.id.as_str())
                .unwrap_or("air")
                .to_string();

            // Get mining time from Lua
            let mut mining_time = 1.8; // Default hand mining (slow)
            if let Some(lua_runtime) = lua_runtime.as_ref()
                && let Ok(lua) = lua_runtime.0.lock()
            {
                let globals = lua.globals();
                let get_mining_time: Result<mlua::Function, _> = globals.get("get_mining_time");
                if let Ok(func) = get_mining_time
                    && let Ok(time) =
                        func.call::<f32>((block_name.clone(), physics.selected_tool.clone()))
                {
                    mining_time = time;
                }

                // Trigger on_mine_tick callback to let Lua mod spawn particles
                let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
                if let Ok(trigger_fn) = trigger {
                    let _ = trigger_fn.call::<()>((
                        block_name.clone(),
                        "on_mine_tick",
                        pos.x,
                        pos.y,
                        pos.z,
                        physics.selected_tool.clone(),
                    ));
                }
            }

            physics.mining_progress += time.delta_secs();

            // Display mining progress periodically
            if (physics.mining_progress * 4.0) as i32
                != ((physics.mining_progress - time.delta_secs()) * 4.0) as i32
            {
                info!(
                    "PLAYER: Mining progress: {:.1}s / {:.1}s (Tool: {})",
                    physics.mining_progress, mining_time, physics.selected_tool
                );
            }

            if physics.mining_progress >= mining_time {
                block_broken = true;
                broken_pos = pos;
                broken_val = block_val;
                physics.mining_target = None;
                physics.mining_progress = 0.0;
            }
        }

        if !mouse_input.pressed(MouseButton::Left) {
            physics.mining_target = None;
            physics.mining_progress = 0.0;
        }

        if block_broken {
            let blocks_before = *chunk.blocks.clone();
            let block_name = registry
                .get_block(broken_val as BlockId)
                .map(|b| b.id.as_str())
                .unwrap_or("air")
                .to_string();

            let broken_idx = (broken_pos.x + broken_pos.y * 32 + broken_pos.z * 32 * 32) as usize;
            chunk.blocks[broken_idx] = 0;
            chunk.has_changes = true;
            info!("PLAYER: Broke block at {:?}", broken_pos);

            // Trigger Lua custom behavior for this block
            if let Some(lua_runtime) = lua_runtime.as_ref()
                && let Ok(lua) = lua_runtime.0.lock()
            {
                let globals = lua.globals();

                // Copy block data to static buffer for safety in Lua closures
                let blocks_buffer = *chunk.blocks.clone();
                let blocks_cell = std::rc::Rc::new(std::cell::RefCell::new(blocks_buffer));

                // Build lookup tables
                let mut name_to_id = std::collections::HashMap::new();
                let mut id_to_name = std::collections::HashMap::new();
                for id in 0..150 {
                    if let Some(block_data) = registry.get_block(id) {
                        name_to_id.insert(block_data.id.clone(), id as u32);
                        id_to_name.insert(id as u32, block_data.id.clone());
                    }
                }
                name_to_id.insert("air".to_string(), 0);
                id_to_name.insert(0, "air".to_string());

                let get_block_buffer = std::rc::Rc::clone(&blocks_cell);
                let get_block_names = id_to_name.clone();
                let get_block = lua.create_function(move |_, (x, y, z): (usize, usize, usize)| {
                    if x < 32 && y < 32 && z < 32 {
                        let idx = x + y * 32 + z * 32 * 32;
                        let id = get_block_buffer.borrow()[idx];
                        let name = get_block_names
                            .get(&id)
                            .map(|s| s.as_str())
                            .unwrap_or("air");
                        Ok(name.to_string())
                    } else {
                        Ok("air".to_string())
                    }
                });

                if let Ok(f) = get_block {
                    let _ = globals.set("get_block", f);
                }

                let set_block_buffer = std::rc::Rc::clone(&blocks_cell);
                let set_block_ids = name_to_id.clone();
                let set_block = lua.create_function(
                    move |_, (x, y, z, name): (usize, usize, usize, String)| {
                        if x < 32 && y < 32 && z < 32 {
                            let idx = x + y * 32 + z * 32 * 32;
                            let id = set_block_ids.get(&name).copied().unwrap_or(0);
                            set_block_buffer.borrow_mut()[idx] = id;
                        }
                        Ok(())
                    },
                );

                if let Ok(f) = set_block {
                    let _ = globals.set("set_block", f);
                }

                // Call trigger_behavior(block_name, "on_broken", x, y, z)
                let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
                if let Ok(trigger_fn) = trigger
                    && let Err(e) = trigger_fn.call::<()>((
                        block_name,
                        "on_broken",
                        broken_pos.x,
                        broken_pos.y,
                        broken_pos.z,
                    ))
                {
                    error!("MODS: Error triggering on_broken callback: {:?}", e);
                }

                // Commit changes
                *chunk.blocks = *blocks_cell.borrow();
            }

            // Notify neighbors of block breaking
            notify_neighbors(
                &mut chunk.blocks,
                &registry,
                lua_runtime.as_deref(),
                broken_pos.x,
                broken_pos.y,
                broken_pos.z,
                "air",
            );
            emit_single_chunk_block_edits(&blocks_before, chunk.blocks.as_ref(), &mut world_edits);
        } else if mouse_input.just_pressed(MouseButton::Right) {
            // Place block
            let place_pos = hit.position.position + hit.normal;
            if (0..32).contains(&place_pos.x)
                && (0..32).contains(&place_pos.y)
                && (0..32).contains(&place_pos.z)
            {
                let block_idx = (place_pos.x + place_pos.y * 32 + place_pos.z * 32 * 32) as usize;

                // Do not place inside player's feet or head
                let block_aabb = rumpel_world::physics::Aabb {
                    min: Vec3::new(place_pos.x as f32, place_pos.y as f32, place_pos.z as f32),
                    max: Vec3::new(
                        place_pos.x as f32 + 1.0,
                        place_pos.y as f32 + 1.0,
                        place_pos.z as f32 + 1.0,
                    ),
                };

                let player_camera_pos = camera_transform.translation;
                let feet_pos = player_camera_pos - Vec3::new(0.0, 1.5, 0.0);

                let player_aabb = rumpel_world::physics::Aabb {
                    min: feet_pos - Vec3::new(0.3, 0.1, 0.3),
                    max: player_camera_pos + Vec3::new(0.3, 0.3, 0.3),
                };

                let overlaps_player = player_aabb.min.x < block_aabb.max.x
                    && player_aabb.max.x > block_aabb.min.x
                    && player_aabb.min.y < block_aabb.max.y
                    && player_aabb.max.y > block_aabb.min.y
                    && player_aabb.min.z < block_aabb.max.z
                    && player_aabb.max.z > block_aabb.min.z;

                if physics.is_flying || !overlaps_player {
                    let blocks_before = *chunk.blocks.clone();
                    chunk.blocks[block_idx] = physics.selected_block;
                    chunk.has_changes = true;
                    let placed_name = registry
                        .get_block(physics.selected_block as BlockId)
                        .map(|b| b.id.as_str())
                        .unwrap_or("air")
                        .to_string();
                    info!(
                        "PLAYER: Placed block {} at {:?}",
                        physics.selected_block, place_pos
                    );

                    // Notify neighbors of block placing
                    notify_neighbors(
                        &mut chunk.blocks,
                        &registry,
                        lua_runtime.as_deref(),
                        place_pos.x,
                        place_pos.y,
                        place_pos.z,
                        &placed_name,
                    );
                    emit_single_chunk_block_edits(
                        &blocks_before,
                        chunk.blocks.as_ref(),
                        &mut world_edits,
                    );
                } else {
                    info!("PLAYER: Cannot place block inside player!");
                }
            }
        }
    }
}

fn camera_lock_enabled() -> bool {
    env_flag(CAMERA_LOCK_ENV) || env_flag(PACKED_CAMERA_LOCK_ENV)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

pub fn cursor_grab_system(
    mut cursor_query: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut cursor_options) = cursor_query.single_mut() else {
        return;
    };

    if mouse.just_pressed(MouseButton::Left) {
        cursor_options.grab_mode = CursorGrabMode::Locked;
        cursor_options.visible = false;
    }

    if key.just_pressed(KeyCode::Escape) {
        cursor_options.grab_mode = CursorGrabMode::None;
        cursor_options.visible = true;
    }
}

pub fn toggle_fullscreen_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut window_query: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = window_query.single_mut() else {
        return;
    };

    let alt_enter =
        keyboard_input.pressed(KeyCode::AltLeft) || keyboard_input.pressed(KeyCode::AltRight);
    let cmd_f =
        keyboard_input.pressed(KeyCode::SuperLeft) || keyboard_input.pressed(KeyCode::SuperRight);

    let toggle = (alt_enter && keyboard_input.just_pressed(KeyCode::Enter))
        || (cmd_f && keyboard_input.just_pressed(KeyCode::KeyF))
        || keyboard_input.just_pressed(KeyCode::F11);

    if toggle {
        window.mode = match window.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
            _ => WindowMode::Windowed,
        };
        info!("PLAYER: Toggled window mode to {:?}", window.mode);
    }
}

pub fn sand_gravity_system(
    time: Res<Time>,
    mut timer: Local<f32>,
    chunk: Option<ResMut<SingleChunkExtract>>,
    registry: Option<Res<BlockRegistry>>,
    mut world_edits: MessageWriter<WorldBlockEdit>,
) {
    let Some(mut chunk) = chunk else {
        return;
    };
    let Some(registry) = registry else {
        return;
    };

    *timer += time.delta_secs();
    if *timer < 0.12 {
        return;
    }
    *timer = 0.0;

    let water_id = registry.get_id("water").map(|id| id as u32).unwrap_or(999);
    let blocks_before = *chunk.blocks.clone();
    let mut moved = false;

    // Run gravity checks from bottom to top (y=1 to y=31) so columns fall smoothly block-by-block
    for y in 1..32 {
        for x in 0..32 {
            for z in 0..32 {
                let idx = x + y * 32 + z * 32 * 32;
                let block_id = chunk.blocks[idx] as BlockId;
                if block_id == 0 {
                    continue;
                }

                // Query block registry for gravity affected property
                let is_gravity = if let Some(data) = registry.get_block(block_id) {
                    data.gravity_affected
                } else {
                    false
                };

                if is_gravity {
                    let below_idx = x + (y - 1) * 32 + z * 32 * 32;
                    let below_block = chunk.blocks[below_idx];

                    // If block below is Air (ID 0)
                    if below_block == 0 {
                        chunk.blocks[below_idx] = block_id as u32; // Move block down
                        chunk.blocks[idx] = 0; // Set old pos to Air
                        moved = true;
                    } else if below_block == water_id {
                        // Swap gravity block and water (fluid displacement)
                        chunk.blocks[below_idx] = block_id as u32;
                        chunk.blocks[idx] = water_id;
                        moved = true;
                    }
                }
            }
        }
    }

    if moved {
        chunk.has_changes = true;
        emit_single_chunk_block_edits(&blocks_before, chunk.blocks.as_ref(), &mut world_edits);
    }
}

pub fn water_spread_system(
    time: Res<Time>,
    mut timer: Local<f32>,
    chunk: Option<ResMut<SingleChunkExtract>>,
    registry: Option<Res<BlockRegistry>>,
    mut world_edits: MessageWriter<WorldBlockEdit>,
) {
    let Some(mut chunk) = chunk else {
        return;
    };
    let Some(registry) = registry else {
        return;
    };

    *timer += time.delta_secs();
    if *timer < 0.12 {
        return;
    }
    *timer = 0.0;

    let water_id = match registry.get_id("water") {
        Some(id) => id as u32,
        None => return,
    };
    let blocks_before = *chunk.blocks.clone();

    // 1. Initialize our levels array: 0 means no water, 1-8 are water levels.
    // Level 8 is the source water (or vertically flowing water).
    let mut levels = [0u8; 32768];

    // BFS queue: store coordinates and the water level.
    let mut queue = std::collections::VecDeque::with_capacity(1024);

    // 2. Identify sources: water blocks that are NOT under another water block.
    // A player-placed water block is a source.
    for y in 0..32 {
        for x in 0..32 {
            for z in 0..32 {
                let idx = x + y * 32 + z * 32 * 32;
                let current_block = chunk.blocks[idx];
                if current_block == water_id {
                    let has_water_above = if y == 31 {
                        false
                    } else {
                        chunk.blocks[x + (y + 1) * 32 + z * 32 * 32] == water_id
                    };

                    if !has_water_above {
                        levels[idx] = 8;
                        queue.push_back((x, y, z, 8u8));
                    }
                }
            }
        }
    }

    // 3. Helper to check if a block is solid
    let is_solid_fn = |bx: usize, by: usize, bz: usize| -> bool {
        let block = chunk.blocks[bx + by * 32 + bz * 32 * 32];
        if block == 0 || block == water_id {
            return false;
        }
        if let Some(data) = registry.get_block(block as BlockId) {
            return data.is_solid;
        }
        false
    };

    // 4. BFS propagation
    while let Some((x, y, z, level)) = queue.pop_front() {
        // Downward propagation
        if y > 0 {
            let below_idx = x + (y - 1) * 32 + z * 32 * 32;
            let below_block = chunk.blocks[below_idx];
            if below_block == 0 || below_block == water_id {
                // If it goes down, it flows at full strength (level 8)
                if levels[below_idx] < 8 {
                    levels[below_idx] = 8;
                    queue.push_back((x, y - 1, z, 8));
                }
            }
        }

        // Horizontal propagation if the block below is solid
        let below_solid = y == 0 || is_solid_fn(x, y - 1, z);

        if below_solid && level > 1 {
            let next_level = level - 1;
            let neighbors = [
                (x.wrapping_sub(1), y, z),
                (x + 1, y, z),
                (x, y, z.wrapping_sub(1)),
                (x, y, z + 1),
            ];

            for (nx, ny, nz) in neighbors {
                if nx < 32 && nz < 32 {
                    let n_idx = nx + ny * 32 + nz * 32 * 32;
                    let n_block = chunk.blocks[n_idx];
                    if (n_block == 0 || n_block == water_id) && levels[n_idx] < next_level {
                        levels[n_idx] = next_level;
                        queue.push_back((nx, ny, nz, next_level));
                    }
                }
            }
        }
    }

    // 5. Commit computed levels to chunk
    let mut changed = false;
    for y in 0..32 {
        for x in 0..32 {
            for z in 0..32 {
                let idx = x + y * 32 + z * 32 * 32;
                let current_block = chunk.blocks[idx];
                let level = levels[idx];

                if level > 0 {
                    if current_block == 0 {
                        chunk.blocks[idx] = water_id;
                        changed = true;
                    }
                } else {
                    if current_block == water_id {
                        chunk.blocks[idx] = 0;
                        changed = true;
                    }
                }
            }
        }
    }

    if changed {
        chunk.has_changes = true;
        emit_single_chunk_block_edits(&blocks_before, chunk.blocks.as_ref(), &mut world_edits);
    }
}

fn emit_single_chunk_block_edits(
    before: &[u32; CHUNK_VOLUME],
    after: &[u32; CHUNK_VOLUME],
    world_edits: &mut MessageWriter<WorldBlockEdit>,
) -> usize {
    visit_single_chunk_block_edits(before, after, |edit| {
        world_edits.write(edit);
    })
}

fn visit_single_chunk_block_edits(
    before: &[u32; CHUNK_VOLUME],
    after: &[u32; CHUNK_VOLUME],
    mut visit: impl FnMut(WorldBlockEdit),
) -> usize {
    let mut emitted = 0;
    for (index, (&previous, &current)) in before.iter().zip(after.iter()).enumerate() {
        if previous == current {
            continue;
        }

        let Ok(block) = BlockId::try_from(current) else {
            continue;
        };
        let Some(edit) = WorldBlockEdit::from_single_chunk_index(index, block) else {
            continue;
        };

        visit(edit);
        emitted += 1;
    }
    emitted
}

pub fn notify_neighbors(
    chunk_blocks: &mut [u32; 32768],
    registry: &BlockRegistry,
    lua_runtime: Option<&rumpel_modding::LuaRuntime>,
    x: i32,
    y: i32,
    z: i32,
    changed_block_name: &str,
) {
    let Some(lua_runtime) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_runtime.0.lock() else {
        return;
    };

    let globals = lua.globals();

    // 1. Copy block data to static buffer for safety in Lua closures
    let blocks_buffer = *chunk_blocks;
    let blocks_cell = std::rc::Rc::new(std::cell::RefCell::new(blocks_buffer));

    // 2. Build lookup tables
    let mut name_to_id = std::collections::HashMap::new();
    let mut id_to_name = std::collections::HashMap::new();
    for id in 0..150 {
        if let Some(block_data) = registry.get_block(id) {
            name_to_id.insert(block_data.id.clone(), id as u32);
            id_to_name.insert(id as u32, block_data.id.clone());
        }
    }
    name_to_id.insert("air".to_string(), 0);
    id_to_name.insert(0, "air".to_string());

    let get_block_buffer = std::rc::Rc::clone(&blocks_cell);
    let get_block_names = id_to_name.clone();
    let get_block = lua.create_function(move |_, (cx, cy, cz): (usize, usize, usize)| {
        if cx < 32 && cy < 32 && cz < 32 {
            let idx = cx + cy * 32 + cz * 32 * 32;
            let id = get_block_buffer.borrow()[idx];
            let name = get_block_names
                .get(&id)
                .map(|s| s.as_str())
                .unwrap_or("air");
            Ok(name.to_string())
        } else {
            Ok("air".to_string())
        }
    });

    if let Ok(f) = get_block {
        let _ = globals.set("get_block", f);
    }

    let set_block_buffer = std::rc::Rc::clone(&blocks_cell);
    let set_block_ids = name_to_id.clone();
    let set_block = lua.create_function(
        move |_, (cx, cy, cz, name): (usize, usize, usize, String)| {
            if cx < 32 && cy < 32 && cz < 32 {
                let idx = cx + cy * 32 + cz * 32 * 32;
                let id = set_block_ids.get(&name).copied().unwrap_or(0);
                set_block_buffer.borrow_mut()[idx] = id;
            }
            Ok(())
        },
    );

    if let Ok(f) = set_block {
        let _ = globals.set("set_block", f);
    }

    // 3. Notify 6 neighbors
    let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
    if let Ok(trigger_fn) = trigger {
        let neighbors = [
            (x - 1, y, z),
            (x + 1, y, z),
            (x, y - 1, z),
            (x, y + 1, z),
            (x, y, z - 1),
            (x, y, z + 1),
        ];

        for (nx, ny, nz) in neighbors {
            if (0..32).contains(&nx) && (0..32).contains(&ny) && (0..32).contains(&nz) {
                let n_idx = (nx + ny * 32 + nz * 32 * 32) as usize;
                let neighbor_id = blocks_cell.borrow()[n_idx];
                let neighbor_name = id_to_name
                    .get(&neighbor_id)
                    .map(|s| s.as_str())
                    .unwrap_or("air");

                if neighbor_name != "air"
                    && let Err(e) = trigger_fn.call::<()>((
                        neighbor_name.to_string(),
                        "on_neighbor_changed",
                        nx,
                        ny,
                        nz,
                        x,
                        y,
                        z,
                        changed_block_name.to_string(),
                    ))
                {
                    error!("MODS: Error triggering on_neighbor_changed: {:?}", e);
                }
            }
        }
    }

    // 4. Commit changes back to blocks slice
    *chunk_blocks = *blocks_cell.borrow();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk_diff_emits_typed_world_block_edits() {
        let mut before = [0; CHUNK_VOLUME];
        let mut after = [0; CHUNK_VOLUME];
        before[ChunkData::get_index(1, 2, 3)] = 3;
        after[ChunkData::get_index(1, 2, 3)] = 0;
        after[ChunkData::get_index(4, 5, 6)] = 7;

        let mut edits = Vec::new();
        let emitted = visit_single_chunk_block_edits(&before, &after, |edit| edits.push(edit));

        assert_eq!(emitted, 2);
        assert_eq!(
            edits,
            vec![
                WorldBlockEdit::new(ChunkPos { x: 0, z: 0 }, LocalBlockPos::new(1, 2, 3), 0),
                WorldBlockEdit::new(ChunkPos { x: 0, z: 0 }, LocalBlockPos::new(4, 5, 6), 7),
            ]
        );
    }

    #[test]
    fn single_chunk_diff_ignores_block_ids_outside_registry_word_size() {
        let before = [0; CHUNK_VOLUME];
        let mut after = [0; CHUNK_VOLUME];
        after[ChunkData::get_index(1, 2, 3)] = u32::from(BlockId::MAX) + 1;

        let mut edits = Vec::new();
        let emitted = visit_single_chunk_block_edits(&before, &after, |edit| edits.push(edit));

        assert_eq!(emitted, 0);
        assert!(edits.is_empty());
    }
}
