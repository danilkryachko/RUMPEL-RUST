use bevy::prelude::*;

pub mod state;

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
/// В legacy storage X и Z лежат в диапазоне 0..15, Y в диапазоне 0..255.
/// Runtime backend может использовать другой размер чанка, но gameplay-код не должен
/// смешивать локальные и мировые координаты.
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

/// Мировые целочисленные координаты блока.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldBlockPos {
    pub position: IVec3,
}

impl WorldBlockPos {
    pub fn new(position: IVec3) -> Self {
        Self { position }
    }
}

impl From<IVec3> for WorldBlockPos {
    fn from(position: IVec3) -> Self {
        Self::new(position)
    }
}

impl From<WorldBlockPos> for IVec3 {
    fn from(pos: WorldBlockPos) -> Self {
        pos.position
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
