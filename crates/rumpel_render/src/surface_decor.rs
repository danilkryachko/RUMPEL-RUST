use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, Mesh},
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::PrimitiveTopology,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use rumpel_prelude::ChunkPos;
use rumpel_prelude::*;
use rumpel_world::{
    chunk::WorldEditStore,
    chunk_gen_cache::cached_chunk,
    surface_decor::{
        DecorBlockContext, DecorInstance, decor_grass_cap_from_env, decor_leaf_cap_from_env,
        resolve_chunk_decor,
    },
    world_gen::WorldGenerationContext,
};
use std::sync::Arc;

use crate::packed_quad_pipeline::packed_view_radius_chunks;

const DESPAWN_RADIUS_EXTRA_CHUNKS: i32 = 2;
const DECOR_CHUNKS_PER_FRAME: usize = 6;
const DECOR_UPLOADS_PER_FRAME: usize = 4;
const GRASS_TEXTURE_PATH: &str = "textures/vegetation/grass_bush.png";
const LEAF_TEXTURE_PATH: &str = "textures/vegetation/leaves_cluster.png";
const GRASS_ALPHA_CUTOFF: f32 = 0.45;
const LEAF_ALPHA_CUTOFF: f32 = 0.16;

pub struct SurfaceDecorPlugin;

impl Plugin for SurfaceDecorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceDecorState>()
            .init_resource::<SurfaceDecorMetrics>()
            .add_systems(Startup, init_surface_decor_assets)
            .add_systems(
                Update,
                (handle_decor_build_tasks, stream_surface_decor).chain(),
            );
    }
}

#[derive(Component)]
struct ChunkDecorRoot {
    chunk: IVec2,
}

#[derive(Component)]
struct DecorBuildTask(Task<BuiltChunkDecor>);

struct BuiltChunkDecor {
    chunk: IVec2,
    grass_mesh: Option<Mesh>,
    leaf_mesh: Option<Mesh>,
}

#[derive(Clone, Copy)]
struct PendingDecorChunk {
    pos: IVec2,
    distance_sq: i32,
}

#[derive(Resource)]
struct SurfaceDecorAssets {
    grass_material: Handle<StandardMaterial>,
    leaf_material: Handle<StandardMaterial>,
    grass_template: CrossBillboardTemplate,
    leaf_template: LeafClumpTemplate,
}

#[derive(Resource, Default)]
pub(crate) struct SurfaceDecorState {
    pub(crate) loaded: HashMap<IVec2, Entity>,
    pub(crate) building: HashMap<IVec2, Entity>,
    pending: Vec<PendingDecorChunk>,
    last_center: Option<IVec2>,
    /// Generation from `WorldEditStore` last seen by the invalidation system.
    last_seen_edit_generation: u64,
}

#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct SurfaceDecorMetrics {
    pub loaded_chunks: usize,
    pub building_chunks: usize,
    pub pending_chunks: usize,
    pub grass_instances_last_upload: usize,
    pub leaf_instances_last_upload: usize,
}

struct CrossBillboardTemplate {
    positions: [[f32; 3]; 8],
    normals: [[f32; 3]; 8],
    uvs: [[f32; 2]; 8],
    indices: [u32; 12],
}

