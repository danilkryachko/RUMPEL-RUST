use std::path::{Path, PathBuf};

use rocksdb::{Options, DB};
use rumpel_world::world_gen::terrain_generation_contract_version;

use rumpel_coords::ChunkPos;
use rumpel_world::chunk::{ChunkData, WorldEditStore};
use rumpel_world::world_gen::{GeneratedChunk, WorldGenerationContext, generate_chunk_uncached};

use crate::chunks::{
    chunk_blob_format_supported, chunk_blob_matches_contract, chunk_storage_key, StoredChunkBlob,
    CHUNK_STORAGE_KEY_PREFIX,
};
use rumpel_blocks::BlockRegistry;
use rumpel_world::surface_decor::chunk_decor_output_uncapped;
use crate::edits::StoredEdits;
use crate::index::{list_world_ids, upsert_world_index};
use crate::meta::{
    unix_timestamp_now, WorldMeta, DEFAULT_TERRAIN_SEED, DEFAULT_WORLD_ID,
    WORLD_META_FORMAT_VERSION, WORLD_META_FORMAT_VERSION_V1,
};

const KEY_META: &[u8] = b"meta:v1";
const KEY_EDITS: &[u8] = b"edits:v1";

#[derive(Debug)]
pub struct WorldDatabase {
    db: DB,
    path: PathBuf,
}

#[derive(Debug)]
pub enum OpenWorldError {
    Io(std::io::Error),
    RocksDb(rocksdb::Error),
    MetaDecode(String),
    EditsDecode(String),
    ChunkDecode(String),
}

impl std::fmt::Display for OpenWorldError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::RocksDb(error) => write!(formatter, "rocksdb error: {error}"),
            Self::MetaDecode(message) => write!(formatter, "world meta decode failed: {message}"),
            Self::EditsDecode(message) => write!(formatter, "world edits decode failed: {message}"),
            Self::ChunkDecode(message) => write!(formatter, "world chunk decode failed: {message}"),
        }
    }
}

impl std::error::Error for OpenWorldError {}

impl From<rocksdb::Error> for OpenWorldError {
    fn from(error: rocksdb::Error) -> Self {
        Self::RocksDb(error)
    }
}

#[derive(Clone, Debug)]
pub struct LoadedWorld {
    pub meta: WorldMeta,
    pub edits: StoredEdits,
    pub purged_stale_chunks: usize,
}

impl WorldDatabase {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open_or_create_world(
        save_root: impl AsRef<Path>,
        world_id: &str,
        terrain_seed: u32,
    ) -> Result<(Self, LoadedWorld), OpenWorldError> {
        let world_dir = save_root.as_ref().join(sanitize_world_id(world_id));
        if world_reset_requested_from_env() && world_dir.exists() {
            std::fs::remove_dir_all(&world_dir).map_err(OpenWorldError::Io)?;
        }
        std::fs::create_dir_all(&world_dir).map_err(OpenWorldError::Io)?;

        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, &world_dir).map_err(OpenWorldError::RocksDb)?;
        let database = Self {
            db,
            path: world_dir,
        };

        if let Some(meta_bytes) = database.db.get(KEY_META).map_err(OpenWorldError::RocksDb)? {
            let mut meta = decode_meta(&meta_bytes).map_err(OpenWorldError::MetaDecode)?;
            let edits = database.load_edits()?;
            let purged_stale_chunks = database.reconcile_terrain_contract(&mut meta)?;
            return Ok((
                database,
                LoadedWorld {
                    meta,
                    edits,
                    purged_stale_chunks,
                },
            ));
        }

