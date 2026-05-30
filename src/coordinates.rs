use bevy::prelude::*;

pub const CHUNK_SIZE: i32 = 16;
pub const CHUNK_HEIGHT: i32 = 256;

/// Глобальные координаты чанка (X, Z) в бесконечном мире.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Component)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// Локальные координаты конкретного блока внутри чанка.
/// X и Z всегда от 0 до 15. Y от 0 до 255.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalBlockPos {
    pub x: u8,
    pub y: u16,
    pub z: u8,
}

impl LocalBlockPos {
    pub fn new(x: u8, y: u16, z: u8) -> Self {
        debug_assert!(x < CHUNK_SIZE as u8);
        debug_assert!(z < CHUNK_SIZE as u8);
        debug_assert!(y < CHUNK_HEIGHT as u16);
        Self { x, y, z }
    }
}

/// Мировые абсолютные координаты (чаще всего используются для игрока/мобов/физики).
#[derive(Clone, Copy, Debug, PartialEq, Component)]
pub struct WorldPos {
    pub position: Vec3,
}

impl WorldPos {
    pub fn to_chunk_pos(&self) -> ChunkPos {
        ChunkPos::new(
            (self.position.x.floor() as i32).div_euclid(CHUNK_SIZE),
            (self.position.z.floor() as i32).div_euclid(CHUNK_SIZE),
        )
    }

    pub fn to_global_block_pos(&self) -> IVec3 {
        IVec3::new(
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        )
    }
}