impl CrossBillboardTemplate {
    fn new() -> Self {
        Self {
            positions: [
                [0.0, 0.0, -0.5],
                [0.0, 0.0, 0.5],
                [0.0, 1.0, 0.5],
                [0.0, 1.0, -0.5],
                [-0.5, 0.0, 0.0],
                [0.5, 0.0, 0.0],
                [0.5, 1.0, 0.0],
                [-0.5, 1.0, 0.0],
            ],
            normals: [
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            uvs: [
                [0.0, 1.0],
                [1.0, 1.0],
                [1.0, 0.0],
                [0.0, 0.0],
                [0.0, 1.0],
                [1.0, 1.0],
                [1.0, 0.0],
                [0.0, 0.0],
            ],
            indices: [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        }
    }
}

/// Volumetric leaf crown mesh ported from RUMPEL2 `_create_leaf_clump_mesh`.
#[derive(Clone)]
struct LeafClumpTemplate {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl LeafClumpTemplate {
    fn new() -> Self {
        let mut template = Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        };
        let pi = std::f32::consts::PI;
        template.append_bisector(
            0.00,
            Vec3::ZERO,
            Vec2::new(1.34, 1.18),
            Vec3::new(0.12, 0.98, 0.20),
            false,
        );
        template.append_bisector(
            pi * 0.50,
            Vec3::new(0.0, 0.02, 0.0),
            Vec2::new(1.30, 1.16),
            Vec3::new(-0.20, 0.98, 0.10),
            true,
        );
        template.append_bisector(
            pi * 0.25,
            Vec3::new(-0.03, 0.01, 0.03),
            Vec2::new(1.26, 1.12),
            Vec3::new(0.28, 0.94, -0.10),
            false,
        );
        template.append_bisector(
            -pi * 0.25,
            Vec3::new(0.03, -0.01, -0.03),
            Vec2::new(1.24, 1.10),
            Vec3::new(-0.10, 0.95, -0.28),
            true,
        );
        template.append_bisector(
            pi * 0.10,
            Vec3::new(0.06, 0.04, -0.04),
            Vec2::new(1.08, 0.90),
            Vec3::new(0.0, 0.44, 0.90),
            false,
        );
        template.append_bisector(
            -pi * 0.40,
            Vec3::new(-0.05, -0.03, 0.04),
            Vec2::new(1.06, 0.88),
            Vec3::new(0.88, 0.44, 0.0),
            true,
        );
        template.append_bisector(
            pi * 0.72,
            Vec3::new(0.02, 0.06, 0.05),
            Vec2::new(0.98, 0.82),
            Vec3::new(-0.82, 0.52, 0.26),
            false,
        );
        template.append_bisector(
            -pi * 0.86,
            Vec3::new(-0.02, -0.05, -0.05),
            Vec2::new(0.98, 0.82),
            Vec3::new(0.26, 0.52, -0.82),
            true,
        );
        template.append_bisector(
            pi * 0.16,
            Vec3::new(0.0, 0.10, 0.0),
            Vec2::new(0.92, 0.76),
            Vec3::new(0.36, 0.86, 0.34),
            false,
        );
        template.append_bisector(
            -pi * 0.58,
            Vec3::new(0.0, -0.08, 0.0),
            Vec2::new(0.90, 0.74),
            Vec3::new(-0.34, 0.84, 0.42),
            true,
        );
        template
    }

    fn append_bisector(
        &mut self,
        angle: f32,
        center_offset: Vec3,
        size: Vec2,
        up_axis: Vec3,
        flip_uv: bool,
    ) {
        let tangent = Vec3::new(angle.cos(), 0.0, angle.sin()).normalize_or_zero();
        let mut vertical = up_axis.normalize_or_zero();
        if tangent.dot(vertical).abs() > 0.82 {
            vertical = (vertical + Vec3::Y * 0.45).normalize_or_zero();
        }
        let half_width = size.x * 0.5;
        let half_height = size.y * 0.5;
        let p0 = center_offset - tangent * half_width - vertical * half_height;
        let p1 = center_offset + tangent * half_width - vertical * half_height;
        let p2 = center_offset + tangent * half_width + vertical * half_height;
        let p3 = center_offset - tangent * half_width + vertical * half_height;
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        let (uv0, uv1, uv2, uv3) = if flip_uv {
            (
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
            )
        } else {
            (
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 0.0),
            )
        };
        self.append_quad([p0, p1, p2, p3], normal, [uv0, uv1, uv2, uv3]);
    }

    fn append_quad(&mut self, points: [Vec3; 4], normal: Vec3, quad_uvs: [Vec2; 4]) {
        let base_index = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        for (point, uv) in points.into_iter().zip(quad_uvs) {
            self.positions.push(point.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
        }
        self.indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2,
            base_index,
            base_index + 2,
            base_index + 3,
        ]);
    }
}

