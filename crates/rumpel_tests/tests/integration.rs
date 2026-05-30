use bevy::prelude::*;
use rumpel_prelude::*;

#[test]
fn test_world_to_chunk_conversion() {
    let chunk_size = 16;
    
    // Position at origin
    let pos_origin = WorldPos { position: Vec3::new(0.0, 0.0, 0.0) };
    assert_eq!(pos_origin.to_chunk_pos(chunk_size), ChunkPos::new(0, 0));

    // Position just inside positive bounds
    let pos_inside = WorldPos { position: Vec3::new(15.9, 0.0, 15.9) };
    assert_eq!(pos_inside.to_chunk_pos(chunk_size), ChunkPos::new(0, 0));

    // Position crossing positive boundary
    let pos_cross = WorldPos { position: Vec3::new(16.0, 0.0, 16.0) };
    assert_eq!(pos_cross.to_chunk_pos(chunk_size), ChunkPos::new(1, 1));

    // Position inside negative bounds
    let pos_neg = WorldPos { position: Vec3::new(-0.1, 0.0, -0.1) };
    assert_eq!(pos_neg.to_chunk_pos(chunk_size), ChunkPos::new(-1, -1));

    // Position deep in negative
    let pos_deep_neg = WorldPos { position: Vec3::new(-16.0, 0.0, -16.0) };
    assert_eq!(pos_deep_neg.to_chunk_pos(chunk_size), ChunkPos::new(-1, -1));
    
    let pos_deeper_neg = WorldPos { position: Vec3::new(-16.1, 0.0, -16.1) };
    assert_eq!(pos_deeper_neg.to_chunk_pos(chunk_size), ChunkPos::new(-2, -2));
}

#[test]
fn test_world_to_global_block_conversion() {
    let pos = WorldPos { position: Vec3::new(1.5, -0.5, 3.9) };
    assert_eq!(pos.to_global_block_pos(), IVec3::new(1, -1, 3));
}
