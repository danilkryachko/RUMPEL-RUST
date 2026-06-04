use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowMode};
use rumpel_prelude::*;

pub mod chat;
pub mod mobs;
pub mod particles;

const PLAYER_MOVE_SPEED: f32 = 60.0;
const PLAYER_WALK_SPEED: f32 = 8.0;
const PLAYER_SPRINT_MULTIPLIER: f32 = 1.65;
const PLAYER_SURFACE_CLEARANCE: f32 = 3.0;
const PLAYER_BLOCK_REACH: f32 = 12.0;
/// Minecraft Java standing hitbox height (1.8 blocks).
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Eye height from foot level (slightly above vanilla Minecraft 1.62).
pub const PLAYER_EYE_HEIGHT: f32 = 1.74;
pub const PLAYER_FEET_SURFACE_EPSILON: f32 = 0.001;
const CAMERA_LOCK_ENV: &str = "RUMPEL_CAMERA_LOCK";
const PACKED_CAMERA_LOCK_ENV: &str = "RUMPEL_PACKED_CAMERA_LOCK";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayerGameMode {
    #[default]
    Survival,
    Creative,
}

impl PlayerGameMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Survival => "Выживание",
            Self::Creative => "Креатив",
        }
    }

    #[must_use]
    pub fn is_creative(self) -> bool {
        matches!(self, Self::Creative)
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Component)]
struct CrosshairHud;

