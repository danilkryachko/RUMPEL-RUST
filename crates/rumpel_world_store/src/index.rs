use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::meta::{unix_timestamp_now, WorldMeta};

const WORLD_INDEX_FILE: &str = "worlds.ron";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldIndexEntry {
    pub world_id: String,
    pub terrain_seed: u32,
    pub contract_version: u64,
    pub updated_at_unix: u64,
    pub created_at_unix: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldIndex {
    pub worlds: Vec<WorldIndexEntry>,
}

impl WorldIndexEntry {
    #[must_use]
    pub fn from_meta(meta: &WorldMeta) -> Self {
        let now = unix_timestamp_now();
        Self {
            world_id: meta.world_id.clone(),
            terrain_seed: meta.terrain_seed,
            contract_version: meta.contract_version,
            updated_at_unix: meta.updated_at_unix.max(now),
            created_at_unix: meta.created_at_unix.max(now),
        }
    }
}

#[must_use]
pub fn world_index_path(save_root: impl AsRef<Path>) -> PathBuf {
    save_root.as_ref().join(WORLD_INDEX_FILE)
}

pub fn load_world_index(save_root: impl AsRef<Path>) -> WorldIndex {
    let path = world_index_path(save_root);
    let Ok(contents) = fs::read_to_string(&path) else {
        return WorldIndex::default();
    };
    ron::from_str(&contents).unwrap_or_default()
}

pub fn save_world_index(save_root: impl AsRef<Path>, index: &WorldIndex) -> Result<(), String> {
    let path = world_index_path(save_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = ron::ser::to_string_pretty(index, ron::ser::PrettyConfig::new())
        .map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
}

pub fn upsert_world_index(save_root: impl AsRef<Path>, meta: &WorldMeta) -> Result<(), String> {
    let mut index = load_world_index(&save_root);
    let entry = WorldIndexEntry::from_meta(meta);
    if let Some(existing) = index
        .worlds
        .iter_mut()
        .find(|world| world.world_id == entry.world_id)
    {
        existing.terrain_seed = entry.terrain_seed;
        existing.contract_version = entry.contract_version;
        existing.updated_at_unix = entry.updated_at_unix;
    } else {
        index.worlds.push(entry);
    }
    index
        .worlds
        .sort_by(|left, right| right.updated_at_unix.cmp(&left.updated_at_unix));
    save_world_index(save_root, &index)
}

#[must_use]
pub fn list_world_ids(save_root: impl AsRef<Path>) -> Vec<String> {
    let root = save_root.as_ref();
    let mut ids: Vec<String> = load_world_index(root)
        .worlds
        .into_iter()
        .map(|entry| entry.world_id)
        .collect();
    if let Ok(read_dir) = fs::read_dir(root) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == "worlds.ron" {
                continue;
            }
            if !ids.iter().any(|id| id == name) {
                ids.push(name.to_string());
            }
        }
    }
    ids.sort();
    ids
}
