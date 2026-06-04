use bevy::app::AppExit;
use bevy::prelude::*;
use rumpel_prelude::{BlockRegistry, ChunkPos};
use rumpel_player::Player;
use rumpel_world::chunk::{CHUNK_SIZE, WorldEditStore};
use rumpel_world::chunk_disk::{
    enqueue_edited_chunks, flush_pending_chunks, install_chunk_persistence, mark_chunk_pending,
};
use rumpel_world::world_gen::{
    terrain_generation_contract_version, WorldGenerationContext,
};
use rumpel_world_store::{
    log_world_list_if_requested, resolve_world_id_from_env, upsert_world_index, DatabaseChunkPersistence,
    LoadedWorld, OpenWorldError, WorldDatabase, WorldMeta, default_terrain_seed_from_env,
    save_root_from_env,
};
use rumpel_world::{chunk_gen_cache::reset_chunk_generation_cache, world_gen::init_active_world_terrain};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const AUTOSAVE_INTERVAL_SECS: f32 = 30.0;

#[derive(Resource)]
pub struct ActiveWorldSave {
    pub database: Arc<WorldDatabase>,
    pub meta: WorldMeta,
    pub save_root: PathBuf,
}

pub fn open_active_world_save() -> Result<ActiveWorldSave, OpenWorldError> {
    let save_root = save_root_from_env();
    log_world_list_if_requested(&save_root);
    let world_id = resolve_world_id_from_env();
    let terrain_seed = default_terrain_seed_from_env();
    let (database, LoadedWorld {
        meta,
        edits,
        purged_stale_chunks,
    }) = WorldDatabase::open_or_create_world(&save_root, &world_id, terrain_seed)?;

    init_active_world_terrain(meta.terrain_seed);
    reset_chunk_generation_cache();

    let active = ActiveWorldSave {
        database: Arc::new(database),
        meta,
        save_root,
    };
    if purged_stale_chunks > 0 {
        info!(
            purged_stale_chunks,
            contract_version = active.meta.contract_version,
            "purged stored chunks after terrain contract change"
        );
    }
    info!(
        world_id = active.meta.world_id.as_str(),
        terrain_seed = active.meta.terrain_seed,
        contract_version = active.meta.contract_version,
        save_path = %active.database.path().display(),
        stored_edits = edits.edits.len(),
        "world save opened"
    );
    Ok(active)
}

pub fn plugin(app: &mut App) {
    app.init_resource::<AutosaveTimer>()
        .add_systems(Startup, (apply_saved_edits_to_world, install_chunk_disk_bridge))
        .add_systems(
            Update,
            (
                autosave_world
                    .run_if(in_state(rumpel_prelude::GameState::InGame)),
                flush_world_on_app_exit,
            ),
        );
}

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self(Timer::new(
            Duration::from_secs_f32(AUTOSAVE_INTERVAL_SECS),
            TimerMode::Repeating,
        ))
    }
}

fn install_chunk_disk_bridge(save: Res<ActiveWorldSave>) {
    let contract = terrain_generation_contract_version();
    install_chunk_persistence(
        DatabaseChunkPersistence::new(save.database.clone(), contract).into_arc(),
    );
}

fn apply_saved_edits_to_world(
    save: Res<ActiveWorldSave>,
    mut edit_store: ResMut<WorldEditStore>,
) {
    let Ok(edits) = save.database.load_edits() else {
        return;
    };
    if edits.edits.is_empty() {
        return;
    }
    edits.apply_to_store(&mut edit_store);
    enqueue_edited_chunks(&edit_store);
    info!(
        stored_edits = edits.edits.len(),
        generation = edits.generation,
        "restored world block edits from save"
    );
}

fn autosave_world(
    time: Res<Time>,
    mut timer: ResMut<AutosaveTimer>,
    mut save: ResMut<ActiveWorldSave>,
    edit_store: Res<WorldEditStore>,
    registry: Res<BlockRegistry>,
    player: Query<&Transform, With<Player>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    capture_player_position(&mut save.meta, &player);
    mark_player_chunk_visited(&player);
    let context = WorldGenerationContext::from_registry(&registry);
    let database = Arc::clone(&save.database);
    let save_root = save.save_root.clone();
    persist_world(
        database.as_ref(),
        &save_root,
        &mut save.meta,
        &edit_store,
        &context,
        registry.as_ref(),
        "autosave",
    );
}

fn flush_world_on_app_exit(
    mut exits: MessageReader<AppExit>,
    mut save: ResMut<ActiveWorldSave>,
    edit_store: Res<WorldEditStore>,
    registry: Res<BlockRegistry>,
    player: Query<&Transform, With<Player>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    capture_player_position(&mut save.meta, &player);
    mark_player_chunk_visited(&player);
    let context = WorldGenerationContext::from_registry(&registry);
    let database = Arc::clone(&save.database);
    let save_root = save.save_root.clone();
    persist_world(
        database.as_ref(),
        &save_root,
        &mut save.meta,
        &edit_store,
        &context,
        registry.as_ref(),
        "app exit",
    );
}

fn capture_player_position(meta: &mut WorldMeta, player: &Query<&Transform, With<Player>>) {
    let Ok(transform) = player.single() else {
        return;
    };
    meta.set_player_position([
        transform.translation.x,
        transform.translation.y,
        transform.translation.z,
    ]);
}

fn mark_player_chunk_visited(player: &Query<&Transform, With<Player>>) {
    let Ok(transform) = player.single() else {
        return;
    };
    let chunk_x = transform.translation.x.div_euclid(CHUNK_SIZE as f32) as i32;
    let chunk_z = transform.translation.z.div_euclid(CHUNK_SIZE as f32) as i32;
    mark_chunk_pending(ChunkPos::new(chunk_x, chunk_z));
}

fn persist_world(
    database: &WorldDatabase,
    save_root: &PathBuf,
    meta: &mut WorldMeta,
    edit_store: &WorldEditStore,
    context: &WorldGenerationContext,
    registry: &BlockRegistry,
    reason: &str,
) {
    meta.contract_version = terrain_generation_contract_version();
    meta.touch_updated();
    enqueue_edited_chunks(edit_store);
    let flushed_chunks = flush_pending_chunks(context, edit_store, registry);
    match database.save_world(meta, edit_store) {
        Ok(()) => {
            if let Err(error) = upsert_world_index(save_root, meta) {
                error!(reason, ?error, "world index update failed");
            }
            info!(
                reason,
                world_id = meta.world_id.as_str(),
                edits = edit_store.len(),
                flushed_chunks,
                pending_chunks = rumpel_world::chunk_disk::pending_chunk_count(),
                has_player_position = meta.has_player_position,
                "world save flushed"
            );
        }
        Err(error) => error!(reason, ?error, "world save flush failed"),
    }
}

#[must_use]
pub fn spawn_position_from_save(meta: &WorldMeta) -> Option<Vec3> {
    meta.player_position().map(|[x, y, z]| Vec3::new(x, y, z))
}
