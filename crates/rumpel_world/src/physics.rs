use bevy::prelude::*;
use rumpel_coords::WorldBlockPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelRaycastHit {
    pub position: WorldBlockPos,
    pub normal: IVec3,
}

pub fn raycast_voxels(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    mut is_solid: impl FnMut(WorldBlockPos) -> bool,
) -> Option<VoxelRaycastHit> {
    if max_distance <= 0.0 || direction.length_squared() == 0.0 {
        return None;
    }

    let direction = direction.normalize();
    let mut current_voxel = IVec3::new(
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    );
    let mut normal = IVec3::ZERO;

    if is_solid(WorldBlockPos::new(current_voxel)) {
        return Some(VoxelRaycastHit {
            position: WorldBlockPos::new(current_voxel),
            normal,
        });
    }

    let step = IVec3::new(
        axis_step(direction.x),
        axis_step(direction.y),
        axis_step(direction.z),
    );
    let t_delta = Vec3::new(
        axis_t_delta(direction.x),
        axis_t_delta(direction.y),
        axis_t_delta(direction.z),
    );
    let mut t_max = Vec3::new(
        axis_t_max(origin.x, current_voxel.x, direction.x, t_delta.x),
        axis_t_max(origin.y, current_voxel.y, direction.y, t_delta.y),
        axis_t_max(origin.z, current_voxel.z, direction.z, t_delta.z),
    );

    let mut distance = 0.0;
    while distance <= max_distance {
        if t_max.x <= t_max.y && t_max.x <= t_max.z {
            current_voxel.x += step.x;
            distance = t_max.x;
            t_max.x += t_delta.x;
            normal = IVec3::new(-step.x, 0, 0);
        } else if t_max.y <= t_max.z {
            current_voxel.y += step.y;
            distance = t_max.y;
            t_max.y += t_delta.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            current_voxel.z += step.z;
            distance = t_max.z;
            t_max.z += t_delta.z;
            normal = IVec3::new(0, 0, -step.z);
        }

        if distance > max_distance {
            break;
        }

        let position = WorldBlockPos::new(current_voxel);
        if is_solid(position) {
            return Some(VoxelRaycastHit { position, normal });
        }
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

pub fn collide_aabb_with_voxels(
    player_aabb: &Aabb,
    velocity: Vec3,
    mut is_solid: impl FnMut(WorldBlockPos) -> bool,
) -> Vec3 {
    let mut allowed_velocity = velocity;
    let mut swept_aabb = *player_aabb;

    allowed_velocity.x = resolve_axis(swept_aabb, Vec3::X, allowed_velocity.x, &mut is_solid);
    swept_aabb.min.x += allowed_velocity.x;
    swept_aabb.max.x += allowed_velocity.x;

    allowed_velocity.y = resolve_axis(swept_aabb, Vec3::Y, allowed_velocity.y, &mut is_solid);
    swept_aabb.min.y += allowed_velocity.y;
    swept_aabb.max.y += allowed_velocity.y;

    allowed_velocity.z = resolve_axis(swept_aabb, Vec3::Z, allowed_velocity.z, &mut is_solid);

    allowed_velocity
}

fn axis_step(value: f32) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn axis_t_delta(value: f32) -> f32 {
    if value == 0.0 {
        f32::INFINITY
    } else {
        (1.0 / value).abs()
    }
}

fn axis_t_max(origin_axis: f32, voxel_axis: i32, direction_axis: f32, t_delta: f32) -> f32 {
    if direction_axis > 0.0 {
        (voxel_axis as f32 + 1.0 - origin_axis) * t_delta
    } else if direction_axis < 0.0 {
        (origin_axis - voxel_axis as f32) * t_delta
    } else {
        f32::INFINITY
    }
}

fn resolve_axis(
    aabb: Aabb,
    axis: Vec3,
    requested_delta: f32,
    is_solid: &mut impl FnMut(WorldBlockPos) -> bool,
) -> f32 {
    if requested_delta == 0.0 {
        return 0.0;
    }

    let delta = axis * requested_delta;
    let moved = Aabb {
        min: aabb.min + delta,
        max: aabb.max + delta,
    };

    if aabb_intersects_solid(moved, is_solid) {
        0.0
    } else {
        requested_delta
    }
}

fn aabb_intersects_solid(aabb: Aabb, is_solid: &mut impl FnMut(WorldBlockPos) -> bool) -> bool {
    let min = IVec3::new(
        aabb.min.x.floor() as i32,
        aabb.min.y.floor() as i32,
        aabb.min.z.floor() as i32,
    );
    let max = IVec3::new(
        (aabb.max.x - f32::EPSILON).floor() as i32,
        (aabb.max.y - f32::EPSILON).floor() as i32,
        (aabb.max.z - f32::EPSILON).floor() as i32,
    );

    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                if is_solid(WorldBlockPos::new(IVec3::new(x, y, z))) {
                    return true;
                }
            }
        }
    }

    false
}
