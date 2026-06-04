use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const WORLD_META_FORMAT_VERSION: u32 = 2;
pub const WORLD_META_FORMAT_VERSION_V1: u32 = 1;
pub const DEFAULT_TERRAIN_SEED: u32 = 1337;
pub const DEFAULT_WORLD_ID: &str = "default";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldMeta {
    pub format_version: u32,
    pub world_id: String,
    pub terrain_seed: u32,
    pub contract_version: u64,
    pub player_x: f32,
    pub player_y: f32,
    pub player_z: f32,
    pub has_player_position: bool,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

impl WorldMeta {
    #[must_use]
    pub fn new(world_id: impl Into<String>, terrain_seed: u32, contract_version: u64) -> Self {
        let now = unix_timestamp_now();
        Self {
            format_version: WORLD_META_FORMAT_VERSION,
            world_id: world_id.into(),
            terrain_seed,
            contract_version,
            player_x: 0.0,
            player_y: 0.0,
            player_z: 0.0,
            has_player_position: false,
            created_at_unix: now,
            updated_at_unix: now,
        }
    }

    pub fn touch_updated(&mut self) {
        self.updated_at_unix = unix_timestamp_now();
    }

    #[must_use]
    pub fn player_position(&self) -> Option<[f32; 3]> {
        self.has_player_position.then_some([self.player_x, self.player_y, self.player_z])
    }

    pub fn set_player_position(&mut self, position: [f32; 3]) {
        self.player_x = position[0];
        self.player_y = position[1];
        self.player_z = position[2];
        self.has_player_position = true;
    }
}

#[must_use]
pub fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
