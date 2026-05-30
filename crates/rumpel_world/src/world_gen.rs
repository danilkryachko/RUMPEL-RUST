use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, Chunk};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::BlockRegistry;
use rumpel_coords::ChunkPos;

pub fn generate_chunk(pos: ChunkPos, registry: &BlockRegistry) -> Chunk {
    let mut chunk = Chunk::new();
    let perlin = Perlin::new(1337); // Жестко заданный сид для тестов

    let grass_id = registry.get_id("grass").unwrap_or(0);
    let dirt_id = registry.get_id("dirt").unwrap_or(0);
    let stone_id = registry.get_id("stone").unwrap_or(0);

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            // Глобальные координаты для шума
            let global_x = pos.x as f64 * CHUNK_SIZE as f64 + x as f64;
            let global_z = pos.z as f64 * CHUNK_SIZE as f64 + z as f64;

            // Шум Перлина возвращает значения от -1.0 до 1.0
            let noise_val = perlin.get([global_x * 0.02, global_z * 0.02]);

            // Преобразуем шум в высоту от 10 до 50 блоков
            let height = ((noise_val + 1.0) * 0.5 * 40.0) as usize + 10;

            for y in 0..CHUNK_HEIGHT {
                if y < height {
                    if y == height - 1 {
                        chunk.set_block(x, y, z, grass_id); // Верхний слой - трава
                    } else if y > height - 4 {
                        chunk.set_block(x, y, z, dirt_id); // Под травой - земля
                    } else {
                        chunk.set_block(x, y, z, stone_id); // Ниже - камень
                    }
                }
            }
        }
    }

    chunk
}