fn init_surface_decor_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let grass_texture = asset_server.load(GRASS_TEXTURE_PATH);
    let leaf_texture = asset_server.load(LEAF_TEXTURE_PATH);
    commands.insert_resource(SurfaceDecorAssets {
        grass_material: materials.add(decor_material(
            grass_texture,
            [0.82, 1.0, 0.78],
            GRASS_ALPHA_CUTOFF,
        )),
        leaf_material: materials.add(decor_material(
            leaf_texture,
            [1.05, 1.05, 1.05],
            LEAF_ALPHA_CUTOFF,
        )),
        grass_template: CrossBillboardTemplate::new(),
        leaf_template: LeafClumpTemplate::new(),
    });
}

fn decor_material(texture: Handle<Image>, tint: [f32; 3], alpha_cutoff: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(tint[0], tint[1], tint[2]),
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Mask(alpha_cutoff),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

fn handle_decor_build_tasks(
    mut commands: Commands,
    decor_assets: Res<SurfaceDecorAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<SurfaceDecorState>,
    mut metrics: ResMut<SurfaceDecorMetrics>,
    mut tasks: Query<(Entity, &ChunkDecorRoot, &mut DecorBuildTask)>,
) {
    metrics.grass_instances_last_upload = 0;
    metrics.leaf_instances_last_upload = 0;
    let center = state.last_center;
    let despawn_radius_sq = decor_despawn_radius_sq();
    let mut uploaded = 0;

    for (entity, root, mut task) in &mut tasks {
        if uploaded >= DECOR_UPLOADS_PER_FRAME {
            break;
        }
        let Some(built) = check_ready(&mut task.0) else {
            continue;
        };
        state.building.remove(&root.chunk);

        let still_wanted = center
            .is_some_and(|center| chunk_distance_sq(center, built.chunk) <= despawn_radius_sq);
        if state.loaded.contains_key(&built.chunk) || !still_wanted {
            commands.entity(entity).despawn();
            continue;
        }

        if built.grass_mesh.is_none() && built.leaf_mesh.is_none() {
            commands.entity(entity).despawn();
            continue;
        }

        let chunk_origin = chunk_world_origin(built.chunk);
        commands.entity(entity).remove::<DecorBuildTask>().insert((
            Transform::from_translation(chunk_origin),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));

        if let Some(grass_mesh) = built.grass_mesh {
            let handle = meshes.add(grass_mesh);
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(handle),
                    MeshMaterial3d(decor_assets.grass_material.clone()),
                    Transform::IDENTITY,
                ));
            });
            metrics.grass_instances_last_upload += 1;
        }
        if let Some(leaf_mesh) = built.leaf_mesh {
            let handle = meshes.add(leaf_mesh);
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(handle),
                    MeshMaterial3d(decor_assets.leaf_material.clone()),
                    Transform::IDENTITY,
                ));
            });
            metrics.leaf_instances_last_upload += 1;
        }

        state.loaded.insert(built.chunk, entity);
        uploaded += 1;
    }

    metrics.loaded_chunks = state.loaded.len();
    metrics.building_chunks = state.building.len();
    metrics.pending_chunks = state.pending.len();
}

