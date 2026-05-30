use bevy::prelude::*;
use crate::chunk::ChunkManager;

pub struct VoxelRaycastHit {
    pub position: IVec3,
    pub normal: IVec3,
}

pub fn raycast_voxels(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    chunk_manager: &ChunkManager,
) -> Option<VoxelRaycastHit> {
    // 3D DDA (Digital Differential Analyzer) algorithm for voxel raycasting
    // This is a skeleton that will be used by the player to break/place blocks
    // against the new ChunkManager.
    
    let mut t = 0.0;
    let mut current_voxel = IVec3::new(
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    );

    let step = IVec3::new(
        if direction.x > 0.0 { 1 } else { -1 },
        if direction.y > 0.0 { 1 } else { -1 },
        if direction.z > 0.0 { 1 } else { -1 },
    );

    let t_delta = Vec3::new(
        (1.0 / direction.x).abs(),
        (1.0 / direction.y).abs(),
        (1.0 / direction.z).abs(),
    );

    let mut t_max = Vec3::new(
        if direction.x > 0.0 { (current_voxel.x as f32 + 1.0 - origin.x) * t_delta.x } else { (origin.x - current_voxel.x as f32) * t_delta.x },
        if direction.y > 0.0 { (current_voxel.y as f32 + 1.0 - origin.y) * t_delta.y } else { (origin.y - current_voxel.y as f32) * t_delta.y },
        if direction.z > 0.0 { (current_voxel.z as f32 + 1.0 - origin.z) * t_delta.z } else { (origin.z - current_voxel.z as f32) * t_delta.z },
    );

    let mut normal = IVec3::ZERO;

    while t < max_distance {
        // TODO: Query chunk_manager for solid blocks at `current_voxel`
        // if chunk_manager.get_block(current_voxel) != AIR { return Some(...) }

        if t_max.x < t_max.y {
            if t_max.x < t_max.z {
                current_voxel.x += step.x;
                t = t_max.x;
                t_max.x += t_delta.x;
                normal = IVec3::new(-step.x, 0, 0);
            } else {
                current_voxel.z += step.z;
                t = t_max.z;
                t_max.z += t_delta.z;
                normal = IVec3::new(0, 0, -step.z);
            }
        } else {
            if t_max.y < t_max.z {
                current_voxel.y += step.y;
                t = t_max.y;
                t_max.y += t_delta.y;
                normal = IVec3::new(0, -step.y, 0);
            } else {
                current_voxel.z += step.z;
                t = t_max.z;
                t_max.z += t_delta.z;
                normal = IVec3::new(0, 0, -step.z);
            }
        }
    }

    None
}

pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

pub fn collide_aabb_with_voxels(
    player_aabb: &Aabb,
    velocity: Vec3,
    chunk_manager: &ChunkManager,
) -> Vec3 {
    // Basic Swept AABB collision against a grid of voxels
    // This function returns the allowed velocity after collision.
    // In a real engine, we expand the AABB by the velocity and query all intersecting voxels.
    
    // For now, return the velocity unmodified (fly mode)
    // TODO: Query all blocks in `chunk_manager` intersecting (player_aabb.min + velocity) and (player_aabb.max + velocity)
    velocity
}
