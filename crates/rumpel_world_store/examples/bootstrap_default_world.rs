use rumpel_world_store::{
    default_terrain_seed_from_env, resolve_world_id_from_env, save_root_from_env,
    world_reset_requested_from_env, WorldDatabase,
};
use rumpel_world::world_gen::{init_active_world_terrain, terrain_generation_contract_version};

fn main() {
    let save_root = save_root_from_env();
    if world_reset_requested_from_env() {
        let world_id = resolve_world_id_from_env();
        let world_dir = save_root.join(&world_id);
        if world_dir.exists() {
            std::fs::remove_dir_all(&world_dir).expect("remove world directory");
            eprintln!("removed world directory {}", world_dir.display());
        }
    }
    let world_id = resolve_world_id_from_env();
    let terrain_seed = default_terrain_seed_from_env();
    let (database, loaded) =
        WorldDatabase::open_or_create_world(&save_root, &world_id, terrain_seed).expect("open world");
    init_active_world_terrain(loaded.meta.terrain_seed);
    eprintln!(
        "world={} seed={} contract={} path={} purged_chunks={}",
        loaded.meta.world_id,
        loaded.meta.terrain_seed,
        terrain_generation_contract_version(),
        database.path().display(),
        loaded.purged_stale_chunks,
    );
}