#[derive(Component)]
pub struct PlayerPhysics {
    pub game_mode: PlayerGameMode,
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
            .init_resource::<rumpel_world::world_blocks::WorldBlocks>()
            .init_resource::<mobs::LuaMobManager>()
            .add_plugins(chat::RumpelChatPlugin)
            .add_systems(
                OnEnter(GameState::InGame),
                (lock_game_cursor, spawn_crosshair, snap_player_on_enter),
            )
            .add_systems(
                Update,
                (
                    init_player_components,
                    player_look,
                    player_move,
                    maintain_player_camera_rig.after(player_move),
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
            game_mode: PlayerGameMode::Survival,
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
    mut query: Query<(&mut Transform, &mut PlayerPhysics), With<Player>>,
    mut world_blocks: ResMut<rumpel_world::world_blocks::WorldBlocks>,
    edit_store: Res<WorldEditStore>,
    registry: Option<Res<BlockRegistry>>,
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

    let Ok((mut transform, mut physics)) = query.single_mut() else {
        return;
    };

    if keyboard_input.just_pressed(KeyCode::KeyF) && !modifier_key_held(&keyboard_input) {
        physics.game_mode = match physics.game_mode {
            PlayerGameMode::Survival => PlayerGameMode::Creative,
            PlayerGameMode::Creative => PlayerGameMode::Survival,
        };
        physics.velocity = Vec3::ZERO;
        physics.mining_target = None;
        physics.mining_progress = 0.0;
        physics.is_grounded = false;

        if physics.game_mode == PlayerGameMode::Survival {
            snap_player_to_surface(
                &mut transform,
                &mut world_blocks,
                &edit_store,
                registry.as_deref(),
            );
        }

        info!(
            "PLAYER: Режим игры: {} (F). {}",
            physics.game_mode.label(),
            if physics.game_mode.is_creative() {
                "Полёт: Space вверх, Shift вниз."
            } else {
                "Ходьба: WASD, Space прыжок, Ctrl бег."
            }
        );
    }
}

fn snap_player_to_surface(
    transform: &mut Transform,
    world_blocks: &mut rumpel_world::world_blocks::WorldBlocks,
    edit_store: &WorldEditStore,
    registry: Option<&BlockRegistry>,
) {
    let Some(registry) = registry else {
        return;
    };
    let wx = transform.translation.x.floor() as i32;
    let wz = transform.translation.z.floor() as i32;
    let mut surface_top = 0;
    for y in 0..96 {
        if world_blocks.is_solid_at_world(IVec3::new(wx, y, wz), edit_store, registry) {
            surface_top = y + 1;
        }
    }
    if surface_top > 0 {
        transform.translation.y = surface_top as f32 + PLAYER_FEET_SURFACE_EPSILON;
        return;
    }
    transform.translation.y =
        rumpel_world::world_gen::terrain_height_at(wx, wz) as f32 + PLAYER_FEET_SURFACE_EPSILON;
}

pub fn snap_player_on_enter(
    mut query: Query<&mut Transform, With<Player>>,
    mut world_blocks: ResMut<rumpel_world::world_blocks::WorldBlocks>,
    edit_store: Res<WorldEditStore>,
    registry: Option<Res<BlockRegistry>>,
) {
    if camera_lock_enabled() {
        return;
    }
    let Ok(mut transform) = query.single_mut() else {
        return;
    };
    snap_player_to_surface(
        &mut transform,
        &mut world_blocks,
        &edit_store,
        registry.as_deref(),
    );
}

pub fn maintain_player_camera_rig(mut camera_query: Query<&mut Transform, With<PlayerCamera>>) {
    for mut camera_transform in camera_query.iter_mut() {
        camera_transform.translation = Vec3::new(0.0, PLAYER_EYE_HEIGHT, 0.0);
    }
}

pub fn lock_game_cursor(mut cursor_query: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if camera_lock_enabled() {
        return;
    }
    let Ok(mut cursor_options) = cursor_query.single_mut() else {
        return;
    };
    cursor_options.grab_mode = CursorGrabMode::Locked;
    cursor_options.visible = false;
}

pub fn spawn_crosshair(mut commands: Commands, camera: Query<Entity, With<PlayerCamera>>) {
    let Ok(cam) = camera.single() else {
        return;
    };
    commands.spawn((
        CrosshairHud,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: px(2),
            height: px(2),
            margin: UiRect::all(px(-1)),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        UiTargetCamera(cam),
    ));
}

fn modifier_key_held(keyboard_input: &ButtonInput<KeyCode>) -> bool {
    keyboard_input.pressed(KeyCode::SuperLeft)
        || keyboard_input.pressed(KeyCode::SuperRight)
        || keyboard_input.pressed(KeyCode::ControlLeft)
        || keyboard_input.pressed(KeyCode::ControlRight)
        || keyboard_input.pressed(KeyCode::AltLeft)
        || keyboard_input.pressed(KeyCode::AltRight)
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
    mut world_blocks: ResMut<rumpel_world::world_blocks::WorldBlocks>,
    edit_store: Res<WorldEditStore>,
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

    if physics.game_mode.is_creative() {
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
        let height = PLAYER_HEIGHT;
        let current_aabb = rumpel_world::physics::Aabb {
            min: transform.translation - Vec3::new(width / 2.0, 0.0, width / 2.0),
            max: transform.translation + Vec3::new(width / 2.0, height, width / 2.0),
        };

        // Check if player is currently submerged in water
        let is_in_water = {
            let mut in_water = false;
            if let Some(ref_reg) = registry.as_ref()
                && let Some(water_id) = ref_reg.get_id("water")
            {
                let min_x = current_aabb.min.x.floor() as i32;
                let max_x = (current_aabb.max.x - 1e-4).floor() as i32;
                let min_y = current_aabb.min.y.floor() as i32;
                let max_y = (current_aabb.max.y - 1e-4).floor() as i32;
                let min_z = current_aabb.min.z.floor() as i32;
                let max_z = (current_aabb.max.z - 1e-4).floor() as i32;

                'water_scan: for x in min_x..=max_x {
                    for y in min_y..=max_y {
                        for z in min_z..=max_z {
                            let block =
                                world_blocks.block_at_world(IVec3::new(x, y, z), &edit_store);
                            if block as BlockId == water_id {
                                in_water = true;
                                break 'water_scan;
                            }
                        }
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

        // Apply horizontal movement (water movement is slower; LeftControl sprints on ground)
        let mut target_speed = if is_in_water { 3.5 } else { PLAYER_WALK_SPEED };
        if physics.is_grounded && !is_in_water && keyboard_input.pressed(KeyCode::ControlLeft) {
            target_speed *= PLAYER_SPRINT_MULTIPLIER;
        }
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

        let mut is_solid_fn = |pos: WorldBlockPos| {
            if let Some(ref_reg) = registry.as_ref() {
                return world_blocks.is_solid_at(pos, &edit_store, ref_reg);
            }
            false
        };

        let movement_step = physics.velocity * delta;
        let allowed_step = rumpel_world::physics::collide_aabb_with_voxels(
            &current_aabb,
            movement_step,
            &mut is_solid_fn,
        );

        // Update player translation based on collision results
        transform.translation += allowed_step;

        // Ground detection
        if movement_step.y < 0.0 && allowed_step.y == 0.0 {
            physics.is_grounded = true;
            physics.velocity.y = 0.0;
        } else if movement_step.y > 0.0 && allowed_step.y == 0.0 {
            physics.is_grounded = false;
            physics.velocity.y = 0.0;
        } else if movement_step.y.abs() < f32::EPSILON && allowed_step.y.abs() < f32::EPSILON {
            // Standing still vertically — probe block below feet
            let below = IVec3::new(
                transform.translation.x.floor() as i32,
                (transform.translation.y - 0.05).floor() as i32,
                transform.translation.z.floor() as i32,
            );
            physics.is_grounded = is_solid_fn(WorldBlockPos::new(below));
            if physics.is_grounded {
                physics.velocity.y = 0.0;
            }
        } else if allowed_step.y.abs() > 0.0001 && movement_step.y > 0.0 {
            physics.is_grounded = false;
        }

        // Wall collisions
        if movement_step.x != 0.0 && allowed_step.x == 0.0 {
            physics.velocity.x = 0.0;
        }
        if movement_step.z != 0.0 && allowed_step.z == 0.0 {
            physics.velocity.z = 0.0;
        }

        // Trigger Lua step behaviors at the block under the player's feet.
        let feet_world = IVec3::new(
            transform.translation.x.floor() as i32,
            (transform.translation.y - 0.1).floor() as i32,
            transform.translation.z.floor() as i32,
        );
        let feet_block_id = world_blocks.block_at_world(feet_world, &edit_store);
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
                let edits = trigger_lua_behavior_with_world(
                    &lua,
                    &mut world_blocks,
                    &edit_store,
                    ref_reg,
                    &block_name,
                    "on_step_on",
                    feet_world,
                );
                for edit in edits {
                    world_blocks.invalidate_chunk(edit.chunk_pos);
                    world_edits.write(edit);
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
    mut world_blocks: ResMut<rumpel_world::world_blocks::WorldBlocks>,
    edit_store: Res<WorldEditStore>,
    single_chunk: Option<ResMut<SingleChunkExtract>>,
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
        return;
    }

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

    let is_solid_fn = |pos: WorldBlockPos| world_blocks.is_solid_at(pos, &edit_store, &registry);

    // Raycast up to 5 blocks
    let hit =
        rumpel_world::physics::raycast_voxels(origin, direction, PLAYER_BLOCK_REACH, is_solid_fn);

    let mut block_broken = false;
    let mut broken_world = IVec3::ZERO;
    let mut broken_val = 0u32;

    if let Some(hit) = hit {
        let world_pos = hit.position.position;
        let block_val = world_blocks.block_at_world(world_pos, &edit_store);

        if mouse_input.pressed(MouseButton::Left) && block_val != 0 {
            let target = world_pos;
            if physics.mining_target != Some(target) {
                physics.mining_target = Some(target);
                physics.mining_progress = 0.0;
            }

            let block_name = registry
                .get_block(block_val)
                .map(|b| b.id.as_str())
                .unwrap_or("air")
                .to_string();

            // Get mining time from Lua
            let mut mining_time = if physics.game_mode.is_creative() {
                0.0
            } else {
                1.8
            };
            if !physics.game_mode.is_creative()
                && let Some(lua_runtime) = lua_runtime.as_ref()
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
            }

            if let Some(lua_runtime) = lua_runtime.as_ref()
                && !physics.game_mode.is_creative()
                && let Ok(lua) = lua_runtime.0.lock()
            {
                let globals = lua.globals();
                let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
                if let Ok(trigger_fn) = trigger {
                    let _ = trigger_fn.call::<()>((
                        block_name.clone(),
                        "on_mine_tick",
                        world_pos.x,
                        world_pos.y,
                        world_pos.z,
                        physics.selected_tool.clone(),
                    ));
                }
            }

            physics.mining_progress += time.delta_secs();

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
                broken_world = world_pos;
                broken_val = u32::from(block_val);
                physics.mining_target = None;
                physics.mining_progress = 0.0;
            }
        }

        if !mouse_input.pressed(MouseButton::Left) {
            physics.mining_target = None;
            physics.mining_progress = 0.0;
        }

        if block_broken {
            let block_name = registry
                .get_block(broken_val as BlockId)
                .map(|b| b.id.as_str())
                .unwrap_or("air")
                .to_string();

            if let Some(edit) =
                rumpel_world::world_blocks::WorldBlocks::set_block_world(broken_world, AIR_BLOCK_ID)
            {
                world_blocks.invalidate_chunk(edit.chunk_pos);
                world_edits.write(edit);
                info!("PLAYER: Broke block at {:?}", broken_world);
            }

            if let Some(lua_runtime) = lua_runtime.as_ref()
                && let Ok(lua) = lua_runtime.0.lock()
            {
                let edits = trigger_lua_behavior_with_world(
                    &lua,
                    &mut world_blocks,
                    &edit_store,
                    &registry,
                    &block_name,
                    "on_block_break",
                    broken_world,
                );
                for edit in edits {
                    world_blocks.invalidate_chunk(edit.chunk_pos);
                    world_edits.write(edit);
                }
            }

            notify_neighbors_world(
                broken_world,
                "air",
                &mut world_blocks,
                &edit_store,
                &registry,
                lua_runtime.as_deref(),
                &mut world_edits,
            );
            sync_single_chunk_extract_optional(&mut world_blocks, &edit_store, single_chunk);
        } else if mouse_input.just_pressed(MouseButton::Right) {
            let place_world = hit.position.position + hit.normal;

            let block_aabb = rumpel_world::physics::Aabb {
                min: Vec3::new(
                    place_world.x as f32,
                    place_world.y as f32,
                    place_world.z as f32,
                ),
                max: Vec3::new(
                    place_world.x as f32 + 1.0,
                    place_world.y as f32 + 1.0,
                    place_world.z as f32 + 1.0,
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

            let place_block = BlockId::try_from(physics.selected_block).unwrap_or(AIR_BLOCK_ID);
            if (physics.game_mode.is_creative() || !overlaps_player)
                && place_block != AIR_BLOCK_ID
                && let Some(edit) = rumpel_world::world_blocks::WorldBlocks::set_block_world(
                    place_world,
                    place_block,
                )
            {
                world_blocks.invalidate_chunk(edit.chunk_pos);
                world_edits.write(edit);
                let placed_name = registry
                    .get_block(place_block)
                    .map(|b| b.id.as_str())
                    .unwrap_or("air")
                    .to_string();
                info!(
                    "PLAYER: Placed block {} at {:?}",
                    physics.selected_block, place_world
                );

                notify_neighbors_world(
                    place_world,
                    &placed_name,
                    &mut world_blocks,
                    &edit_store,
                    &registry,
                    lua_runtime.as_deref(),
                    &mut world_edits,
                );

                if let Some(lua_runtime) = lua_runtime.as_ref()
                    && let Ok(lua) = lua_runtime.0.lock()
                {
                    let edits = trigger_lua_behavior_with_world(
                        &lua,
                        &mut world_blocks,
                        &edit_store,
                        &registry,
                        &placed_name,
                        "on_block_place",
                        place_world,
                    );
                    for edit in edits {
                        world_blocks.invalidate_chunk(edit.chunk_pos);
                        world_edits.write(edit);
                    }
                }

                sync_single_chunk_extract_optional(&mut world_blocks, &edit_store, single_chunk);
            } else if overlaps_player && !physics.game_mode.is_creative() {
                info!("PLAYER: Cannot place block inside player!");
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

fn sync_single_chunk_extract_optional(
    world_blocks: &mut rumpel_world::world_blocks::WorldBlocks,
    edit_store: &WorldEditStore,
    chunk: Option<ResMut<SingleChunkExtract>>,
) {
    if let Some(mut chunk) = chunk {
        world_blocks.sync_chunk_to_single_chunk_extract(
            ChunkPos::new(0, 0),
            edit_store,
            chunk.blocks.as_mut(),
        );
        chunk.has_changes = true;
    }
}

/// How far around a trigger position to pre-read blocks into the Lua snapshot.
/// Covers TNT's blast radius (3) with a two-level chain margin (3 + 3 = 6).
const BEHAVIOR_SNAPSHOT_RADIUS: i32 = 6;

/// Returns `(name_to_id, id_to_name)` maps built from the block registry,
/// always including the "air" ↔ 0 mapping.
fn build_registry_maps(
    registry: &BlockRegistry,
) -> (
    std::collections::HashMap<String, u32>,
    std::collections::HashMap<u32, String>,
) {
    let mut name_to_id = std::collections::HashMap::new();
    let mut id_to_name = std::collections::HashMap::new();
    for raw_id in 0..256u32 {
        if let Some(data) = registry.get_block(raw_id as BlockId) {
            name_to_id.insert(data.id.clone(), raw_id);
            id_to_name.insert(raw_id, data.id.clone());
        }
    }
    name_to_id.insert("air".to_string(), 0);
    id_to_name.insert(0, "air".to_string());
    (name_to_id, id_to_name)
}

/// Installs world-coord `get_block` / `set_block` on the Lua globals.
///
/// `snapshot` is a pre-read map of world positions to block IDs; `get_block`
/// checks the write overlay first, then falls back to this snapshot.
/// `set_block` writes update both the overlay and the returned pending-edits
/// list, so nested Lua calls (e.g. TNT chain reactions) see the up-to-date
/// state through `get_block` without re-entering Rust.
///
/// Returns `(old_get_block, old_set_block, pending_edits)`.  The caller must
/// restore the old globals and flush the pending edits when finished.
fn install_world_block_globals(
    lua: &mlua::Lua,
    snapshot: std::collections::HashMap<IVec3, BlockId>,
    name_to_id: std::collections::HashMap<String, u32>,
    id_to_name: std::collections::HashMap<u32, String>,
) -> (
    mlua::Value,
    mlua::Value,
    std::rc::Rc<std::cell::RefCell<Vec<WorldBlockEdit>>>,
) {
    let globals = lua.globals();
    let prev_get_block: mlua::Value = globals.get("get_block").unwrap_or(mlua::Value::Nil);
    let prev_set_block: mlua::Value = globals.get("set_block").unwrap_or(mlua::Value::Nil);

    let pending_edits =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::<WorldBlockEdit>::new()));
    let overlay = std::rc::Rc::new(std::cell::RefCell::new(
        std::collections::HashMap::<IVec3, BlockId>::new(),
    ));

    // get_block: overlay wins over snapshot; unknown positions → "air".
    let gb_overlay = std::rc::Rc::clone(&overlay);
    let gb_names = id_to_name;
    if let Ok(f) = lua.create_function(move |_, (x, y, z): (i32, i32, i32)| {
        let wp = IVec3::new(x, y, z);
        let id = gb_overlay
            .borrow()
            .get(&wp)
            .copied()
            .unwrap_or_else(|| snapshot.get(&wp).copied().unwrap_or(AIR_BLOCK_ID));
        Ok(gb_names
            .get(&u32::from(id))
            .map(String::as_str)
            .unwrap_or("air")
            .to_string())
    }) {
        let _ = globals.set("get_block", f);
    }

    // set_block: resolve name → id, emit WorldBlockEdit, update overlay.
    let sb_pending = std::rc::Rc::clone(&pending_edits);
    let sb_overlay = std::rc::Rc::clone(&overlay);
    let sb_ids = name_to_id;
    if let Ok(f) = lua.create_function(
        move |_, (x, y, z, name): (i32, i32, i32, String)| {
            let raw_id = sb_ids.get(&name).copied().unwrap_or(0);
            let block_id = BlockId::try_from(raw_id).unwrap_or(AIR_BLOCK_ID);
            let wp = IVec3::new(x, y, z);
            if let Some(edit) =
                rumpel_world::world_blocks::WorldBlocks::set_block_world(wp, block_id)
            {
                sb_overlay.borrow_mut().insert(wp, block_id);
                sb_pending.borrow_mut().push(edit);
            }
            Ok(())
        },
    ) {
        let _ = globals.set("set_block", f);
    }

    (prev_get_block, prev_set_block, pending_edits)
}

/// Pre-reads a cubic snapshot of radius [`BEHAVIOR_SNAPSHOT_RADIUS`] around
/// `pos`, installs world-coord `get_block` / `set_block`, calls
/// `trigger_behavior(block_name, event_name, x, y, z)`, then restores the
/// previous globals and returns all `WorldBlockEdit`s produced by `set_block`.
///
/// Chain reactions (e.g. TNT calling `trigger_behavior` for adjacent blocks)
/// remain entirely within Lua and share the same overlay, so each nested
/// `get_block` correctly reflects previously destroyed blocks.
fn trigger_lua_behavior_with_world(
    lua: &mlua::Lua,
    world_blocks: &mut rumpel_world::world_blocks::WorldBlocks,
    edit_store: &WorldEditStore,
    registry: &BlockRegistry,
    block_name: &str,
    event_name: &str,
    pos: IVec3,
) -> Vec<WorldBlockEdit> {
    let (name_to_id, id_to_name) = build_registry_maps(registry);

    let r = BEHAVIOR_SNAPSHOT_RADIUS;
    let mut snapshot = std::collections::HashMap::<IVec3, BlockId>::new();
    for dx in -r..=r {
        for dy in -r..=r {
            for dz in -r..=r {
                let wp = IVec3::new(pos.x + dx, pos.y + dy, pos.z + dz);
                if wp.y >= 0 {
                    snapshot.insert(wp, world_blocks.block_at_world(wp, edit_store));
                }
            }
        }
    }

    let (prev_gb, prev_sb, pending_edits) =
        install_world_block_globals(lua, snapshot, name_to_id, id_to_name);

    let globals = lua.globals();
    let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
    if let Ok(trigger_fn) = trigger
        && let Err(e) = trigger_fn.call::<()>((
            block_name.to_string(),
            event_name.to_string(),
            pos.x,
            pos.y,
            pos.z,
        ))
    {
        error!(
            "MODS: Error in {} for {}: {:?}",
            event_name, block_name, e
        );
    }

    let _ = globals.set("get_block", prev_gb);
    let _ = globals.set("set_block", prev_sb);

    pending_edits.borrow().clone()
}

fn notify_neighbors_world(
    center: IVec3,
    changed_block_name: &str,
    world_blocks: &mut rumpel_world::world_blocks::WorldBlocks,
    edit_store: &WorldEditStore,
    registry: &BlockRegistry,
    lua_runtime: Option<&rumpel_modding::LuaRuntime>,
    world_edits: &mut MessageWriter<WorldBlockEdit>,
) {
    let Some(lua_runtime) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_runtime.0.lock() else {
        return;
    };

    let (name_to_id, id_to_name) = build_registry_maps(registry);

    let neighbors = [
        center + IVec3::NEG_X,
        center + IVec3::X,
        center + IVec3::NEG_Y,
        center + IVec3::Y,
        center + IVec3::NEG_Z,
        center + IVec3::Z,
    ];

    // Pre-read neighbor blocks; keep a copy for the trigger loop.
    let mut snapshot = std::collections::HashMap::<IVec3, BlockId>::new();
    for neighbor in neighbors {
        snapshot.insert(neighbor, world_blocks.block_at_world(neighbor, edit_store));
    }
    let neighbor_ids = snapshot.clone();

    let id_to_name_for_trigger = id_to_name.clone();
    let (prev_gb, prev_sb, pending_edits) =
        install_world_block_globals(&lua, snapshot, name_to_id, id_to_name);

    let globals = lua.globals();
    let trigger: Result<mlua::Function, _> = globals.get("trigger_behavior");
    if let Ok(trigger_fn) = trigger {
        for neighbor in neighbors {
            let neighbor_id = neighbor_ids.get(&neighbor).copied().unwrap_or(AIR_BLOCK_ID);
            let neighbor_name = id_to_name_for_trigger
                .get(&u32::from(neighbor_id))
                .map(|s| s.as_str())
                .unwrap_or("air");
            if neighbor_name == "air" {
                continue;
            }
            if let Err(e) = trigger_fn.call::<()>((
                neighbor_name.to_string(),
                "on_neighbor_changed",
                neighbor.x,
                neighbor.y,
                neighbor.z,
                center.x,
                center.y,
                center.z,
                changed_block_name.to_string(),
            )) {
                error!("MODS: Error triggering on_neighbor_changed: {:?}", e);
            }
        }
    }

    let _ = globals.set("get_block", prev_gb);
    let _ = globals.set("set_block", prev_sb);

    for edit in pending_edits.borrow().iter().copied() {
        world_blocks.invalidate_chunk(edit.chunk_pos);
        world_edits.write(edit);
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