fn stream_surface_decor(
    mut commands: Commands,
    registry: Res<BlockRegistry>,
    edit_store: Res<WorldEditStore>,
    decor_assets: Res<SurfaceDecorAssets>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut state: ResMut<SurfaceDecorState>,
    mut metrics: ResMut<SurfaceDecorMetrics>,
) {
    let Some(camera_transform) = camera_query.iter().next() else {
        return;
    };
    let camera_pos = camera_transform.translation();
    let center = IVec2::new(
        (camera_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (camera_pos.z / CHUNK_SIZE as f32).floor() as i32,
    );

    if state.last_center != Some(center) {
        despawn_far_decor(center, &mut commands, &mut state);
        rebuild_pending_decor(center, &mut state);
        state.last_center = Some(center);
    }

    let world = Arc::new(WorldGenerationContext::from_registry(&registry));
    let edit_snapshot = Arc::new(edit_store.clone());
    let decor_context = DecorBlockContext::from_registry(&registry);
    let grass_cap = decor_grass_cap_from_env();
    let leaf_cap = decor_leaf_cap_from_env();
    let grass_template = decor_assets.grass_template.clone();
    let leaf_template = decor_assets.leaf_template.clone();
    let thread_pool = AsyncComputeTaskPool::get();

    let mut spawned = 0;
    while spawned < DECOR_CHUNKS_PER_FRAME {
        let Some(pending) = state.pending.pop() else {
            break;
        };
        if state.loaded.contains_key(&pending.pos) || state.building.contains_key(&pending.pos) {
            continue;
        }

        let chunk_pos = pending.pos;
        let world = Arc::clone(&world);
        let edit_snapshot = Arc::clone(&edit_snapshot);
        let decor_context = decor_context.clone();
        let grass_template = grass_template.clone();
        let leaf_template = leaf_template.clone();
        let task = thread_pool.spawn(async move {
            build_chunk_decor_meshes(
                chunk_pos,
                &world,
                &decor_context,
                grass_cap,
                leaf_cap,
                &grass_template,
                &leaf_template,
                &edit_snapshot,
            )
        });

        let entity = commands
            .spawn((ChunkDecorRoot { chunk: chunk_pos }, DecorBuildTask(task)))
            .id();
        state.building.insert(chunk_pos, entity);
        spawned += 1;
    }

    metrics.loaded_chunks = state.loaded.len();
    metrics.building_chunks = state.building.len();
    metrics.pending_chunks = state.pending.len();
}

#[expect(
    clippy::too_many_arguments,
    reason = "Decor build task receives independent context, caps, templates, and edits."
)]
fn build_chunk_decor_meshes(
    chunk: IVec2,
    world: &WorldGenerationContext,
    decor_context: &DecorBlockContext,
    grass_cap: i64,
    leaf_cap: i64,
    grass_template: &CrossBillboardTemplate,
    leaf_template: &LeafClumpTemplate,
    edit_store: &WorldEditStore,
) -> BuiltChunkDecor {
    let chunk_pos = ChunkPos { x: chunk.x, z: chunk.y };
    let generated = cached_chunk(chunk_pos, world);
    let mut chunk_data = generated.chunk.clone();
    edit_store.apply_all_edits_to_chunk(chunk_pos, &mut chunk_data);
    let (grass, leaves, _counts) = resolve_chunk_decor(
        &generated.decor,
        &chunk_data,
        decor_context,
        grass_cap,
        leaf_cap,
    );

    BuiltChunkDecor {
        chunk,
        grass_mesh: merge_grass_instances(&grass, grass_template),
        leaf_mesh: merge_leaf_instances(&leaves, leaf_template),
    }
}

fn merge_grass_instances(
    instances: &[DecorInstance],
    template: &CrossBillboardTemplate,
) -> Option<Mesh> {
    merge_template_instances(
        instances,
        &template.positions,
        &template.normals,
        &template.uvs,
        &template.indices,
        |instance, _uv_y| grass_vertex_color(instance),
    )
}

fn merge_leaf_instances(instances: &[DecorInstance], template: &LeafClumpTemplate) -> Option<Mesh> {
    merge_template_instances(
        instances,
        &template.positions,
        &template.normals,
        &template.uvs,
        &template.indices,
        leaf_vertex_color,
    )
}

