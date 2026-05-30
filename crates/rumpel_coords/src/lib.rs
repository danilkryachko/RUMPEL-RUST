use bevy::prelude::*;

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
        Self { x, y, z }
    }
}

/// Мировые абсолютные координаты (чаще всего используются для игрока/мобов/физики).
#[derive(Clone, Copy, Debug, PartialEq, Component)]
pub struct WorldPos {
    pub position: Vec3,
}

impl WorldPos {
    pub fn to_chunk_pos(&self, chunk_size: i32) -> ChunkPos {
        ChunkPos::new(
            (self.position.x.floor() as i32).div_euclid(chunk_size),
            (self.position.z.floor() as i32).div_euclid(chunk_size),
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
