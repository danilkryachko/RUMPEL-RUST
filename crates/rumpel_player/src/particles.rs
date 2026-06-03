use crate::mobs::build_cube_mesh;
use bevy::prelude::*;
use mlua::Table;

#[derive(Component)]
pub struct VoxelParticle {
    pub velocity: Vec3,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub start_scale: f32,
}

pub fn spawn_voxel_particles_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lua_runtime: Option<Res<rumpel_modding::LuaRuntime>>,
) {
    let Some(lua_runtime) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_runtime.0.lock() else {
        return;
    };

    let globals = lua.globals();
    let Ok(queue) = globals.get::<Table>("ParticleSpawnQueue") else {
        return;
    };

    let len = queue.len().unwrap_or(0);
    if len == 0 {
        return;
    }

    let mut spawns = Vec::new();
    for i in 1..=len {
        if let Ok(entry) = queue.get::<Table>(i)
            && let (
                Ok(x),
                Ok(y),
                Ok(z),
                Ok(vx),
                Ok(vy),
                Ok(vz),
                Ok(r),
                Ok(g),
                Ok(b),
                Ok(a),
                Ok(lifetime),
                Ok(size),
            ) = (
                entry.get::<f32>("x"),
                entry.get::<f32>("y"),
                entry.get::<f32>("z"),
                entry.get::<f32>("vx"),
                entry.get::<f32>("vy"),
                entry.get::<f32>("vz"),
                entry.get::<f32>("r"),
                entry.get::<f32>("g"),
                entry.get::<f32>("b"),
                entry.get::<f32>("a"),
                entry.get::<f32>("lifetime"),
                entry.get::<f32>("size"),
            )
        {
            spawns.push((
                Vec3::new(x, y, z),
                Vec3::new(vx, vy, vz),
                Color::srgba(r, g, b, a),
                lifetime,
                size,
            ));
        }
    }

    // Clear the queue in Lua
    if let Err(e) = lua.load("ParticleSpawnQueue = {}").exec() {
        error!("MODS: Failed to clear ParticleSpawnQueue: {:?}", e);
        return;
    }

    for (pos, vel, color, lifetime, size) in spawns {
        // Spawn particle cube
        commands.spawn((
            VoxelParticle {
                velocity: vel,
                lifetime,
                max_lifetime: lifetime,
                start_scale: size,
            },
            Mesh3d(meshes.add(build_cube_mesh(size, size, size))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true, // glowing unlit effect!
                ..default()
            })),
            Transform::from_translation(pos),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
    }
}

pub fn update_voxel_particles_system(
    mut commands: Commands,
    time: Res<Time>,
    mut particle_query: Query<(Entity, &mut Transform, &mut VoxelParticle)>,
) {
    let delta = time.delta_secs();

    for (entity, mut transform, mut particle) in &mut particle_query {
        particle.lifetime -= delta;
        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Apply movement
        transform.translation += particle.velocity * delta;

        // Shrink scale gradually over its lifetime
        let life_ratio = (particle.lifetime / particle.max_lifetime).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(life_ratio);
    }
}