fn merge_template_instances(
    instances: &[DecorInstance],
    template_positions: &[[f32; 3]],
    template_normals: &[[f32; 3]],
    template_uvs: &[[f32; 2]],
    template_indices: &[u32],
    vertex_color_fn: impl Fn(&DecorInstance, f32) -> [f32; 4],
) -> Option<Mesh> {
    if instances.is_empty() {
        return None;
    }

    let verts_per_instance = template_positions.len();
    let indices_per_instance = template_indices.len();
    let mut positions = Vec::with_capacity(instances.len() * verts_per_instance);
    let mut normals = Vec::with_capacity(instances.len() * verts_per_instance);
    let mut uvs = Vec::with_capacity(instances.len() * verts_per_instance);
    let mut colors = Vec::with_capacity(instances.len() * verts_per_instance);
    let mut indices = Vec::with_capacity(instances.len() * indices_per_instance);

    for instance in instances {
        let base_index = u32::try_from(positions.len()).unwrap_or(u32::MAX);
        let transform = instance_transform(instance);
        for (vertex_index, position) in template_positions.iter().enumerate() {
            let world = transform.transform_point(Vec3::from_array(*position));
            positions.push(world.to_array());
            let normal = (transform.rotation * Vec3::from_array(template_normals[vertex_index]))
                .normalize_or_zero();
            normals.push(normal.to_array());
            let uv = template_uvs[vertex_index];
            uvs.push(uv);
            colors.push(vertex_color_fn(instance, uv[1]));
        }
        for index in template_indices {
            indices.push(base_index + index);
        }
    }

    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices)),
    )
}

fn grass_vertex_color(instance: &DecorInstance) -> [f32; 4] {
    let mix = instance.custom[0];
    [0.72 + mix * 0.18, 0.96 + mix * 0.04, 0.68 + mix * 0.08, 1.0]
}

/// Light per-instance tint; texture carries most of the leaf color (RUMPEL2 shader).
fn leaf_vertex_color(instance: &DecorInstance, uv_y: f32) -> [f32; 4] {
    let warm_mix = instance.custom[0].clamp(0.0, 1.0);
    let shade_mix = instance.custom[2].clamp(0.0, 1.0);
    let height_mask = uv_y.clamp(0.0, 1.0);
    let lift = 0.94 + height_mask * 0.06;
    [
        (1.0 + warm_mix * 0.06 - shade_mix * 0.05) * lift,
        (1.0 + warm_mix * 0.04 - shade_mix * 0.03) * lift,
        (0.98 + warm_mix * 0.03 - shade_mix * 0.04) * lift,
        1.0,
    ]
}

fn instance_transform(instance: &DecorInstance) -> Transform {
    Transform {
        translation: Vec3::from_array(instance.translation),
        rotation: Quat::from_rotation_y(instance.rotation_y),
        scale: Vec3::from_array(instance.scale),
    }
}

fn decor_view_radius_chunks() -> i32 {
    packed_view_radius_chunks()
}

fn decor_despawn_radius_sq() -> i32 {
    let radius = decor_view_radius_chunks() + DESPAWN_RADIUS_EXTRA_CHUNKS;
    radius * radius
}

/// Despawn entities for chunks dirtied by world block edits and re-queue them for rebuild.
pub(crate) fn invalidate_edited_decor_chunks(
    edit_store: Res<WorldEditStore>,
    mut commands: Commands,
    mut state: ResMut<SurfaceDecorState>,
) {
    let current_gen = edit_store.generation();
    if current_gen == state.last_seen_edit_generation {
        return;
    }
    let old_gen = state.last_seen_edit_generation;
    state.last_seen_edit_generation = current_gen;

    let dirty =
        crate::feature_decor_invalidation::dirty_layer_chunks_since(
            &edit_store,
            old_gen,
            &state.loaded,
            &state.building,
        );

    for pos in dirty {
        if let Some(entity) = state.loaded.remove(&pos) {
            commands.entity(entity).despawn();
        }
        if let Some(entity) = state.building.remove(&pos) {
            commands.entity(entity).despawn();
        }
        if let Some(center) = state.last_center {
            let distance_sq = {
                let dx = center.x - pos.x;
                let dz = center.y - pos.y;
                dx * dx + dz * dz
            };
            let view_radius = decor_view_radius_chunks();
            if distance_sq <= view_radius * view_radius {
                state.pending.push(PendingDecorChunk { pos, distance_sq });
            }
        }
    }
}