        let meta = WorldMeta::new(world_id, terrain_seed, terrain_generation_contract_version());
        let edits = StoredEdits {
            generation: 0,
            edits: Vec::new(),
        };
        database.write_meta(&meta)?;
        database.write_edits(&edits)?;
        upsert_world_index(save_root.as_ref(), &meta).map_err(|error| {
            OpenWorldError::MetaDecode(format!("world index update failed: {error}"))
        })?;
        Ok((
            database,
            LoadedWorld {
                meta,
                edits,
                purged_stale_chunks: 0,
            },
        ))
    }

    pub fn reconcile_terrain_contract(&self, meta: &mut WorldMeta) -> Result<usize, OpenWorldError> {
        let current = terrain_generation_contract_version();
        if meta.contract_version == current {
            return Ok(0);
        }
        let purged = self.purge_stored_chunks()?;
        meta.contract_version = current;
        meta.touch_updated();
        self.write_meta(meta)?;
        Ok(purged)
    }

    pub fn purge_stored_chunks(&self) -> Result<usize, OpenWorldError> {
        let mut deleted = 0usize;
        for item in self.db.iterator(rocksdb::IteratorMode::Start) {
            let (key, _) = item?;
            if key.starts_with(CHUNK_STORAGE_KEY_PREFIX) {
                self.db.delete(key)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    pub fn refresh_world_index(
        &self,
        save_root: impl AsRef<std::path::Path>,
        meta: &WorldMeta,
    ) -> Result<(), String> {
        upsert_world_index(save_root, meta)
    }

    pub fn write_meta(&self, meta: &WorldMeta) -> Result<(), rocksdb::Error> {
        let bytes = bincode::serialize(meta).expect("WorldMeta should always serialize");
        self.db.put(KEY_META, bytes)
    }

    pub fn write_edits(&self, edits: &StoredEdits) -> Result<(), rocksdb::Error> {
        let bytes = bincode::serialize(edits).expect("StoredEdits should always serialize");
        self.db.put(KEY_EDITS, bytes)
    }

    pub fn load_edits(&self) -> Result<StoredEdits, OpenWorldError> {
        let Some(bytes) = self.db.get(KEY_EDITS).map_err(OpenWorldError::RocksDb)? else {
            return Ok(StoredEdits {
                generation: 0,
                edits: Vec::new(),
            });
        };
        decode_edits(&bytes).map_err(OpenWorldError::EditsDecode)
    }

    pub fn save_world(
        &self,
        meta: &WorldMeta,
        store: &WorldEditStore,
    ) -> Result<(), rocksdb::Error> {
        self.write_meta(meta)?;
        self.write_edits(&StoredEdits::from_store(store))
    }

    pub fn write_chunk_blob(&self, blob: &StoredChunkBlob) -> Result<(), rocksdb::Error> {
        let key = chunk_storage_key(ChunkPos::new(blob.chunk_x, blob.chunk_z));
        let bytes = bincode::serialize(blob).expect("StoredChunkBlob should always serialize");
        self.db.put(key, bytes)
    }

    pub fn load_chunk_blob(
        &self,
        pos: ChunkPos,
        expected_contract: u64,
    ) -> Result<Option<StoredChunkBlob>, OpenWorldError> {
        let key = chunk_storage_key(pos);
        let Some(bytes) = self.db.get(&key).map_err(OpenWorldError::RocksDb)? else {
            return Ok(None);
        };
        let blob = decode_chunk(&bytes).map_err(OpenWorldError::ChunkDecode)?;
        if !chunk_blob_matches_contract(&blob, expected_contract) {
            self.db.delete(key)?;
            return Ok(None);
        }
        Ok(Some(blob))
    }

    pub fn persist_chunk_from_data(
        &self,
        pos: ChunkPos,
        contract_version: u64,
        chunk: &ChunkData,
    ) -> Result<(), rocksdb::Error> {
        let blob = StoredChunkBlob::from_chunk_data(pos, contract_version, chunk);
        self.write_chunk_blob(&blob)
    }

    pub fn persist_generated_chunk(
        &self,
        pos: ChunkPos,
        contract_version: u64,
        generated: &GeneratedChunk,
    ) -> Result<(), rocksdb::Error> {
        let blob = StoredChunkBlob::from_generated(
            pos,
            contract_version,
            &generated.chunk,
            &generated.decor,
        );
        self.write_chunk_blob(&blob)
    }

    pub fn materialize_and_persist_chunk(
        &self,
        pos: ChunkPos,
        contract_version: u64,
        context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
        registry: &BlockRegistry,
    ) -> Result<(), rocksdb::Error> {
        let mut generated = if let Ok(Some(blob)) = self.load_chunk_blob(pos, contract_version) {
            let mut chunk = blob.to_chunk_data();
            edit_store.apply_all_edits_to_chunk(pos, &mut chunk);
            GeneratedChunk {
                chunk,
                decor: blob.to_decor_output(),
            }
        } else {
            let mut generated = generate_chunk_uncached(pos, context);
            edit_store.apply_all_edits_to_chunk(pos, &mut generated.chunk);
            generated
        };
        generated.decor = chunk_decor_output_uncapped(&generated.chunk, registry);
        self.persist_generated_chunk(pos, contract_version, &generated)
    }

    #[must_use]
    pub fn load_generated_chunk(
        &self,
        pos: ChunkPos,
        expected_contract: u64,
        _context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
    ) -> Option<GeneratedChunk> {
        let blob = self.load_chunk_blob(pos, expected_contract).ok().flatten()?;
        let mut chunk = blob.to_chunk_data();
        edit_store.apply_all_edits_to_chunk(pos, &mut chunk);
        Some(GeneratedChunk {
            chunk,
            decor: blob.to_decor_output(),
        })
    }
}

fn sanitize_world_id(world_id: &str) -> String {
    let trimmed = world_id.trim();
    if trimmed.is_empty() {
        return DEFAULT_WORLD_ID.to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct WorldMetaV1 {
    format_version: u32,
    world_id: String,
    terrain_seed: u32,
    contract_version: u64,
    player_x: f32,
    player_y: f32,
    player_z: f32,
    has_player_position: bool,
}

fn decode_meta(bytes: &[u8]) -> Result<WorldMeta, String> {
    if let Ok(meta) = bincode::deserialize::<WorldMeta>(bytes) {
        if meta.format_version == WORLD_META_FORMAT_VERSION {
            return Ok(meta);
        }
        if meta.format_version == WORLD_META_FORMAT_VERSION_V1 {
            return Ok(upgrade_meta_v1(meta));
        }
    }

    let legacy: WorldMetaV1 = bincode::deserialize(bytes).map_err(|error| error.to_string())?;
    if legacy.format_version != WORLD_META_FORMAT_VERSION_V1 {
        return Err(format!(
            "unsupported world meta format version {}",
            legacy.format_version
        ));
    }
    let now = unix_timestamp_now();
    Ok(WorldMeta {
        format_version: WORLD_META_FORMAT_VERSION,
        world_id: legacy.world_id,
        terrain_seed: legacy.terrain_seed,
        contract_version: legacy.contract_version,
        player_x: legacy.player_x,
        player_y: legacy.player_y,
        player_z: legacy.player_z,
        has_player_position: legacy.has_player_position,
        created_at_unix: now,
        updated_at_unix: now,
    })
}

fn upgrade_meta_v1(mut meta: WorldMeta) -> WorldMeta {
    let now = unix_timestamp_now();
    if meta.created_at_unix == 0 {
        meta.created_at_unix = now;
    }
    if meta.updated_at_unix == 0 {
        meta.updated_at_unix = now;
    }
    meta.format_version = WORLD_META_FORMAT_VERSION;
    meta
}

fn decode_chunk(bytes: &[u8]) -> Result<StoredChunkBlob, String> {
    let blob: StoredChunkBlob = bincode::deserialize(bytes).map_err(|error| error.to_string())?;
    if !chunk_blob_format_supported(blob.format_version) {
        return Err(format!(
            "unsupported stored chunk format version {}",
            blob.format_version
        ));
    }
    Ok(blob)
}

fn decode_edits(bytes: &[u8]) -> Result<StoredEdits, String> {
    bincode::deserialize(bytes).map_err(|error| error.to_string())
}

#[must_use]
pub fn default_terrain_seed_from_env() -> u32 {
    std::env::var("RUMPEL_WORLD_SEED")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_TERRAIN_SEED)
}

#[must_use]
pub fn world_id_from_env() -> String {
    std::env::var("RUMPEL_WORLD_ID")
        .ok()
        .map(|value| sanitize_world_id(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_WORLD_ID.to_string())
}

#[must_use]
pub fn new_world_id_from_timestamp() -> String {
    sanitize_world_id(&format!("world_{}", unix_timestamp_now()))
}

#[must_use]
pub fn resolve_world_id_from_env() -> String {
    if env_flag_enabled("RUMPEL_WORLD_NEW") {
        return new_world_id_from_timestamp();
    }
    world_id_from_env()
}

#[must_use]
pub fn world_reset_requested_from_env() -> bool {
    env_flag_enabled("RUMPEL_WORLD_RESET")
}

pub fn log_world_list_if_requested(save_root: impl AsRef<Path>) {
    if !env_flag_enabled("RUMPEL_WORLD_LIST") {
        return;
    }
    for world_id in list_world_ids(save_root) {
        println!("rumpel_world:{world_id}");
    }
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

#[must_use]
pub fn save_root_from_env() -> PathBuf {
    std::env::var("RUMPEL_SAVE_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("saves"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_coords::{ChunkPos, LocalBlockPos};
    use rumpel_world::chunk::{WorldBlockEdit, WorldEditStore};
    use rumpel_world::world_gen::init_active_world_terrain;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("world store test mutex poisoned")
    }

    fn stale_contract_pair() -> (u64, u64) {
        let current = terrain_generation_contract_version();
        let stale = current.wrapping_sub(1);
        if stale == current {
            (current, current.wrapping_add(1))
        } else {
            (current, stale)
        }
    }

    #[test]
    fn roundtrips_meta_and_edits() {
        let _guard = test_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let seed = 4242;
        init_active_world_terrain(seed);
        let (db, loaded) =
            WorldDatabase::open_or_create_world(temp.path(), "test_world", seed).expect("open");
        assert_eq!(loaded.meta.terrain_seed, seed);
        assert!(loaded.edits.edits.is_empty());

        let mut meta = loaded.meta.clone();
        meta.set_player_position([12.0, 64.0, -8.0]);
        let mut store = WorldEditStore::default();
        let edit = WorldBlockEdit::new(
            ChunkPos::new(0, 0),
            LocalBlockPos::new(1, 2, 3),
            5,
        );
        store.apply_edit(edit);

        db.save_world(&meta, &store).expect("save");
        drop(db);
        let (db2, reloaded) =
            WorldDatabase::open_or_create_world(temp.path(), "test_world", seed).expect("reopen");
        assert_eq!(reloaded.meta, meta);
        assert_eq!(reloaded.edits.edits.len(), 1);

        let mut restored = WorldEditStore::default();
        reloaded.edits.apply_to_store(&mut restored);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.generation(), store.generation());
        let _ = db2;
    }

    #[test]
    fn roundtrips_chunk_blob() {
        let _guard = test_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let seed = 9001;
        init_active_world_terrain(seed);
        let contract = terrain_generation_contract_version();
        let (db, _) =
            WorldDatabase::open_or_create_world(temp.path(), "chunk_world", seed).expect("open");
        let pos = ChunkPos::new(2, -3);
        let mut chunk = ChunkData::default();
        chunk.set_block(4, 12, 7, 9);
        db.persist_chunk_from_data(pos, contract, &chunk)
            .expect("persist chunk");
        drop(db);

        let (db2, _) =
            WorldDatabase::open_or_create_world(temp.path(), "chunk_world", seed).expect("reopen");
        let loaded = db2
            .load_chunk_blob(pos, contract)
            .expect("load")
            .expect("blob");
        assert_eq!(loaded.chunk_x, pos.x);
        assert_eq!(loaded.chunk_z, pos.z);
        assert_eq!(loaded.contract_version, contract);
        let restored = loaded.to_chunk_data();
        assert_eq!(restored.get_block(4, 12, 7), 9);
    }

    #[test]
    fn rejects_stale_chunk_contract_on_load() {
        let _guard = test_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let seed = 42;
        init_active_world_terrain(seed);
        let (current, stale) = stale_contract_pair();
        let (db, _) =
            WorldDatabase::open_or_create_world(temp.path(), "stale_chunks", seed).expect("open");
        let pos = ChunkPos::new(0, 0);
        let mut chunk = ChunkData::default();
        chunk.set_block(1, 1, 1, 3);
        db.persist_chunk_from_data(pos, stale, &chunk)
            .expect("persist old contract");
        assert!(db.load_chunk_blob(pos, stale).expect("load").is_some());
        assert!(db.load_chunk_blob(pos, current).expect("load").is_none());
    }

    #[test]
    fn reconcile_purges_chunks_when_contract_changes() {
        let _guard = test_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let seed = 7;
        init_active_world_terrain(seed);
        let (db, loaded) =
            WorldDatabase::open_or_create_world(temp.path(), "migrate", seed).expect("open");
        let current = loaded.meta.contract_version;
        let stale_contract = current.wrapping_sub(1);
        let stale_contract = if stale_contract == current {
            current.wrapping_add(1)
        } else {
            stale_contract
        };
        let pos = ChunkPos::new(1, 1);
        let mut chunk = ChunkData::default();
        chunk.set_block(2, 2, 2, 4);
        db.persist_chunk_from_data(pos, stale_contract, &chunk)
            .expect("persist");
        let mut meta = loaded.meta.clone();
        meta.contract_version = stale_contract;
        db.write_meta(&meta).expect("meta");
        let purged = db.reconcile_terrain_contract(&mut meta).expect("reconcile");
        assert_eq!(purged, 1);
        assert_eq!(meta.contract_version, terrain_generation_contract_version());
        assert!(db
            .load_chunk_blob(pos, meta.contract_version)
            .expect("load")
            .is_none());
    }
}