fn rebuild_pending_decor(center: IVec2, state: &mut SurfaceDecorState) {
    let view_radius = decor_view_radius_chunks();
    let radius_sq = view_radius * view_radius;
    let mut wanted = HashMap::<IVec2, i32>::default();

    for dz in -view_radius..=view_radius {
        for dx in -view_radius..=view_radius {
            let distance_sq = dx * dx + dz * dz;
            if distance_sq > radius_sq {
                continue;
            }
            let chunk_pos = center + IVec2::new(dx, dz);
            wanted.insert(chunk_pos, distance_sq);
        }
    }

    let mut pending: Vec<_> = wanted
        .into_iter()
        .filter_map(|(pos, distance_sq)| {
            (!state.loaded.contains_key(&pos) && !state.building.contains_key(&pos))
                .then_some(PendingDecorChunk { pos, distance_sq })
        })
        .collect();

    pending.sort_by(|left, right| {
        left.distance_sq
            .cmp(&right.distance_sq)
            .then_with(|| left.pos.y.cmp(&right.pos.y))
            .then_with(|| left.pos.x.cmp(&right.pos.x))
    });
    state.pending = pending;
}

fn despawn_far_decor(center: IVec2, commands: &mut Commands, state: &mut SurfaceDecorState) {
    let despawn_radius_sq = decor_despawn_radius_sq();
    let loaded: Vec<_> = state
        .loaded
        .iter()
        .filter(|(pos, _)| chunk_distance_sq(center, **pos) > despawn_radius_sq)
        .map(|(pos, entity)| (*pos, *entity))
        .collect();
    for (pos, entity) in loaded {
        commands.entity(entity).despawn();
        state.loaded.remove(&pos);
    }

    let building: Vec<_> = state
        .building
        .iter()
        .filter(|(pos, _)| chunk_distance_sq(center, **pos) > despawn_radius_sq)
        .map(|(pos, entity)| (*pos, *entity))
        .collect();
    for (pos, entity) in building {
        commands.entity(entity).despawn();
        state.building.remove(&pos);
    }
}

fn chunk_distance_sq(center: IVec2, chunk: IVec2) -> i32 {
    let dx = center.x - chunk.x;
    let dz = center.y - chunk.y;
    dx * dx + dz * dz
}

fn chunk_world_origin(chunk: IVec2) -> Vec3 {
    Vec3::new(
        chunk.x as f32 * CHUNK_SIZE as f32,
        0.0,
        chunk.y as f32 * CHUNK_SIZE as f32,
    )
}

impl Clone for CrossBillboardTemplate {
    fn clone(&self) -> Self {
        Self {
            positions: self.positions,
            normals: self.normals,
            uvs: self.uvs,
            indices: self.indices,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_world::surface_decor::DecorInstance;

    fn sample_grass_instance() -> DecorInstance {
        DecorInstance {
            translation: [4.0, 1.0, 4.0],
            rotation_y: 0.5,
            scale: [1.0, 1.2, 1.0],
            custom: [0.1, 0.2, 0.0, 1.0],
        }
    }

    #[test]
    fn merge_grass_instances_builds_non_empty_mesh() {
        let template = CrossBillboardTemplate::new();
        let mesh = merge_grass_instances(&[sample_grass_instance()], &template).expect("mesh");
        assert!(mesh.count_vertices() > 0);
        assert!(mesh.indices().is_some());
    }

    #[test]
    fn leaf_clump_template_is_denser_than_grass_cross() {
        let grass = CrossBillboardTemplate::new();
        let leaf = LeafClumpTemplate::new();
        assert!(leaf.positions.len() > grass.positions.len());
        assert_eq!(leaf.positions.len(), 40);
    }

    #[test]
    fn merge_leaf_instances_builds_volumetric_mesh() {
        let template = LeafClumpTemplate::new();
        let mesh = merge_leaf_instances(&[sample_grass_instance()], &template).expect("mesh");
        assert_eq!(mesh.count_vertices(), 40);
    }
}
