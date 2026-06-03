use bevy::{
    ecs::system::SystemParam,
    image::BevyDefault,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        graph::CameraDriverLabel,
        mesh::{
            RenderMesh, RenderMeshBufferInfo,
            allocator::{MeshAllocator, MeshBufferSlice},
        },
        render_asset::RenderAssets,
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{
            binding_types::{
                sampler, storage_buffer_read_only_sized, storage_buffer_sized, texture_2d,
                texture_2d_array, uniform_buffer,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::GpuImage,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use bevy_asset::{embedded_asset, load_embedded_asset};
use rumpel_prelude::*;
use std::{
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    RenderedChunk, RenderedChunkCount,
    voxel_material::{
        ATTRIBUTE_VOXEL_REPEAT_UV, ATTRIBUTE_VOXEL_TILE, VoxelQuadMaterial, load_block_atlas,
    },
};

const CHUNK_BLOCK_COUNT: usize = 32 * 32 * 32;
const CHUNK_FACE_BLOCK_COUNT: usize = 32 * 32;
const COMPUTE_BOUNDARY_FACE_COUNT: usize = 6;
const COMPUTE_BOUNDARY_BLOCK_WORDS: usize = CHUNK_FACE_BLOCK_COUNT * COMPUTE_BOUNDARY_FACE_COUNT;
const COUNTER_BUFFER_SIZE: u64 = 8;
const GPU_COUNTERS_ENV: &str = "RUMPEL_GPU_COUNTERS";
const GPU_COMPUTE_MAX_JOBS_PER_FRAME_ENV: &str = "RUMPEL_GPU_COMPUTE_MAX_JOBS_PER_FRAME";
const GPU_COMPUTE_QUEUE_RADIUS_ENV: &str = "RUMPEL_GPU_COMPUTE_QUEUE_RADIUS";
const COMPUTE_DIRECT_RENDER_ENV: &str = "RUMPEL_COMPUTE_DIRECT_RENDER";
const COMPUTE_DIRECT_INDIRECT_ENV: &str = "RUMPEL_COMPUTE_DIRECT_INDIRECT";
const COMPUTE_DIRECT_MULTI_INDIRECT_ENV: &str = "RUMPEL_COMPUTE_DIRECT_MULTI_INDIRECT";
const COMPUTE_DIRECT_GPU_CULL_ENV: &str = "RUMPEL_COMPUTE_DIRECT_GPU_CULL";
const COMPUTE_DIRECT_GPU_CULL_COMPACT_ENV: &str = "RUMPEL_COMPUTE_DIRECT_GPU_CULL_COMPACT";
const COMPUTE_DIRECT_GPU_OCCLUSION_CULL_ENV: &str = "RUMPEL_COMPUTE_DIRECT_OCCLUSION_CULL";
const VERTEX_WORDS_PER_VERTEX: u32 = 13;
const COMPUTE_WORD_SIZE_BYTES: u64 = 4;
const COMPUTE_DIRECT_DRAW_COMMAND_BYTES: u64 =
    std::mem::size_of::<VoxelComputeDirectDrawCommand>() as u64;
const COMPUTE_DIRECT_CULL_WORKGROUP_SIZE: usize = 64;
const COMPUTE_WORKGROUP_DISPATCH: u32 = 8;
const MESH_VERTEX_CAPACITY: usize = 65_536;
const MESH_INDEX_CAPACITY: usize = 98_304;
const COMPUTE_TEXTURE_PALETTE_BLOCKS: usize = 256;
const COMPUTE_TEXTURES_PER_BLOCK: usize = 3;
const COMPUTE_TEXTURE_PALETTE_WORDS: usize =
    COMPUTE_TEXTURE_PALETTE_BLOCKS * COMPUTE_TEXTURES_PER_BLOCK;
const COMPUTE_FALLBACK_TEXTURE_TILE: u32 = 3;
const COMPUTE_VERTICAL_LAYER_HEIGHT: i32 = 32;
const COMPUTE_VERTICAL_LAYER_BASES: [i32; 2] = [0, COMPUTE_VERTICAL_LAYER_HEIGHT];
const COMPUTE_PARITY_CHUNK_POS: ChunkPos = ChunkPos { x: 0, z: 0 };
const DEFAULT_GPU_COMPUTE_MAX_JOBS_PER_FRAME: usize = 8;
const DEFAULT_GPU_COMPUTE_QUEUE_RADIUS: i32 = 3;
const COMPUTE_NEIGHBOR_PLUS_X: u32 = 1 << 0;
const COMPUTE_NEIGHBOR_MINUS_X: u32 = 1 << 1;
const COMPUTE_NEIGHBOR_PLUS_Z: u32 = 1 << 2;
const COMPUTE_NEIGHBOR_MINUS_Z: u32 = 1 << 3;
const COMPUTE_NEIGHBOR_PLUS_Y: u32 = 1 << 4;
const COMPUTE_NEIGHBOR_MINUS_Y: u32 = 1 << 5;
const COMPUTE_DIRECT_VIEW_BUFFER_SIZE: u64 =
    std::mem::size_of::<VoxelComputeDirectViewBuffer>() as u64;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct VoxelComputeLabel;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct VoxelComputeDirectRenderLabel;

pub struct VoxelComputePlugin;

impl Plugin for VoxelComputePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../assets/shaders/mesh_generator.wgsl");
        embedded_asset!(app, "../assets/shaders/compute_terrain_direct.wgsl");
        embedded_asset!(app, "../assets/shaders/compute_terrain_cull.wgsl");
        let atlas = {
            let asset_server = app.world().resource::<AssetServer>();
            load_block_atlas(asset_server)
        };
        app.insert_resource(VoxelComputeBlockAtlas { handle: atlas });
        app.add_plugins(ExtractResourcePlugin::<VoxelComputeBlockAtlas>::default());
        app.add_message::<WorldBlockEdit>();
        app.add_message::<VoxelComputeBlockEdit>();
        app.init_resource::<WorldEditStore>();
        app.init_resource::<SingleChunkExtract>();
        app.add_plugins(ExtractComponentPlugin::<GenerateChunkMesh>::default());
        app.add_plugins(ExtractComponentPlugin::<VoxelComputeMeshContract>::default());
        app.add_plugins(ExtractComponentPlugin::<VoxelComputeChunkSource>::default());
        app.add_systems(OnEnter(GameState::Loading), setup_compute_queue_chunks);
        app.add_systems(
            Update,
            (
                bridge_world_block_edits_to_voxel_compute_edits,
                apply_voxel_compute_block_edits,
            )
                .chain(),
        );

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<ChunksToProcess>();
        render_app.init_resource::<VoxelComputeLifecycle>();
        render_app.init_resource::<VoxelComputeDirectRenderTelemetry>();
        render_app.insert_resource(VoxelComputeSettings::from_env());
        render_app.add_systems(
            Render,
            (
                collect_compute_counter_readback,
                prepare_voxel_compute_direct_view_uniforms,
                prepare_chunks,
            )
                .chain()
                .in_set(RenderSystems::Prepare),
        );

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(VoxelComputeLabel, VoxelComputeNode::default());
        render_graph.add_node_edge(VoxelComputeLabel, CameraDriverLabel);
    }

    fn finish(&self, app: &mut App) {
        let asset_server = app.world().resource::<AssetServer>();
        let shader = load_embedded_asset!(asset_server, "../assets/shaders/mesh_generator.wgsl");
        let direct_shader = load_embedded_asset!(
            asset_server,
            "../assets/shaders/compute_terrain_direct.wgsl"
        );
        let cull_shader =
            load_embedded_asset!(asset_server, "../assets/shaders/compute_terrain_cull.wgsl");
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        let render_device = render_app.world().resource::<RenderDevice>().clone();
        let pipeline_cache = render_app.world().resource::<PipelineCache>();

        let entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                uniform_buffer::<DataRanges>(false),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
            ),
        );

        let bind_group_layout =
            render_device.create_bind_group_layout("voxel_compute_bind_group_layout", &entries);

        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("voxel_compute_pipeline".into()),
            layout: vec![BindGroupLayoutDescriptor::new(
                "voxel_compute_bind_group_layout",
                &entries,
            )],
            shader,
            shader_defs: vec![],
            entry_point: Some(std::borrow::Cow::Borrowed("main")),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        let direct_view_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX,
            (storage_buffer_read_only_sized(false, None),),
        );
        let direct_view_bind_group_layout = render_device
            .create_bind_group_layout("voxel_compute_direct_view_layout", &direct_view_entries);
        let direct_view_layout_desc = BindGroupLayoutDescriptor::new(
            "voxel_compute_direct_view_layout",
            &direct_view_entries,
        );

        let direct_terrain_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        );
        let direct_terrain_bind_group_layout = render_device.create_bind_group_layout(
            "voxel_compute_direct_terrain_layout",
            &direct_terrain_entries,
        );
        let direct_terrain_layout_desc = BindGroupLayoutDescriptor::new(
            "voxel_compute_direct_terrain_layout",
            &direct_terrain_entries,
        );

        let direct_pipeline = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("voxel_compute_direct_render_pipeline".into()),
            layout: vec![direct_view_layout_desc, direct_terrain_layout_desc],
            vertex: VertexState {
                shader: direct_shader.clone(),
                shader_defs: vec![],
                entry_point: Some("vertex".into()),
                buffers: vec![],
            },
            fragment: Some(FragmentState {
                shader: direct_shader,
                shader_defs: vec![],
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        let cull_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                texture_2d(TextureSampleType::Depth),
            ),
        );
        let cull_bind_group_layout = render_device
            .create_bind_group_layout("voxel_compute_direct_cull_layout", &cull_entries);
        let cull_layout_desc =
            BindGroupLayoutDescriptor::new("voxel_compute_direct_cull_layout", &cull_entries);
        let cull_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("voxel_compute_direct_cull_pipeline".into()),
            layout: vec![cull_layout_desc],
            shader: cull_shader,
            shader_defs: vec![],
            entry_point: Some(std::borrow::Cow::Borrowed("main")),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        render_app.insert_resource(VoxelComputePipeline {
            pipeline,
            bind_group_layout,
        });
        render_app.insert_resource(VoxelComputeDirectPipeline {
            pipeline: direct_pipeline,
            view_bind_group_layout: direct_view_bind_group_layout,
            terrain_bind_group_layout: direct_terrain_bind_group_layout,
        });
        render_app.insert_resource(VoxelComputeDirectCullPipeline {
            pipeline: cull_pipeline,
            bind_group_layout: cull_bind_group_layout,
        });
        render_app.init_resource::<VoxelComputeBuffers>();

        render_app
            .world_mut()
            .resource_mut::<bevy::render::mesh::allocator::MeshAllocator>()
            .extra_buffer_usages = BufferUsages::STORAGE;

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        if let Some(core_3d_graph) =
            render_graph.get_sub_graph_mut(bevy::core_pipeline::core_3d::graph::Core3d)
        {
            core_3d_graph.add_node(
                VoxelComputeDirectRenderLabel,
                VoxelComputeDirectRenderNode::default(),
            );
            core_3d_graph.add_node_edge(
                bevy::core_pipeline::core_3d::graph::Node3d::EndMainPass,
                VoxelComputeDirectRenderLabel,
            );
            core_3d_graph.add_node_edge(
                VoxelComputeDirectRenderLabel,
                bevy::core_pipeline::core_3d::graph::Node3d::StartMainPassPostProcessing,
            );
        }
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub struct GenerateChunkMesh(pub Handle<Mesh>);

#[derive(Resource, Clone)]
struct VoxelComputeBlockAtlas {
    handle: Handle<Image>,
}

impl ExtractResource for VoxelComputeBlockAtlas {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

#[derive(Component)]
struct VoxelComputeDirectViewUniform {
    buffer: Buffer,
    bind_group: BindGroup,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct VoxelComputeDirectViewBuffer {
    view_projection_columns: [[f32; 4]; 4],
}

unsafe impl bytemuck::Zeroable for VoxelComputeDirectViewBuffer {}
unsafe impl bytemuck::Pod for VoxelComputeDirectViewBuffer {}

#[derive(Clone, Copy)]
#[repr(C)]
struct VoxelComputeDirectChunkParams {
    chunk_offset: [f32; 4],
    draw: [u32; 4],
    offsets: [u32; 4],
}

unsafe impl bytemuck::Zeroable for VoxelComputeDirectChunkParams {}
unsafe impl bytemuck::Pod for VoxelComputeDirectChunkParams {}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct VoxelComputeDirectCullMetadata {
    bounds_min: [f32; 4],
    bounds_max: [f32; 4],
}

unsafe impl bytemuck::Zeroable for VoxelComputeDirectCullMetadata {}
unsafe impl bytemuck::Pod for VoxelComputeDirectCullMetadata {}

#[derive(Clone, Copy)]
#[repr(C)]
struct VoxelComputeDirectCullConfig {
    clip_from_world_columns: [[f32; 4]; 4],
    draw: [u32; 4],
}

unsafe impl bytemuck::Zeroable for VoxelComputeDirectCullConfig {}
unsafe impl bytemuck::Pod for VoxelComputeDirectCullConfig {}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct VoxelComputeDirectDrawCommand {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

unsafe impl bytemuck::Zeroable for VoxelComputeDirectDrawCommand {}
unsafe impl bytemuck::Pod for VoxelComputeDirectDrawCommand {}

#[derive(Clone, Copy, Debug, Message)]
pub struct VoxelComputeBlockEdit {
    pub chunk_pos: ChunkPos,
    pub local_pos: LocalBlockPos,
    pub block: BlockId,
}

impl VoxelComputeBlockEdit {
    #[must_use]
    pub fn new(chunk_pos: ChunkPos, local_pos: LocalBlockPos, block: BlockId) -> Self {
        Self {
            chunk_pos,
            local_pos,
            block,
        }
    }

    fn layer_local(self) -> Option<(ComputeChunkKey, usize, usize, usize)> {
        let x = usize::from(self.local_pos.x);
        let z = usize::from(self.local_pos.z);
        if x >= CHUNK_SIZE || z >= CHUNK_SIZE {
            return None;
        }

        let local_y = i32::from(self.local_pos.y);
        let y_base =
            local_y.div_euclid(COMPUTE_VERTICAL_LAYER_HEIGHT) * COMPUTE_VERTICAL_LAYER_HEIGHT;
        let y = usize::try_from(local_y - y_base).ok()?;
        if y >= CHUNK_SIZE {
            return None;
        }

        Some((
            ComputeChunkKey {
                chunk_pos: self.chunk_pos,
                y_base,
            },
            x,
            y,
            z,
        ))
    }
}

impl From<WorldBlockEdit> for VoxelComputeBlockEdit {
    fn from(edit: WorldBlockEdit) -> Self {
        Self::new(edit.chunk_pos, edit.local_pos, edit.block)
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub struct VoxelComputeChunkSource {
    blocks: Box<[u32; CHUNK_BLOCK_COUNT]>,
    boundary_blocks: Box<[u32; COMPUTE_BOUNDARY_BLOCK_WORDS]>,
    texture_tiles: Box<[u32; COMPUTE_TEXTURE_PALETTE_WORDS]>,
    generation: u64,
}

impl VoxelComputeChunkSource {
    fn new(
        blocks: Box<[u32; CHUNK_BLOCK_COUNT]>,
        boundary_blocks: Box<[u32; COMPUTE_BOUNDARY_BLOCK_WORDS]>,
        texture_tiles: Box<[u32; COMPUTE_TEXTURE_PALETTE_WORDS]>,
    ) -> Self {
        Self {
            blocks,
            boundary_blocks,
            texture_tiles,
            generation: 0,
        }
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn mark_dirty(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub struct VoxelComputeMeshContract {
    chunk_pos: ChunkPos,
    y_base: i32,
    neighbor_mask: u32,
    solid_blocks: usize,
    expected_visible_faces: u32,
    expected_vertices: u32,
    expected_indices: u32,
}

impl VoxelComputeMeshContract {
    fn chunk_key(&self) -> ComputeChunkKey {
        ComputeChunkKey {
            chunk_pos: self.chunk_pos,
            y_base: self.y_base,
        }
    }
}

fn setup_compute_queue_chunks(
    mut commands: Commands,
    mut params: ComputeQueueSetupParams,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }

    let context = WorldGenerationContext::from_registry(&params.registry);
    let chunk_positions = compute_queue_chunk_positions();
    let compute_chunks = compute_queue_chunk_keys(&chunk_positions);
    let compute_chunk_set = compute_chunks.iter().copied().collect::<HashSet<_>>();

    info!(
        chunks = compute_chunks.len(),
        "voxel compute queue generated from rumpel_world"
    );

    let material = params.materials.add(VoxelQuadMaterial {
        atlas: load_block_atlas(&params.asset_server),
    });
    let texture_tiles = copy_texture_palette_to_compute_buffer(&params.registry);
    let direct_render = compute_direct_render_enabled_from_env();

    for chunk_key in compute_chunks {
        let source_chunk = generate_compute_layer_chunk(
            chunk_key.chunk_pos,
            chunk_key.y_base,
            &context,
            &params.edit_store,
        );
        if chunk_key.chunk_pos == COMPUTE_PARITY_CHUNK_POS && chunk_key.y_base == 0 {
            copy_chunk_data_to_single_chunk_extract(&source_chunk, &mut params.single_chunk);
        }
        let boundary_blocks = copy_boundary_blocks_to_compute_buffer(
            chunk_key,
            &context,
            &params.edit_store,
            &compute_chunk_set,
        );
        let neighbor_mask = compute_neighbor_mask(chunk_key, &compute_chunk_set);
        let blocks = copy_source_chunk_to_compute_buffer(&source_chunk);
        let contract = compute_mesh_contract_from_compute_blocks(
            chunk_key.chunk_pos,
            chunk_key.y_base,
            &blocks,
            &boundary_blocks,
            context.palette.air,
            neighbor_mask,
        );
        let handle = params.meshes.add(empty_compute_mesh());

        info!(
            chunk_pos = ?chunk_key.chunk_pos,
            y_base = chunk_key.y_base,
            neighbor_mask,
            solid_blocks = contract.solid_blocks,
            expected_visible_faces = contract.expected_visible_faces,
            expected_vertices = contract.expected_vertices,
            expected_indices = contract.expected_indices,
            "voxel compute queue chunk prepared"
        );

        let mut entity = commands.spawn((
            GenerateChunkMesh(handle.clone()),
            VoxelComputeChunkSource::new(blocks, boundary_blocks, texture_tiles.clone()),
            contract,
            RenderedChunk,
            RenderedChunkCount(1),
            Transform::from_xyz(
                (chunk_key.chunk_pos.x * 32) as f32,
                chunk_key.y_base as f32,
                (chunk_key.chunk_pos.z * 32) as f32,
            ),
        ));

        if !direct_render {
            entity.insert((Mesh3d(handle.clone()), MeshMaterial3d(material.clone())));
        }
    }

    *spawned = true;
}

#[derive(SystemParam)]
struct ComputeQueueSetupParams<'w> {
    asset_server: Res<'w, AssetServer>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<VoxelQuadMaterial>>,
    registry: Res<'w, BlockRegistry>,
    edit_store: Res<'w, WorldEditStore>,
    single_chunk: ResMut<'w, SingleChunkExtract>,
}

fn apply_voxel_compute_block_edits(
    mut edits: MessageReader<VoxelComputeBlockEdit>,
    mut chunks: Query<(
        &'static mut VoxelComputeChunkSource,
        &'static mut VoxelComputeMeshContract,
    )>,
) {
    let pending_edits = edits.read().copied().collect::<Vec<_>>();
    if pending_edits.is_empty() {
        return;
    }

    let mut applied_edits = 0;
    let mut ignored_edits = 0;
    let mut touched_chunks = HashSet::<ComputeChunkKey>::default();

    for edit in pending_edits {
        let mut edit_touched_chunks = 0;
        for (mut source, mut contract) in &mut chunks {
            if apply_compute_block_edit_to_source(&edit, &mut source, &mut contract) {
                touched_chunks.insert(contract.chunk_key());
                edit_touched_chunks += 1;
            }
        }

        if edit_touched_chunks == 0 {
            ignored_edits += 1;
        } else {
            applied_edits += 1;
        }
    }

    info!(
        applied_edits,
        ignored_edits,
        touched_chunks = touched_chunks.len(),
        "voxel compute block edits applied"
    );
}

fn bridge_world_block_edits_to_voxel_compute_edits(
    mut world_edits: MessageReader<WorldBlockEdit>,
    mut compute_edits: MessageWriter<VoxelComputeBlockEdit>,
) {
    let mut bridged_edits = 0;
    for edit in world_edits.read().copied() {
        compute_edits.write(edit.into());
        bridged_edits += 1;
    }

    if bridged_edits > 0 {
        info!(bridged_edits, "world block edits bridged to voxel compute");
    }
}

fn apply_compute_block_edit_to_source(
    edit: &VoxelComputeBlockEdit,
    source: &mut VoxelComputeChunkSource,
    contract: &mut VoxelComputeMeshContract,
) -> bool {
    let Some((edited_key, x, y, z)) = edit.layer_local() else {
        return false;
    };

    let mut changed = false;
    let chunk_key = contract.chunk_key();
    let block = u32::from(edit.block);

    if chunk_key == edited_key {
        let index = compute_block_index(x, y, z);
        if source.blocks[index] != block {
            source.blocks[index] = block;
            changed = true;
        }
    }

    if let Some(offset) = boundary_offset_for_edited_neighbor(chunk_key, edited_key, x, y, z)
        && source.boundary_blocks[offset] != block
    {
        source.boundary_blocks[offset] = block;
        changed = true;
    }

    if changed {
        source.mark_dirty();
        *contract = compute_mesh_contract_from_compute_source(source, contract);
    }

    changed
}

fn empty_compute_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.; 3]; MESH_VERTEX_CAPACITY],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.; 3]; MESH_VERTEX_CAPACITY])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.; 4]; MESH_VERTEX_CAPACITY])
    .with_inserted_attribute(
        ATTRIBUTE_VOXEL_REPEAT_UV,
        vec![[0.; 2]; MESH_VERTEX_CAPACITY],
    )
    .with_inserted_attribute(ATTRIBUTE_VOXEL_TILE, vec![0_u32; MESH_VERTEX_CAPACITY])
    .with_inserted_indices(bevy::mesh::Indices::U32(vec![0; MESH_INDEX_CAPACITY]));

    mesh.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    mesh
}

fn compute_queue_chunk_positions() -> Vec<ChunkPos> {
    compute_queue_chunk_positions_for_radius(gpu_compute_queue_radius_from_env())
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ComputeChunkKey {
    chunk_pos: ChunkPos,
    y_base: i32,
}

fn compute_queue_chunk_keys(chunk_positions: &[ChunkPos]) -> Vec<ComputeChunkKey> {
    chunk_positions
        .iter()
        .flat_map(|&chunk_pos| {
            COMPUTE_VERTICAL_LAYER_BASES.map(|y_base| ComputeChunkKey { chunk_pos, y_base })
        })
        .collect()
}

fn compute_queue_chunk_positions_for_radius(radius: i32) -> Vec<ChunkPos> {
    let radius_sq = radius * radius;
    let mut positions = Vec::new();

    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dz * dz <= radius_sq {
                positions.push(ChunkPos {
                    x: COMPUTE_PARITY_CHUNK_POS.x + dx,
                    z: COMPUTE_PARITY_CHUNK_POS.z + dz,
                });
            }
        }
    }

    positions.sort_by_key(|pos| {
        let dx = pos.x - COMPUTE_PARITY_CHUNK_POS.x;
        let dz = pos.z - COMPUTE_PARITY_CHUNK_POS.z;
        (dx * dx + dz * dz, pos.z, pos.x)
    });
    positions
}

fn copy_source_chunk_to_compute_buffer(source: &ChunkData) -> Box<[u32; CHUNK_BLOCK_COUNT]> {
    let mut blocks = Box::new([0; CHUNK_BLOCK_COUNT]);
    for (target_block, source_block) in blocks.iter_mut().zip(source.blocks.iter()) {
        *target_block = u32::from(*source_block);
    }
    blocks
}

fn copy_chunk_data_to_single_chunk_extract(source: &ChunkData, target: &mut SingleChunkExtract) {
    for (target_block, source_block) in target.blocks.iter_mut().zip(source.blocks.iter()) {
        *target_block = u32::from(*source_block);
    }
    target.has_changes = false;
}

fn copy_boundary_blocks_to_compute_buffer(
    chunk_key: ComputeChunkKey,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    positions: &HashSet<ComputeChunkKey>,
) -> Box<[u32; COMPUTE_BOUNDARY_BLOCK_WORDS]> {
    let mut blocks = Box::new([u32::from(context.palette.air); COMPUTE_BOUNDARY_BLOCK_WORDS]);
    for face in BoundaryFace::ALL {
        let neighbor_key = face.neighbor_key(chunk_key);
        if !positions.contains(&neighbor_key) {
            continue;
        }
        let neighbor = generate_compute_layer_chunk(
            neighbor_key.chunk_pos,
            neighbor_key.y_base,
            context,
            edit_store,
        );
        copy_boundary_face_blocks(&mut blocks, face, &neighbor);
    }
    blocks
}

fn copy_boundary_face_blocks(
    blocks: &mut [u32; COMPUTE_BOUNDARY_BLOCK_WORDS],
    face: BoundaryFace,
    neighbor: &ChunkData,
) {
    for a in 0..CHUNK_SIZE {
        for b in 0..CHUNK_SIZE {
            let (x, y, z) = face.neighbor_sample(a, b);
            let target = face.buffer_offset(a, b);
            blocks[target] = u32::from(neighbor.get_block(x, y, z));
        }
    }
}

#[derive(Clone, Copy)]
enum BoundaryFace {
    PlusX,
    MinusX,
    PlusY,
    MinusY,
    PlusZ,
    MinusZ,
}

impl BoundaryFace {
    const ALL: [Self; COMPUTE_BOUNDARY_FACE_COUNT] = [
        Self::PlusX,
        Self::MinusX,
        Self::PlusY,
        Self::MinusY,
        Self::PlusZ,
        Self::MinusZ,
    ];

    fn index(self) -> usize {
        match self {
            Self::PlusX => 0,
            Self::MinusX => 1,
            Self::PlusY => 2,
            Self::MinusY => 3,
            Self::PlusZ => 4,
            Self::MinusZ => 5,
        }
    }

    fn from_direction(dx: i32, dy: i32, dz: i32) -> Self {
        if dx > 0 {
            Self::PlusX
        } else if dx < 0 {
            Self::MinusX
        } else if dy > 0 {
            Self::PlusY
        } else if dy < 0 {
            Self::MinusY
        } else if dz > 0 {
            Self::PlusZ
        } else {
            Self::MinusZ
        }
    }

    fn neighbor_key(self, chunk_key: ComputeChunkKey) -> ComputeChunkKey {
        match self {
            Self::PlusX => ComputeChunkKey {
                chunk_pos: ChunkPos {
                    x: chunk_key.chunk_pos.x + 1,
                    z: chunk_key.chunk_pos.z,
                },
                y_base: chunk_key.y_base,
            },
            Self::MinusX => ComputeChunkKey {
                chunk_pos: ChunkPos {
                    x: chunk_key.chunk_pos.x - 1,
                    z: chunk_key.chunk_pos.z,
                },
                y_base: chunk_key.y_base,
            },
            Self::PlusY => ComputeChunkKey {
                chunk_pos: chunk_key.chunk_pos,
                y_base: chunk_key.y_base + COMPUTE_VERTICAL_LAYER_HEIGHT,
            },
            Self::MinusY => ComputeChunkKey {
                chunk_pos: chunk_key.chunk_pos,
                y_base: chunk_key.y_base - COMPUTE_VERTICAL_LAYER_HEIGHT,
            },
            Self::PlusZ => ComputeChunkKey {
                chunk_pos: ChunkPos {
                    x: chunk_key.chunk_pos.x,
                    z: chunk_key.chunk_pos.z + 1,
                },
                y_base: chunk_key.y_base,
            },
            Self::MinusZ => ComputeChunkKey {
                chunk_pos: ChunkPos {
                    x: chunk_key.chunk_pos.x,
                    z: chunk_key.chunk_pos.z - 1,
                },
                y_base: chunk_key.y_base,
            },
        }
    }

    fn neighbor_sample(self, a: usize, b: usize) -> (usize, usize, usize) {
        match self {
            Self::PlusX => (0, a, b),
            Self::MinusX => (CHUNK_SIZE - 1, a, b),
            Self::PlusY => (a, 0, b),
            Self::MinusY => (a, CHUNK_SIZE - 1, b),
            Self::PlusZ => (a, b, 0),
            Self::MinusZ => (a, b, CHUNK_SIZE - 1),
        }
    }

    fn buffer_offset(self, a: usize, b: usize) -> usize {
        self.index() * CHUNK_FACE_BLOCK_COUNT + a + b * CHUNK_SIZE
    }

    fn buffer_offset_for_neighbor_local(self, x: usize, y: usize, z: usize) -> Option<usize> {
        match self {
            Self::PlusX => (x == 0).then(|| self.buffer_offset(y, z)),
            Self::MinusX => (x == CHUNK_SIZE - 1).then(|| self.buffer_offset(y, z)),
            Self::PlusY => (y == 0).then(|| self.buffer_offset(x, z)),
            Self::MinusY => (y == CHUNK_SIZE - 1).then(|| self.buffer_offset(x, z)),
            Self::PlusZ => (z == 0).then(|| self.buffer_offset(x, y)),
            Self::MinusZ => (z == CHUNK_SIZE - 1).then(|| self.buffer_offset(x, y)),
        }
    }
}

fn boundary_offset_for_edited_neighbor(
    observer_key: ComputeChunkKey,
    edited_key: ComputeChunkKey,
    x: usize,
    y: usize,
    z: usize,
) -> Option<usize> {
    BoundaryFace::ALL.into_iter().find_map(|face| {
        (face.neighbor_key(observer_key) == edited_key)
            .then(|| face.buffer_offset_for_neighbor_local(x, y, z))
            .flatten()
    })
}

fn generate_compute_layer_chunk(
    chunk_pos: ChunkPos,
    y_base: i32,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
) -> ChunkData {
    let mut chunk = ChunkData::default();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let global_x = chunk_pos.x * CHUNK_SIZE as i32 + x as i32;
            let global_z = chunk_pos.z * CHUNK_SIZE as i32 + z as i32;
            let surface_height = terrain_height_at(global_x, global_z);

            for y in 0..CHUNK_SIZE {
                let world_y = y_base + y as i32;
                if world_y < 0 {
                    continue;
                }
                let block_id =
                    terrain_block_at_height(world_y as usize, surface_height, context.palette);
                if block_id != context.palette.air {
                    chunk.set_block(x, y, z, block_id);
                }
            }
        }
    }
    edit_store.apply_to_chunk_layer(chunk_pos, y_base, &mut chunk);
    chunk
}

fn copy_texture_palette_to_compute_buffer(
    registry: &BlockRegistry,
) -> Box<[u32; COMPUTE_TEXTURE_PALETTE_WORDS]> {
    let mut tiles = Box::new([COMPUTE_FALLBACK_TEXTURE_TILE; COMPUTE_TEXTURE_PALETTE_WORDS]);
    if let Ok(mappings) = registry.texture_mappings.read() {
        copy_texture_mappings_to_buffer(&mut tiles, mappings.iter());
    }
    tiles
}

fn copy_texture_mappings_to_buffer<'a>(
    tiles: &mut [u32; COMPUTE_TEXTURE_PALETTE_WORDS],
    mappings: impl IntoIterator<Item = (&'a BlockId, &'a [u32; 3])>,
) {
    for (&block_id, &textures) in mappings {
        let block_index = usize::from(block_id);
        if block_index >= COMPUTE_TEXTURE_PALETTE_BLOCKS {
            continue;
        }
        let offset = block_index * COMPUTE_TEXTURES_PER_BLOCK;
        tiles[offset] = textures[0];
        tiles[offset + 1] = textures[1];
        tiles[offset + 2] = textures[2];
    }
}

fn compute_mesh_contract_from_compute_source(
    source: &VoxelComputeChunkSource,
    contract: &VoxelComputeMeshContract,
) -> VoxelComputeMeshContract {
    compute_mesh_contract_from_compute_blocks(
        contract.chunk_pos,
        contract.y_base,
        &source.blocks,
        &source.boundary_blocks,
        AIR_BLOCK_ID,
        contract.neighbor_mask,
    )
}

fn compute_mesh_contract_from_compute_blocks(
    chunk_pos: ChunkPos,
    y_base: i32,
    blocks: &[u32; CHUNK_BLOCK_COUNT],
    boundary_blocks: &[u32; COMPUTE_BOUNDARY_BLOCK_WORDS],
    air: BlockId,
    neighbor_mask: u32,
) -> VoxelComputeMeshContract {
    let mut solid_blocks = 0usize;
    let mut visible_faces = 0u32;
    let air = u32::from(air);

    for z in 0..32 {
        for y in 0..32 {
            for x in 0..32 {
                if block_at_compute_blocks(blocks, x, y, z) == air {
                    continue;
                }

                solid_blocks += 1;
                visible_faces += visible_face_count_for_block(
                    blocks,
                    boundary_blocks,
                    air,
                    neighbor_mask,
                    x,
                    y,
                    z,
                );
            }
        }
    }

    VoxelComputeMeshContract {
        chunk_pos,
        y_base,
        neighbor_mask,
        solid_blocks,
        expected_visible_faces: visible_faces,
        expected_vertices: visible_faces * 4,
        expected_indices: visible_faces * 6,
    }
}

fn compute_neighbor_mask(chunk_key: ComputeChunkKey, positions: &HashSet<ComputeChunkKey>) -> u32 {
    let mut mask = 0;
    if positions.contains(&ComputeChunkKey {
        chunk_pos: ChunkPos {
            x: chunk_key.chunk_pos.x + 1,
            z: chunk_key.chunk_pos.z,
        },
        y_base: chunk_key.y_base,
    }) {
        mask |= COMPUTE_NEIGHBOR_PLUS_X;
    }
    if positions.contains(&ComputeChunkKey {
        chunk_pos: ChunkPos {
            x: chunk_key.chunk_pos.x - 1,
            z: chunk_key.chunk_pos.z,
        },
        y_base: chunk_key.y_base,
    }) {
        mask |= COMPUTE_NEIGHBOR_MINUS_X;
    }
    if positions.contains(&ComputeChunkKey {
        chunk_pos: ChunkPos {
            x: chunk_key.chunk_pos.x,
            z: chunk_key.chunk_pos.z + 1,
        },
        y_base: chunk_key.y_base,
    }) {
        mask |= COMPUTE_NEIGHBOR_PLUS_Z;
    }
    if positions.contains(&ComputeChunkKey {
        chunk_pos: ChunkPos {
            x: chunk_key.chunk_pos.x,
            z: chunk_key.chunk_pos.z - 1,
        },
        y_base: chunk_key.y_base,
    }) {
        mask |= COMPUTE_NEIGHBOR_MINUS_Z;
    }
    if positions.contains(&ComputeChunkKey {
        chunk_pos: chunk_key.chunk_pos,
        y_base: chunk_key.y_base + COMPUTE_VERTICAL_LAYER_HEIGHT,
    }) {
        mask |= COMPUTE_NEIGHBOR_PLUS_Y;
    }
    if positions.contains(&ComputeChunkKey {
        chunk_pos: chunk_key.chunk_pos,
        y_base: chunk_key.y_base - COMPUTE_VERTICAL_LAYER_HEIGHT,
    }) {
        mask |= COMPUTE_NEIGHBOR_MINUS_Y;
    }
    mask
}

fn visible_face_count_for_block(
    blocks: &[u32; CHUNK_BLOCK_COUNT],
    boundary_blocks: &[u32; COMPUTE_BOUNDARY_BLOCK_WORDS],
    air: u32,
    _neighbor_mask: u32,
    x: i32,
    y: i32,
    z: i32,
) -> u32 {
    const DIRECTIONS: [(i32, i32, i32); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    DIRECTIONS
        .iter()
        .filter(|&&(dx, dy, dz)| {
            let nx = x + dx;
            let ny = y + dy;
            let nz = z + dz;
            if is_inside_compute_chunk(nx, ny, nz) {
                block_at_compute_blocks(blocks, nx, ny, nz) == air
            } else {
                boundary_block_at(boundary_blocks, dx, dy, dz, x, y, z) == air
            }
        })
        .count()
        .try_into()
        .expect("block face count fits in u32")
}

fn boundary_block_at(
    boundary_blocks: &[u32; COMPUTE_BOUNDARY_BLOCK_WORDS],
    dx: i32,
    dy: i32,
    dz: i32,
    x: i32,
    y: i32,
    z: i32,
) -> u32 {
    let face = BoundaryFace::from_direction(dx, dy, dz);
    let (a, b) = match face {
        BoundaryFace::PlusX | BoundaryFace::MinusX => (y as usize, z as usize),
        BoundaryFace::PlusY | BoundaryFace::MinusY => (x as usize, z as usize),
        BoundaryFace::PlusZ | BoundaryFace::MinusZ => (x as usize, y as usize),
    };
    boundary_blocks[face.buffer_offset(a, b)]
}

fn is_inside_compute_chunk(x: i32, y: i32, z: i32) -> bool {
    (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z)
}

fn block_at_compute_blocks(blocks: &[u32; CHUNK_BLOCK_COUNT], x: i32, y: i32, z: i32) -> u32 {
    blocks[compute_block_index(x as usize, y as usize, z as usize)]
}

fn compute_block_index(x: usize, y: usize, z: usize) -> usize {
    x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
}

fn prepare_voxel_compute_direct_view_uniforms(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline: Option<Res<VoxelComputeDirectPipeline>>,
    settings: Res<VoxelComputeSettings>,
    views: Query<(
        Entity,
        &'static ExtractedView,
        Option<&'static VoxelComputeDirectViewUniform>,
    )>,
) {
    if !settings.direct_render {
        return;
    }
    let Some(pipeline) = pipeline else {
        return;
    };

    for (entity, extracted_view, existing_uniform) in &views {
        let clip_from_world = compute_direct_clip_from_world(extracted_view);
        let view_data = VoxelComputeDirectViewBuffer {
            view_projection_columns: clip_from_world.to_cols_array_2d(),
        };

        if let Some(uniform) = existing_uniform {
            render_queue.write_buffer(&uniform.buffer, 0, bytemuck::bytes_of(&view_data));
            continue;
        }

        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_view_buffer"),
            size: COMPUTE_DIRECT_VIEW_BUFFER_SIZE,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        render_queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&view_data));

        let bind_group = render_device.create_bind_group(
            Some("voxel_compute_direct_view_bind_group"),
            &pipeline.view_bind_group_layout,
            &BindGroupEntries::sequential((buffer.as_entire_buffer_binding(),)),
        );

        commands
            .entity(entity)
            .insert(VoxelComputeDirectViewUniform { buffer, bind_group });
    }
}

#[derive(Component)]
pub struct VoxelComputeMesh;

#[derive(Resource, Default)]
struct ChunksToProcess(Vec<ComputeChunkToProcess>);

struct ComputeChunkToProcess {
    mesh_id: AssetId<Mesh>,
    source_generation: u64,
    contract: Option<VoxelComputeMeshContract>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComputeLifecycleState {
    Pending,
    Building,
    Loaded,
}

struct ComputeLifecycleEntry {
    state: ComputeLifecycleState,
    source_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ComputeLifecycleStatus {
    state: ComputeLifecycleState,
    invalidated: bool,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct ComputeLifecycleSnapshot {
    pending: usize,
    building: usize,
    loaded: usize,
    total: usize,
}

#[derive(Default)]
struct VoxelComputeLifecycleInner {
    entries: HashMap<AssetId<Mesh>, ComputeLifecycleEntry>,
    last_logged: Option<ComputeLifecycleSnapshot>,
}

#[derive(Resource, Default)]
struct VoxelComputeLifecycle {
    inner: Mutex<VoxelComputeLifecycleInner>,
}

impl VoxelComputeLifecycle {
    fn retain_known_meshes(&self, current_meshes: &HashSet<AssetId<Mesh>>) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            warn!("voxel compute lifecycle state is unavailable");
            return 0;
        };
        let before = inner.entries.len();
        inner
            .entries
            .retain(|mesh_id, _| current_meshes.contains(mesh_id));
        before - inner.entries.len()
    }

    fn state_or_insert_pending(
        &self,
        mesh_id: AssetId<Mesh>,
        source_generation: u64,
    ) -> ComputeLifecycleStatus {
        let Ok(mut inner) = self.inner.lock() else {
            warn!("voxel compute lifecycle state is unavailable");
            return ComputeLifecycleStatus {
                state: ComputeLifecycleState::Loaded,
                invalidated: false,
            };
        };

        let entry = inner
            .entries
            .entry(mesh_id)
            .or_insert(ComputeLifecycleEntry {
                state: ComputeLifecycleState::Pending,
                source_generation,
            });

        if entry.source_generation != source_generation {
            entry.source_generation = source_generation;
            entry.state = ComputeLifecycleState::Pending;
            return ComputeLifecycleStatus {
                state: ComputeLifecycleState::Pending,
                invalidated: true,
            };
        }

        ComputeLifecycleStatus {
            state: entry.state,
            invalidated: false,
        }
    }

    fn mark_building(&self, mesh_id: AssetId<Mesh>, source_generation: u64) {
        self.set_state(mesh_id, source_generation, ComputeLifecycleState::Building);
    }

    fn mark_loaded(&self, mesh_id: AssetId<Mesh>, source_generation: u64) {
        self.set_state(mesh_id, source_generation, ComputeLifecycleState::Loaded);
    }

    fn mark_pending_many(&self, mesh_ids: &HashSet<AssetId<Mesh>>) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            warn!("voxel compute lifecycle state is unavailable");
            return 0;
        };

        let mut changed = 0;
        for mesh_id in mesh_ids {
            if let Some(entry) = inner.entries.get_mut(mesh_id)
                && entry.state != ComputeLifecycleState::Pending
            {
                entry.state = ComputeLifecycleState::Pending;
                changed += 1;
            }
        }
        changed
    }

    fn set_state(
        &self,
        mesh_id: AssetId<Mesh>,
        source_generation: u64,
        state: ComputeLifecycleState,
    ) {
        let Ok(mut inner) = self.inner.lock() else {
            warn!("voxel compute lifecycle state is unavailable");
            return;
        };

        let entry = inner
            .entries
            .entry(mesh_id)
            .or_insert(ComputeLifecycleEntry {
                state: ComputeLifecycleState::Pending,
                source_generation,
            });

        if entry.source_generation == source_generation {
            entry.state = state;
        }
    }

    fn log_summary(&self, frame_stats: ComputeLifecycleFrameStats) {
        let Ok(mut inner) = self.inner.lock() else {
            warn!("voxel compute lifecycle state is unavailable");
            return;
        };

        let snapshot = inner.snapshot();
        if !frame_stats.has_activity() && inner.last_logged == Some(snapshot) {
            return;
        }

        inner.last_logged = Some(snapshot);
        info!(
            pending = snapshot.pending,
            building = snapshot.building,
            loaded = snapshot.loaded,
            total = snapshot.total,
            queued_this_frame = frame_stats.queued,
            submitted_this_frame = frame_stats.submitted,
            invalidated_this_frame = frame_stats.invalidated,
            rebuilds_this_frame = frame_stats.rebuilds,
            evicted_lifecycle_this_frame = frame_stats.evicted_lifecycle,
            evicted_buffers_this_frame = frame_stats.evicted_buffers,
            cancelled_readbacks_this_frame = frame_stats.cancelled_readbacks,
            owned_output_buffers_this_frame = frame_stats.owned_output_buffers,
            owned_output_bytes_this_frame = frame_stats.owned_output_bytes,
            "voxel compute lifecycle summary"
        );
    }
}

#[derive(Clone, Copy, Default)]
struct ComputeLifecycleFrameStats {
    queued: usize,
    submitted: usize,
    invalidated: usize,
    rebuilds: usize,
    evicted_lifecycle: usize,
    evicted_buffers: usize,
    cancelled_readbacks: usize,
    owned_output_buffers: usize,
    owned_output_bytes: u64,
}

impl ComputeLifecycleFrameStats {
    fn has_activity(self) -> bool {
        self.queued > 0
            || self.submitted > 0
            || self.invalidated > 0
            || self.rebuilds > 0
            || self.evicted_lifecycle > 0
            || self.evicted_buffers > 0
            || self.cancelled_readbacks > 0
            || self.owned_output_buffers > 0
            || self.owned_output_bytes > 0
    }
}

impl VoxelComputeLifecycleInner {
    fn snapshot(&self) -> ComputeLifecycleSnapshot {
        let mut snapshot = ComputeLifecycleSnapshot {
            total: self.entries.len(),
            ..default()
        };

        for entry in self.entries.values() {
            match entry.state {
                ComputeLifecycleState::Pending => {
                    snapshot.pending += 1;
                }
                ComputeLifecycleState::Building => {
                    snapshot.building += 1;
                }
                ComputeLifecycleState::Loaded => {
                    snapshot.loaded += 1;
                }
            }
        }

        snapshot
    }
}

#[derive(Resource)]
struct VoxelComputeSettings {
    diagnostic_counter_readback: bool,
    direct_render: bool,
    direct_indirect: bool,
    direct_multi_indirect: bool,
    direct_gpu_cull: bool,
    direct_gpu_cull_compact: bool,
    direct_gpu_occlusion_cull: bool,
    max_jobs_per_frame: usize,
}

impl VoxelComputeSettings {
    fn from_env() -> Self {
        Self {
            diagnostic_counter_readback: env_flag(GPU_COUNTERS_ENV),
            direct_render: compute_direct_render_enabled_from_env(),
            direct_indirect: env_flag_default(COMPUTE_DIRECT_INDIRECT_ENV, true),
            direct_multi_indirect: env_flag_default(COMPUTE_DIRECT_MULTI_INDIRECT_ENV, true),
            direct_gpu_cull: env_flag_default(COMPUTE_DIRECT_GPU_CULL_ENV, true),
            direct_gpu_cull_compact: env_flag_default(COMPUTE_DIRECT_GPU_CULL_COMPACT_ENV, true),
            direct_gpu_occlusion_cull: env_flag_default(
                COMPUTE_DIRECT_GPU_OCCLUSION_CULL_ENV,
                true,
            ),
            max_jobs_per_frame: gpu_compute_max_jobs_per_frame_from_env(),
        }
    }
}

#[derive(SystemParam)]
struct PrepareChunksParams<'w, 's> {
    meshes_to_generate: Query<
        'w,
        's,
        (
            &'static GenerateChunkMesh,
            &'static VoxelComputeChunkSource,
            Option<&'static VoxelComputeMeshContract>,
        ),
    >,
    chunks: ResMut<'w, ChunksToProcess>,
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: Res<'w, VoxelComputePipeline>,
    direct_pipeline: Option<Res<'w, VoxelComputeDirectPipeline>>,
    block_atlas: Option<Res<'w, VoxelComputeBlockAtlas>>,
    mesh_allocator: Res<'w, MeshAllocator>,
    render_device: Res<'w, RenderDevice>,
    render_queue: Res<'w, RenderQueue>,
    gpu_images: Res<'w, RenderAssets<GpuImage>>,
    render_meshes: ResMut<'w, RenderAssets<RenderMesh>>,
    buffers: ResMut<'w, VoxelComputeBuffers>,
    settings: Res<'w, VoxelComputeSettings>,
    lifecycle: Res<'w, VoxelComputeLifecycle>,
}

fn prepare_chunks(mut params: PrepareChunksParams) {
    let pipeline_ready = params
        .pipeline_cache
        .get_compute_pipeline(params.pipeline.pipeline)
        .is_some();

    let current_meshes = params
        .meshes_to_generate
        .iter()
        .map(|(gmesh, _, _)| gmesh.0.id())
        .collect::<HashSet<_>>();
    let evicted_lifecycle_this_frame = params.lifecycle.retain_known_meshes(&current_meshes);
    let buffer_eviction = params.buffers.retain_known_meshes(&current_meshes);

    let diagnostic_readback_busy =
        params.settings.diagnostic_counter_readback && params.buffers.readback_pending();

    let direct_resources_ready = if params.settings.direct_render {
        params
            .direct_pipeline
            .as_deref()
            .zip(params.block_atlas.as_deref())
            .and_then(|(_, block_atlas)| params.gpu_images.get(&block_atlas.handle))
            .is_some()
    } else {
        true
    };

    if !pipeline_ready || diagnostic_readback_busy || !direct_resources_ready {
        params.chunks.0.clear();
        params.lifecycle.log_summary(ComputeLifecycleFrameStats {
            evicted_lifecycle: evicted_lifecycle_this_frame,
            evicted_buffers: buffer_eviction.buffer_meshes,
            cancelled_readbacks: buffer_eviction.cancelled_readbacks,
            ..default()
        });
        return;
    }

    let mut chunk_data = Vec::new();
    let mut invalidated_this_frame = 0;
    let mut rebuilds_this_frame = 0;
    let mut owned_output_stats = ComputeOutputBufferFrameStats::default();

    if params.settings.direct_render {
        let direct_layout =
            VoxelComputeOutputLayout::new(MESH_VERTEX_CAPACITY as u32, MESH_INDEX_CAPACITY as u32);
        let arena_stats = params.buffers.direct_arena.ensure_capacity(
            &params.render_device,
            current_meshes.len(),
            direct_layout,
        );
        if arena_stats.reallocated {
            invalidated_this_frame += params.lifecycle.mark_pending_many(&current_meshes);
            for output in params.buffers.output_buffers.values_mut() {
                output.direct_slot = None;
                output.direct_index_count = 0;
                output.direct_bounds_min = [0.0; 4];
                output.direct_bounds_max = [0.0; 4];
            }
        }
        owned_output_stats.bytes_allocated += arena_stats.bytes_allocated;
        if arena_stats.reallocated {
            owned_output_stats.created += 1;
        }

        if let (Some(pipeline), Some(block_atlas)) = (
            params.direct_pipeline.as_deref(),
            params.block_atlas.as_deref(),
        ) && let Some(gpu_atlas) = params.gpu_images.get(&block_atlas.handle)
        {
            params.buffers.direct_arena.prepare_bind_group(
                &params.render_device,
                pipeline,
                gpu_atlas,
            );
        }
    }

    for (gmesh, source, contract) in &params.meshes_to_generate {
        if chunk_data.len() >= params.settings.max_jobs_per_frame {
            break;
        }

        let id = gmesh.0.id();
        let source_generation = source.generation();
        let lifecycle_status = params
            .lifecycle
            .state_or_insert_pending(id, source_generation);
        if lifecycle_status.invalidated {
            invalidated_this_frame += 1;
        }
        if lifecycle_status.state != ComputeLifecycleState::Pending {
            continue;
        }
        let Some((vertex_capacity, index_capacity)) =
            compute_output_capacities(params.settings.direct_render, &params.mesh_allocator, id)
        else {
            continue;
        };
        if !params.settings.direct_render
            && let Some(contract) = contract
            && !update_render_mesh_draw_count(
                &mut params.render_meshes,
                id,
                contract.expected_indices,
                contract.expected_vertices,
            )
        {
            continue;
        }
        let output_stats = prepare_compute_output_buffers(
            &params.render_device,
            &mut params.buffers,
            id,
            vertex_capacity,
            index_capacity,
            params.settings.direct_render,
        );
        owned_output_stats.created += output_stats.created;
        owned_output_stats.bytes_allocated += output_stats.bytes_allocated;
        if params.settings.direct_render {
            prepare_compute_direct_draw_state(
                ComputeDirectBindGroupParams {
                    render_queue: &params.render_queue,
                    mesh_id: id,
                    contract,
                },
                &mut params.buffers,
            );
        }
        prepare_compute_chunk_buffer(
            &params.render_device,
            &params.render_queue,
            &mut params.buffers,
            id,
            source,
        );
        if lifecycle_status.invalidated {
            rebuilds_this_frame += 1;
        }
        chunk_data.push(ComputeChunkToProcess {
            mesh_id: id,
            source_generation,
            contract: contract.cloned(),
        });
    }

    if params.settings.direct_render {
        let output_buffers = std::mem::take(&mut params.buffers.output_buffers);
        params
            .buffers
            .direct_arena
            .refresh_draw_commands(&params.render_queue, &output_buffers);
        params.buffers.output_buffers = output_buffers;
    }

    for chunk in &chunk_data {
        params
            .lifecycle
            .mark_building(chunk.mesh_id, chunk.source_generation);
    }
    params.lifecycle.log_summary(ComputeLifecycleFrameStats {
        queued: chunk_data.len(),
        invalidated: invalidated_this_frame,
        rebuilds: rebuilds_this_frame,
        evicted_lifecycle: evicted_lifecycle_this_frame,
        evicted_buffers: buffer_eviction.buffer_meshes,
        cancelled_readbacks: buffer_eviction.cancelled_readbacks,
        owned_output_buffers: owned_output_stats.created,
        owned_output_bytes: owned_output_stats.bytes_allocated,
        ..default()
    });

    params.chunks.0 = chunk_data;
}

fn prepare_compute_output_buffers(
    render_device: &RenderDevice,
    buffers: &mut VoxelComputeBuffers,
    mesh_id: AssetId<Mesh>,
    vertex_capacity: u32,
    index_capacity: u32,
    direct_render: bool,
) -> ComputeOutputBufferFrameStats {
    let layout = VoxelComputeOutputLayout::new(vertex_capacity, index_capacity);
    let mut stats = ComputeOutputBufferFrameStats::default();

    let needs_rebuild = buffers
        .output_buffers
        .get(&mesh_id)
        .is_none_or(|buffers| buffers.layout != layout);
    if needs_rebuild {
        buffers.output_buffers.insert(
            mesh_id,
            VoxelComputeOutputBuffers {
                vertex_buffer: (!direct_render).then(|| {
                    render_device.create_buffer(&BufferDescriptor {
                        label: Some("compute_queue_owned_vertex_output_buffer"),
                        size: layout.vertex_bytes,
                        usage: BufferUsages::STORAGE
                            | BufferUsages::COPY_SRC
                            | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                }),
                index_buffer: (!direct_render).then(|| {
                    render_device.create_buffer(&BufferDescriptor {
                        label: Some("compute_queue_owned_index_output_buffer"),
                        size: layout.index_bytes,
                        usage: BufferUsages::STORAGE
                            | BufferUsages::COPY_SRC
                            | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                }),
                layout,
                direct_slot: None,
                direct_index_count: 0,
                direct_bounds_min: [0.0; 4],
                direct_bounds_max: [0.0; 4],
            },
        );
        stats.created = 1;
        if !direct_render {
            stats.bytes_allocated = layout.vertex_bytes + layout.index_bytes;
        }
    }

    stats
}

fn compute_output_capacities(
    direct_render: bool,
    mesh_allocator: &MeshAllocator,
    mesh_id: AssetId<Mesh>,
) -> Option<(u32, u32)> {
    if direct_render {
        return Some((MESH_VERTEX_CAPACITY as u32, MESH_INDEX_CAPACITY as u32));
    }

    let vertex_buffer_slice = mesh_allocator.mesh_vertex_slice(&mesh_id)?;
    let index_buffer_slice = mesh_allocator.mesh_index_slice(&mesh_id)?;
    Some((
        vertex_buffer_slice.range.end - vertex_buffer_slice.range.start,
        index_buffer_slice.range.end - index_buffer_slice.range.start,
    ))
}

struct ComputeDirectBindGroupParams<'a> {
    render_queue: &'a RenderQueue,
    mesh_id: AssetId<Mesh>,
    contract: Option<&'a VoxelComputeMeshContract>,
}

fn prepare_compute_direct_draw_state(
    params: ComputeDirectBindGroupParams,
    buffers: &mut VoxelComputeBuffers,
) {
    let Some(contract) = params.contract else {
        return;
    };
    let Some(slot) = buffers.direct_arena.assign_slot(params.mesh_id) else {
        return;
    };
    let Some(output) = buffers.output_buffers.get_mut(&params.mesh_id) else {
        return;
    };
    output.direct_slot = Some(slot);

    let index_count = contract.expected_indices.min(output.layout.index_capacity);
    let vertex_count = contract
        .expected_vertices
        .min(output.layout.vertex_capacity);
    let chunk_params = VoxelComputeDirectChunkParams {
        chunk_offset: compute_direct_chunk_offset(contract),
        draw: [index_count, vertex_count, 0, 0],
        offsets: [
            compute_direct_vertex_word_offset(slot, output.layout),
            compute_direct_index_offset(slot, output.layout),
            0,
            0,
        ],
    };
    output.direct_index_count = chunk_params.draw[0] - chunk_params.draw[0] % 3;
    let (bounds_min, bounds_max) = compute_direct_chunk_bounds(contract);
    output.direct_bounds_min = bounds_min;
    output.direct_bounds_max = bounds_max;
    buffers
        .direct_arena
        .write_chunk_params(params.render_queue, slot, chunk_params);
}

fn compute_direct_chunk_offset(contract: &VoxelComputeMeshContract) -> [f32; 4] {
    [
        (contract.chunk_pos.x * CHUNK_SIZE as i32) as f32,
        contract.y_base as f32,
        (contract.chunk_pos.z * CHUNK_SIZE as i32) as f32,
        0.0,
    ]
}

fn compute_direct_chunk_bounds(contract: &VoxelComputeMeshContract) -> ([f32; 4], [f32; 4]) {
    let min = compute_direct_chunk_offset(contract);
    (
        min,
        [
            min[0] + CHUNK_SIZE as f32,
            min[1] + CHUNK_SIZE as f32,
            min[2] + CHUNK_SIZE as f32,
            0.0,
        ],
    )
}

fn compute_direct_vertex_word_offset(slot: u32, layout: VoxelComputeOutputLayout) -> u32 {
    slot.saturating_mul(layout.vertex_word_capacity)
}

fn compute_direct_index_offset(slot: u32, layout: VoxelComputeOutputLayout) -> u32 {
    slot.saturating_mul(layout.index_capacity)
}

fn compute_output_bindings<'a>(
    settings: &VoxelComputeSettings,
    buffers: &'a VoxelComputeBuffers,
    output: &'a VoxelComputeOutputBuffers,
) -> Option<(BindingResource<'a>, BindingResource<'a>)> {
    if settings.direct_render {
        let slot = output.direct_slot?;
        return buffers.direct_arena.output_bindings(slot, output.layout);
    }

    let (Some(vertex_buffer), Some(index_buffer)) =
        (output.vertex_buffer.as_ref(), output.index_buffer.as_ref())
    else {
        return None;
    };
    Some((
        BindingResource::Buffer(vertex_buffer.as_entire_buffer_binding()),
        BindingResource::Buffer(index_buffer.as_entire_buffer_binding()),
    ))
}

fn prepare_compute_chunk_buffer(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    buffers: &mut VoxelComputeBuffers,
    mesh_id: AssetId<Mesh>,
    source: &VoxelComputeChunkSource,
) {
    let chunk_buffer = buffers.chunk_buffers.entry(mesh_id).or_insert_with(|| {
        render_device.create_buffer(&BufferDescriptor {
            label: Some("compute_queue_chunk_data_buffer"),
            size: (CHUNK_BLOCK_COUNT * 4) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });

    render_queue.write_buffer(chunk_buffer, 0, bytemuck::cast_slice(&*source.blocks));

    let boundary_buffer = buffers.boundary_buffers.entry(mesh_id).or_insert_with(|| {
        render_device.create_buffer(&BufferDescriptor {
            label: Some("compute_queue_boundary_data_buffer"),
            size: (COMPUTE_BOUNDARY_BLOCK_WORDS * 4) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });

    render_queue.write_buffer(
        boundary_buffer,
        0,
        bytemuck::cast_slice(&*source.boundary_blocks),
    );

    let texture_palette_buffer = buffers
        .texture_palette_buffers
        .entry(mesh_id)
        .or_insert_with(|| {
            render_device.create_buffer(&BufferDescriptor {
                label: Some("compute_queue_texture_palette_buffer"),
                size: (COMPUTE_TEXTURE_PALETTE_WORDS * 4) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

    render_queue.write_buffer(
        texture_palette_buffer,
        0,
        bytemuck::cast_slice(&*source.texture_tiles),
    );
}

#[derive(ShaderType)]
struct DataRanges {
    vertex_start: u32,
    vertex_end: u32,
    index_start: u32,
    index_end: u32,
    neighbor_mask: u32,
}

#[derive(Resource)]
struct VoxelComputePipeline {
    pipeline: CachedComputePipelineId,
    bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
struct VoxelComputeDirectPipeline {
    pipeline: CachedRenderPipelineId,
    view_bind_group_layout: BindGroupLayout,
    terrain_bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
struct VoxelComputeDirectCullPipeline {
    pipeline: CachedComputePipelineId,
    bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
struct VoxelComputeBuffers {
    chunk_buffers: HashMap<AssetId<Mesh>, Buffer>,
    boundary_buffers: HashMap<AssetId<Mesh>, Buffer>,
    texture_palette_buffers: HashMap<AssetId<Mesh>, Buffer>,
    output_buffers: HashMap<AssetId<Mesh>, VoxelComputeOutputBuffers>,
    direct_arena: VoxelComputeDirectArena,
    atomic_buffer: Buffer,
    counter_readback_buffer: Buffer,
    counter_readback: Mutex<CounterReadbackState>,
}

struct VoxelComputeOutputBuffers {
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    layout: VoxelComputeOutputLayout,
    direct_slot: Option<u32>,
    direct_index_count: u32,
    direct_bounds_min: [f32; 4],
    direct_bounds_max: [f32; 4],
}

#[derive(Default)]
struct VoxelComputeOcclusionTexture {
    texture: Option<Texture>,
    view: Option<TextureView>,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct VoxelComputeDirectArena {
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    params_buffer: Option<Buffer>,
    indirect_buffer: Option<Buffer>,
    cull_metadata_buffer: Option<Buffer>,
    cull_output_indirect_buffer: Option<Buffer>,
    cull_config_buffer: Option<Buffer>,
    cull_count_buffer: Option<Buffer>,
    occlusion_texture: Mutex<VoxelComputeOcclusionTexture>,
    terrain_bind_group: Option<BindGroup>,
    slot_capacity: usize,
    mesh_slots: HashMap<AssetId<Mesh>, u32>,
    commands: Vec<VoxelComputeDirectDrawCommand>,
    cull_metadata: Vec<VoxelComputeDirectCullMetadata>,
    command_count: usize,
    draw_mode: VoxelComputeDirectDrawMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum VoxelComputeDirectDrawMode {
    #[default]
    Direct,
    Indirect,
    MultiIndirect,
}

impl VoxelComputeDirectDrawMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Indirect => "indirect",
            Self::MultiIndirect => "multi-indirect",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoxelComputeOutputLayout {
    vertex_capacity: u32,
    index_capacity: u32,
    vertex_word_capacity: u32,
    vertex_bytes: u64,
    index_bytes: u64,
}

impl VoxelComputeOutputLayout {
    fn new(vertex_capacity: u32, index_capacity: u32) -> Self {
        let vertex_word_capacity = vertex_capacity.saturating_mul(VERTEX_WORDS_PER_VERTEX);
        Self {
            vertex_capacity,
            index_capacity,
            vertex_word_capacity,
            vertex_bytes: u64::from(vertex_word_capacity) * COMPUTE_WORD_SIZE_BYTES,
            index_bytes: u64::from(index_capacity) * COMPUTE_WORD_SIZE_BYTES,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct VoxelComputeDirectArenaFrameStats {
    reallocated: bool,
    bytes_allocated: u64,
}

impl VoxelComputeDirectArena {
    fn retain_known_meshes(&mut self, current_meshes: &HashSet<AssetId<Mesh>>) -> usize {
        let before = self.mesh_slots.len();
        self.mesh_slots
            .retain(|mesh_id, _| current_meshes.contains(mesh_id));
        before - self.mesh_slots.len()
    }

    fn ensure_capacity(
        &mut self,
        render_device: &RenderDevice,
        required_slots: usize,
        layout: VoxelComputeOutputLayout,
    ) -> VoxelComputeDirectArenaFrameStats {
        let required_slots = required_slots.max(1);
        if self.slot_capacity >= required_slots
            && self.vertex_buffer.is_some()
            && self.index_buffer.is_some()
            && self.params_buffer.is_some()
            && self.indirect_buffer.is_some()
            && self.cull_metadata_buffer.is_some()
            && self.cull_output_indirect_buffer.is_some()
            && self.cull_config_buffer.is_some()
            && self.cull_count_buffer.is_some()
        {
            return VoxelComputeDirectArenaFrameStats::default();
        }

        let slot_capacity = required_slots.next_power_of_two();
        let vertex_bytes = layout.vertex_bytes * slot_capacity as u64;
        let index_bytes = layout.index_bytes * slot_capacity as u64;
        let params_bytes =
            std::mem::size_of::<VoxelComputeDirectChunkParams>() as u64 * slot_capacity as u64;
        let indirect_bytes = COMPUTE_DIRECT_DRAW_COMMAND_BYTES * slot_capacity as u64;
        let cull_metadata_bytes =
            std::mem::size_of::<VoxelComputeDirectCullMetadata>() as u64 * slot_capacity as u64;
        let cull_config_bytes = std::mem::size_of::<VoxelComputeDirectCullConfig>() as u64;

        self.vertex_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_vertex_arena_buffer"),
            size: vertex_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.index_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_index_arena_buffer"),
            size: index_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.params_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_params_arena_buffer"),
            size: params_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.indirect_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_indirect_draw_buffer"),
            size: indirect_bytes,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.cull_metadata_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_cull_metadata_buffer"),
            size: cull_metadata_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.cull_output_indirect_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_culled_indirect_draw_buffer"),
            size: indirect_bytes,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));
        self.cull_config_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_cull_config_buffer"),
            size: cull_config_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.cull_count_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("voxel_compute_direct_cull_count_buffer"),
            size: std::mem::size_of::<u32>() as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.terrain_bind_group = None;
        self.slot_capacity = slot_capacity;
        self.commands.clear();
        self.cull_metadata.clear();
        self.command_count = 0;

        VoxelComputeDirectArenaFrameStats {
            reallocated: true,
            bytes_allocated: vertex_bytes
                + index_bytes
                + params_bytes
                + indirect_bytes * 2
                + cull_metadata_bytes
                + cull_config_bytes
                + std::mem::size_of::<u32>() as u64,
        }
    }

    fn prepare_bind_group(
        &mut self,
        render_device: &RenderDevice,
        pipeline: &VoxelComputeDirectPipeline,
        gpu_atlas: &GpuImage,
    ) {
        if self.terrain_bind_group.is_some() {
            return;
        }

        let (Some(vertex_buffer), Some(index_buffer), Some(params_buffer)) = (
            self.vertex_buffer.as_ref(),
            self.index_buffer.as_ref(),
            self.params_buffer.as_ref(),
        ) else {
            return;
        };

        self.terrain_bind_group = Some(render_device.create_bind_group(
            Some("voxel_compute_direct_arena_terrain_bind_group"),
            &pipeline.terrain_bind_group_layout,
            &BindGroupEntries::sequential((
                vertex_buffer.as_entire_buffer_binding(),
                index_buffer.as_entire_buffer_binding(),
                params_buffer.as_entire_buffer_binding(),
                BindingResource::TextureView(&gpu_atlas.texture_view),
                BindingResource::Sampler(&gpu_atlas.sampler),
            )),
        ));
    }

    fn assign_slot(&mut self, mesh_id: AssetId<Mesh>) -> Option<u32> {
        if let Some(slot) = self.mesh_slots.get(&mesh_id) {
            return Some(*slot);
        }

        let used_slots = self.mesh_slots.values().copied().collect::<HashSet<_>>();
        for slot in 0..self.slot_capacity {
            let slot = slot.min(u32::MAX as usize) as u32;
            if !used_slots.contains(&slot) {
                self.mesh_slots.insert(mesh_id, slot);
                return Some(slot);
            }
        }

        None
    }

    fn write_chunk_params(
        &self,
        render_queue: &RenderQueue,
        slot: u32,
        params: VoxelComputeDirectChunkParams,
    ) {
        let Some(params_buffer) = self.params_buffer.as_ref() else {
            return;
        };
        render_queue.write_buffer(
            params_buffer,
            u64::from(slot) * std::mem::size_of::<VoxelComputeDirectChunkParams>() as u64,
            bytemuck::bytes_of(&params),
        );
    }

    fn refresh_draw_commands(
        &mut self,
        render_queue: &RenderQueue,
        output_buffers: &HashMap<AssetId<Mesh>, VoxelComputeOutputBuffers>,
    ) {
        let Some(indirect_buffer) = self.indirect_buffer.as_ref() else {
            self.commands.clear();
            self.cull_metadata.clear();
            self.command_count = 0;
            return;
        };

        let mut commands = output_buffers
            .values()
            .filter_map(|output| {
                let slot = output.direct_slot?;
                (output.direct_index_count > 0).then_some((
                    slot,
                    output.direct_index_count,
                    output.direct_bounds_min,
                    output.direct_bounds_max,
                ))
            })
            .collect::<Vec<_>>();
        commands.sort_by_key(|(slot, ..)| *slot);

        let (draw_commands, cull_metadata): (Vec<_>, Vec<_>) = commands
            .into_iter()
            .map(|(slot, index_count, bounds_min, bounds_max)| {
                (
                    VoxelComputeDirectDrawCommand {
                        vertex_count: index_count,
                        instance_count: 1,
                        first_vertex: 0,
                        first_instance: slot,
                    },
                    VoxelComputeDirectCullMetadata {
                        bounds_min,
                        bounds_max,
                    },
                )
            })
            .unzip();
        self.command_count = draw_commands.len();
        self.commands = draw_commands;
        self.cull_metadata = cull_metadata;
        if self.commands.is_empty() {
            return;
        }

        render_queue.write_buffer(indirect_buffer, 0, bytemuck::cast_slice(&self.commands));
        if let Some(metadata_buffer) = self.cull_metadata_buffer.as_ref() {
            render_queue.write_buffer(
                metadata_buffer,
                0,
                bytemuck::cast_slice(&self.cull_metadata),
            );
        }
    }

    fn output_bindings(
        &self,
        slot: u32,
        layout: VoxelComputeOutputLayout,
    ) -> Option<(BindingResource<'_>, BindingResource<'_>)> {
        let (Some(vertex_buffer), Some(index_buffer)) =
            (self.vertex_buffer.as_ref(), self.index_buffer.as_ref())
        else {
            return None;
        };

        let vertex_offset = u64::from(slot) * layout.vertex_bytes;
        let index_offset = u64::from(slot) * layout.index_bytes;
        Some((
            BindingResource::Buffer(BufferBinding {
                buffer: vertex_buffer,
                offset: vertex_offset,
                size: NonZeroU64::new(layout.vertex_bytes),
            }),
            BindingResource::Buffer(BufferBinding {
                buffer: index_buffer,
                offset: index_offset,
                size: NonZeroU64::new(layout.index_bytes),
            }),
        ))
    }
}

#[derive(Clone, Copy, Default)]
struct ComputeOutputBufferFrameStats {
    created: usize,
    bytes_allocated: u64,
}

#[derive(Default)]
struct CounterReadbackState {
    copy_pending: bool,
    map_pending: bool,
    ready: Option<Arc<AtomicBool>>,
    success: Option<Arc<AtomicBool>>,
    mesh_id: Option<AssetId<Mesh>>,
    contract: Option<VoxelComputeMeshContract>,
    vertex_capacity: u32,
    index_capacity: u32,
}

impl VoxelComputeBuffers {
    fn retain_known_meshes(
        &mut self,
        current_meshes: &HashSet<AssetId<Mesh>>,
    ) -> VoxelComputeBufferEviction {
        let mut evicted_meshes = HashSet::default();
        retain_compute_asset_map_entries(
            &mut self.chunk_buffers,
            current_meshes,
            &mut evicted_meshes,
        );
        retain_compute_asset_map_entries(
            &mut self.boundary_buffers,
            current_meshes,
            &mut evicted_meshes,
        );
        retain_compute_asset_map_entries(
            &mut self.texture_palette_buffers,
            current_meshes,
            &mut evicted_meshes,
        );
        retain_compute_asset_map_entries(
            &mut self.output_buffers,
            current_meshes,
            &mut evicted_meshes,
        );
        if self.direct_arena.retain_known_meshes(current_meshes) > 0 {
            self.direct_arena.command_count = 0;
        }

        let cancelled_readbacks = self.cancel_stale_readback(current_meshes);
        VoxelComputeBufferEviction {
            buffer_meshes: evicted_meshes.len(),
            cancelled_readbacks,
        }
    }

    fn cancel_stale_readback(&mut self, current_meshes: &HashSet<AssetId<Mesh>>) -> usize {
        let Ok(mut state) = self.counter_readback.lock() else {
            warn!("voxel compute counter readback state is unavailable");
            return 0;
        };

        let Some(mesh_id) = state.mesh_id else {
            return 0;
        };
        if current_meshes.contains(&mesh_id) {
            return 0;
        }

        if state.map_pending {
            self.counter_readback_buffer.unmap();
        }
        *state = CounterReadbackState::default();
        1
    }

    fn readback_pending(&self) -> bool {
        self.counter_readback
            .lock()
            .is_ok_and(|state| state.copy_pending || state.map_pending)
    }

    fn request_counter_readback(
        &self,
        mesh_id: AssetId<Mesh>,
        contract: Option<VoxelComputeMeshContract>,
        vertex_capacity: u32,
        index_capacity: u32,
    ) {
        let Ok(mut state) = self.counter_readback.lock() else {
            warn!("voxel compute counter readback state is unavailable");
            return;
        };
        if state.copy_pending || state.map_pending {
            return;
        }

        state.copy_pending = true;
        state.mesh_id = Some(mesh_id);
        state.contract = contract;
        state.vertex_capacity = vertex_capacity;
        state.index_capacity = index_capacity;
    }
}

#[derive(Clone, Copy, Default)]
struct VoxelComputeBufferEviction {
    buffer_meshes: usize,
    cancelled_readbacks: usize,
}

fn retain_compute_asset_map_entries<T>(
    entries: &mut HashMap<AssetId<Mesh>, T>,
    current_meshes: &HashSet<AssetId<Mesh>>,
    evicted_meshes: &mut HashSet<AssetId<Mesh>>,
) {
    entries.retain(|mesh_id, _| {
        let keep = current_meshes.contains(mesh_id);
        if !keep {
            evicted_meshes.insert(*mesh_id);
        }
        keep
    });
}

impl FromWorld for VoxelComputeBuffers {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let atomic_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("atomic_counters_buffer"),
            size: 20,
            usage: BufferUsages::STORAGE
                | BufferUsages::INDIRECT
                | BufferUsages::COPY_DST
                | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let counter_readback_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("atomic_counters_readback_buffer"),
            size: COUNTER_BUFFER_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            chunk_buffers: HashMap::default(),
            boundary_buffers: HashMap::default(),
            texture_palette_buffers: HashMap::default(),
            output_buffers: HashMap::default(),
            direct_arena: VoxelComputeDirectArena::default(),
            atomic_buffer,
            counter_readback_buffer,
            counter_readback: Mutex::default(),
        }
    }
}

fn collect_compute_counter_readback(
    buffers: Res<VoxelComputeBuffers>,
    settings: Res<VoxelComputeSettings>,
    mut render_meshes: ResMut<RenderAssets<RenderMesh>>,
) {
    let Ok(mut state) = buffers.counter_readback.lock() else {
        warn!("voxel compute counter readback state is unavailable");
        return;
    };

    if state.copy_pending && !state.map_pending {
        let ready = Arc::new(AtomicBool::new(false));
        let success = Arc::new(AtomicBool::new(false));
        let ready_callback = Arc::clone(&ready);
        let success_callback = Arc::clone(&success);

        buffers
            .counter_readback_buffer
            .slice(..)
            .map_async(MapMode::Read, move |result| {
                success_callback.store(result.is_ok(), Ordering::Release);
                ready_callback.store(true, Ordering::Release);
            });

        state.copy_pending = false;
        state.map_pending = true;
        state.ready = Some(ready);
        state.success = Some(success);
        return;
    }

    let Some(ready) = &state.ready else {
        return;
    };
    if !state.map_pending || !ready.load(Ordering::Acquire) {
        return;
    }

    let success = state
        .success
        .as_ref()
        .is_some_and(|flag| flag.load(Ordering::Acquire));

    if success {
        let mapped = buffers.counter_readback_buffer.slice(..).get_mapped_range();
        if mapped.len() >= COUNTER_BUFFER_SIZE as usize {
            let mut index_bytes = [0; 4];
            index_bytes.copy_from_slice(&mapped[0..4]);
            let index_count = u32::from_le_bytes(index_bytes);
            let vertex_count = (index_count / 6) * 4;
            let vertex_overflow = vertex_count > state.vertex_capacity;
            let index_overflow = index_count > state.index_capacity;
            let visible_index_count = index_count.min(state.index_capacity);

            if !settings.direct_render
                && let Some(mesh_id) = state.mesh_id
            {
                update_render_mesh_draw_count(
                    &mut render_meshes,
                    mesh_id,
                    visible_index_count,
                    vertex_count.min(state.vertex_capacity),
                );
            }

            if let Some(contract) = &state.contract {
                record_compute_contract_result(contract, vertex_count, index_count);
            }

            if vertex_count == 0 || index_count == 0 || vertex_overflow || index_overflow {
                warn!(
                    ?state.mesh_id,
                    vertex_count,
                    index_count,
                    vertex_capacity = state.vertex_capacity,
                    index_capacity = state.index_capacity,
                    vertex_overflow,
                    index_overflow,
                    "voxel compute mesh counters need attention"
                );
            } else if settings.diagnostic_counter_readback {
                info!(
                    ?state.mesh_id,
                    vertex_count,
                    index_count,
                    vertex_capacity = state.vertex_capacity,
                    index_capacity = state.index_capacity,
                    "voxel compute mesh counters"
                );
            }
        }
        drop(mapped);
    } else {
        warn!(?state.mesh_id, "voxel compute counter readback failed");
    }

    buffers.counter_readback_buffer.unmap();
    *state = CounterReadbackState::default();
}

fn record_compute_contract_result(
    contract: &VoxelComputeMeshContract,
    vertex_count: u32,
    index_count: u32,
) {
    if vertex_count != contract.expected_vertices || index_count != contract.expected_indices {
        warn!(
            chunk_pos = ?contract.chunk_pos,
            y_base = contract.y_base,
            solid_blocks = contract.solid_blocks,
            expected_visible_faces = contract.expected_visible_faces,
            expected_vertices = contract.expected_vertices,
            actual_vertices = vertex_count,
            expected_indices = contract.expected_indices,
            actual_indices = index_count,
            "voxel compute parity contract mismatch"
        );
    } else {
        info!(
            chunk_pos = ?contract.chunk_pos,
            y_base = contract.y_base,
            solid_blocks = contract.solid_blocks,
            visible_faces = contract.expected_visible_faces,
            vertices = vertex_count,
            indices = index_count,
            "voxel compute parity contract matched"
        );
    }
}

fn update_render_mesh_draw_count(
    render_meshes: &mut RenderAssets<RenderMesh>,
    mesh_id: AssetId<Mesh>,
    index_count: u32,
    vertex_count: u32,
) -> bool {
    let Some(render_mesh) = render_meshes.get_mut(mesh_id) else {
        warn!(
            ?mesh_id,
            "voxel compute mesh is not prepared for draw count update"
        );
        return false;
    };

    render_mesh.vertex_count = vertex_count;
    let aligned_index_count = index_count - index_count % 3;
    match &mut render_mesh.buffer_info {
        RenderMeshBufferInfo::Indexed { count, .. } => {
            *count = aligned_index_count;
        }
        RenderMeshBufferInfo::NonIndexed => {
            warn!(?mesh_id, "voxel compute mesh is not indexed");
            return false;
        }
    }
    true
}

fn copy_owned_output_to_render_mesh(
    render_context: &mut RenderContext,
    output_buffers: &VoxelComputeOutputBuffers,
    vertex_buffer_slice: &MeshBufferSlice,
    index_buffer_slice: &MeshBufferSlice,
    contract: Option<&VoxelComputeMeshContract>,
) {
    let (Some(vertex_buffer), Some(index_buffer)) = (
        output_buffers.vertex_buffer.as_ref(),
        output_buffers.index_buffer.as_ref(),
    ) else {
        warn!("voxel compute compatibility copy-back output buffers are unavailable");
        return;
    };
    let (vertex_copy_bytes, index_copy_bytes) =
        compute_owned_output_copy_bytes(output_buffers.layout, contract);

    if vertex_copy_bytes > 0 {
        render_context.command_encoder().copy_buffer_to_buffer(
            vertex_buffer,
            0,
            vertex_buffer_slice.buffer,
            vertex_byte_offset(vertex_buffer_slice.range.start),
            vertex_copy_bytes,
        );
    }

    if index_copy_bytes > 0 {
        render_context.command_encoder().copy_buffer_to_buffer(
            index_buffer,
            0,
            index_buffer_slice.buffer,
            index_byte_offset(index_buffer_slice.range.start),
            index_copy_bytes,
        );
    }
}

fn compute_owned_output_copy_bytes(
    layout: VoxelComputeOutputLayout,
    contract: Option<&VoxelComputeMeshContract>,
) -> (u64, u64) {
    let vertex_count = contract.map_or(layout.vertex_capacity, |contract| {
        contract.expected_vertices.min(layout.vertex_capacity)
    });
    let index_count = contract.map_or(layout.index_capacity, |contract| {
        contract.expected_indices.min(layout.index_capacity)
    });

    (
        vertex_byte_count(vertex_count),
        u64::from(index_count) * COMPUTE_WORD_SIZE_BYTES,
    )
}

fn vertex_byte_offset(vertex_start: u32) -> u64 {
    vertex_byte_count(vertex_start)
}

fn vertex_byte_count(vertex_count: u32) -> u64 {
    u64::from(vertex_count) * u64::from(VERTEX_WORDS_PER_VERTEX) * COMPUTE_WORD_SIZE_BYTES
}

fn index_byte_offset(index_start: u32) -> u64 {
    u64::from(index_start) * COMPUTE_WORD_SIZE_BYTES
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct VoxelComputeDirectRenderSnapshot {
    views: usize,
    chunks_drawn: usize,
    draw_calls: usize,
    indices_drawn: u64,
    arena_slots: usize,
    indirect_commands: usize,
    cull_enabled: bool,
    cull_count_supported: bool,
    cull_compact_enabled: bool,
    cull_candidate_commands: usize,
    cull_visible_commands: usize,
    cull_culled_commands: usize,
    draw_mode: VoxelComputeDirectDrawMode,
    skipped_without_bind_group: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct VoxelComputeDirectCullEstimate {
    visible_commands: usize,
    visible_indices: u64,
}

#[derive(Resource, Default)]
struct VoxelComputeDirectRenderTelemetry {
    last_logged: Mutex<Option<VoxelComputeDirectRenderSnapshot>>,
}

impl VoxelComputeDirectRenderTelemetry {
    fn log_summary(&self, snapshot: VoxelComputeDirectRenderSnapshot) {
        let Ok(mut last_logged) = self.last_logged.lock() else {
            warn!("voxel compute direct render telemetry is unavailable");
            return;
        };
        if *last_logged == Some(snapshot) {
            return;
        }
        *last_logged = Some(snapshot);
        info!(
            views = snapshot.views,
            chunks_drawn = snapshot.chunks_drawn,
            draw_calls = snapshot.draw_calls,
            indices_drawn = snapshot.indices_drawn,
            arena_slots = snapshot.arena_slots,
            indirect_commands = snapshot.indirect_commands,
            cull_enabled = snapshot.cull_enabled,
            cull_count_supported = snapshot.cull_count_supported,
            cull_compact_enabled = snapshot.cull_compact_enabled,
            cull_candidate_commands = snapshot.cull_candidate_commands,
            cull_visible_commands = snapshot.cull_visible_commands,
            cull_culled_commands = snapshot.cull_culled_commands,
            draw_mode = snapshot.draw_mode.as_str(),
            skipped_without_bind_group = snapshot.skipped_without_bind_group,
            "voxel compute direct render summary"
        );
    }
}

fn compute_direct_clip_from_world(extracted_view: &ExtractedView) -> Mat4 {
    extracted_view.clip_from_world.unwrap_or_else(|| {
        extracted_view.clip_from_view * extracted_view.world_from_view.affine().inverse()
    })
}

fn compute_direct_cull_estimate(
    metadata: &[VoxelComputeDirectCullMetadata],
    commands: &[VoxelComputeDirectDrawCommand],
    command_count: usize,
    clip_from_world: Mat4,
) -> VoxelComputeDirectCullEstimate {
    let mut estimate = VoxelComputeDirectCullEstimate::default();
    for (metadata, command) in metadata.iter().zip(commands.iter()).take(command_count) {
        if command.vertex_count == 0 {
            continue;
        }
        if !compute_direct_bounds_are_visible(
            Vec3::from_array([
                metadata.bounds_min[0],
                metadata.bounds_min[1],
                metadata.bounds_min[2],
            ]),
            Vec3::from_array([
                metadata.bounds_max[0],
                metadata.bounds_max[1],
                metadata.bounds_max[2],
            ]),
            clip_from_world,
        ) {
            continue;
        }
        estimate.visible_commands += 1;
        estimate.visible_indices += u64::from(command.vertex_count);
    }
    estimate
}

fn compute_direct_bounds_are_visible(
    bounds_min: Vec3,
    bounds_max: Vec3,
    clip_from_world: Mat4,
) -> bool {
    let corners = [
        Vec3::new(bounds_min.x, bounds_min.y, bounds_min.z),
        Vec3::new(bounds_max.x, bounds_min.y, bounds_min.z),
        Vec3::new(bounds_min.x, bounds_max.y, bounds_min.z),
        Vec3::new(bounds_max.x, bounds_max.y, bounds_min.z),
        Vec3::new(bounds_min.x, bounds_min.y, bounds_max.z),
        Vec3::new(bounds_max.x, bounds_min.y, bounds_max.z),
        Vec3::new(bounds_min.x, bounds_max.y, bounds_max.z),
        Vec3::new(bounds_max.x, bounds_max.y, bounds_max.z),
    ];

    let mut outside_left = true;
    let mut outside_right = true;
    let mut outside_bottom = true;
    let mut outside_top = true;
    let mut outside_near = true;
    let mut outside_far = true;

    for corner in corners {
        let clip = clip_from_world * corner.extend(1.0);
        outside_left &= clip.x < -clip.w;
        outside_right &= clip.x > clip.w;
        outside_bottom &= clip.y < -clip.w;
        outside_top &= clip.y > clip.w;
        outside_near &= clip.z < 0.0;
        outside_far &= clip.z > clip.w;
    }

    !(outside_left || outside_right || outside_bottom || outside_top || outside_near || outside_far)
}

pub struct VoxelComputeNode {
    state: VoxelComputeState,
}

enum VoxelComputeState {
    Loading,
    Ready,
}

impl Default for VoxelComputeNode {
    fn default() -> Self {
        Self {
            state: VoxelComputeState::Loading,
        }
    }
}

impl render_graph::Node for VoxelComputeNode {
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<VoxelComputePipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            VoxelComputeState::Loading => {
                if let CachedPipelineState::Ok(_) =
                    pipeline_cache.get_compute_pipeline_state(pipeline.pipeline)
                {
                    self.state = VoxelComputeState::Ready;
                }
            }
            VoxelComputeState::Ready => {}
        }
    }

    fn run(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        if matches!(self.state, VoxelComputeState::Loading) {
            return Ok(());
        }

        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<VoxelComputePipeline>();
        let mesh_allocator = world.resource::<MeshAllocator>();
        let render_queue = world.resource::<RenderQueue>();
        let chunks = world.resource::<ChunksToProcess>();
        let settings = world.resource::<VoxelComputeSettings>();
        let lifecycle = world.resource::<VoxelComputeLifecycle>();
        let Some(buffers) = world.get_resource::<VoxelComputeBuffers>() else {
            return Ok(());
        };

        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) else {
            return Ok(());
        };

        if settings.diagnostic_counter_readback && buffers.readback_pending() {
            return Ok(());
        }

        let mut submitted_this_frame = 0;
        for chunk in &chunks.0 {
            let mesh_id = chunk.mesh_id;
            let Some(chunk_buffer) = buffers.chunk_buffers.get(&mesh_id) else {
                warn!(?mesh_id, "voxel compute chunk input buffer is not prepared");
                continue;
            };
            let Some(boundary_buffer) = buffers.boundary_buffers.get(&mesh_id) else {
                warn!(
                    ?mesh_id,
                    "voxel compute boundary input buffer is not prepared"
                );
                continue;
            };
            let Some(texture_palette_buffer) = buffers.texture_palette_buffers.get(&mesh_id) else {
                warn!(
                    ?mesh_id,
                    "voxel compute texture palette buffer is not prepared"
                );
                continue;
            };
            let Some(output_buffers) = buffers.output_buffers.get(&mesh_id) else {
                warn!(?mesh_id, "voxel compute output buffers are not prepared");
                continue;
            };
            let mesh_slices = if settings.direct_render {
                None
            } else {
                let Some(vertex_buffer_slice) = mesh_allocator.mesh_vertex_slice(&mesh_id) else {
                    warn!(?mesh_id, "voxel compute mesh has no allocated vertex slice");
                    continue;
                };
                let Some(index_buffer_slice) = mesh_allocator.mesh_index_slice(&mesh_id) else {
                    warn!(?mesh_id, "voxel compute mesh has no allocated index slice");
                    continue;
                };
                Some((vertex_buffer_slice, index_buffer_slice))
            };

            let vertex_capacity = output_buffers.layout.vertex_capacity;
            let index_capacity = output_buffers.layout.index_capacity;
            let Some((vertex_output_binding, index_output_binding)) =
                compute_output_bindings(settings, buffers, output_buffers)
            else {
                warn!(
                    ?mesh_id,
                    "voxel compute output buffer binding is not prepared"
                );
                continue;
            };

            let ranges = DataRanges {
                vertex_start: 0,
                vertex_end: output_buffers.layout.vertex_word_capacity,
                index_start: 0,
                index_end: output_buffers.layout.index_capacity,
                neighbor_mask: chunk
                    .contract
                    .as_ref()
                    .map_or(0, |contract| contract.neighbor_mask),
            };

            let mut uniforms = UniformBuffer::from(ranges);
            uniforms.write_buffer(render_context.render_device(), render_queue);

            render_context
                .command_encoder()
                .clear_buffer(&buffers.atomic_buffer, 0, None);

            let bind_group = render_context.render_device().create_bind_group(
                Some("voxel_compute_bind_group"),
                &pipeline.bind_group_layout,
                &BindGroupEntries::sequential((
                    chunk_buffer.as_entire_buffer_binding(),
                    &uniforms,
                    vertex_output_binding,
                    index_output_binding,
                    buffers.atomic_buffer.as_entire_buffer_binding(),
                    texture_palette_buffer.as_entire_buffer_binding(),
                    boundary_buffer.as_entire_buffer_binding(),
                )),
            );

            {
                let mut pass =
                    render_context
                        .command_encoder()
                        .begin_compute_pass(&ComputePassDescriptor {
                            label: Some("voxel_compute_pass"),
                            timestamp_writes: None,
                        });

                pass.set_pipeline(compute_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(
                    COMPUTE_WORKGROUP_DISPATCH,
                    COMPUTE_WORKGROUP_DISPATCH,
                    COMPUTE_WORKGROUP_DISPATCH,
                );
            }

            if settings.diagnostic_counter_readback && !buffers.readback_pending() {
                render_context.command_encoder().copy_buffer_to_buffer(
                    &buffers.atomic_buffer,
                    0,
                    &buffers.counter_readback_buffer,
                    0,
                    COUNTER_BUFFER_SIZE,
                );

                buffers.request_counter_readback(
                    mesh_id,
                    chunk.contract.clone(),
                    vertex_capacity,
                    index_capacity,
                );
            }

            if let Some((vertex_buffer_slice, index_buffer_slice)) = mesh_slices {
                copy_owned_output_to_render_mesh(
                    render_context,
                    output_buffers,
                    &vertex_buffer_slice,
                    &index_buffer_slice,
                    chunk.contract.as_ref(),
                );
            }

            lifecycle.mark_loaded(mesh_id, chunk.source_generation);
            submitted_this_frame += 1;
        }
        lifecycle.log_summary(ComputeLifecycleFrameStats {
            submitted: submitted_this_frame,
            ..default()
        });

        Ok(())
    }
}

pub struct VoxelComputeDirectRenderNode {
    state: VoxelComputeDirectRenderState,
}

enum VoxelComputeDirectRenderState {
    Loading,
    Ready,
}

impl Default for VoxelComputeDirectRenderNode {
    fn default() -> Self {
        Self {
            state: VoxelComputeDirectRenderState::Loading,
        }
    }
}

impl render_graph::Node for VoxelComputeDirectRenderNode {
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<VoxelComputeDirectPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            VoxelComputeDirectRenderState::Loading => {
                if let CachedPipelineState::Ok(_) =
                    pipeline_cache.get_render_pipeline_state(pipeline.pipeline)
                {
                    self.state = VoxelComputeDirectRenderState::Ready;
                }
            }
            VoxelComputeDirectRenderState::Ready => {}
        }
    }

    fn run(
        &self,
        graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        if matches!(self.state, VoxelComputeDirectRenderState::Loading) {
            return Ok(());
        }

        let settings = world.resource::<VoxelComputeSettings>();
        if !settings.direct_render {
            return Ok(());
        }

        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<VoxelComputeDirectPipeline>();
        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline) else {
            return Ok(());
        };

        let view_entity = graph.view_entity();
        let Some(view_target) = world.get::<ViewTarget>(view_entity) else {
            return Ok(());
        };
        let Some(depth_texture) = world.get::<ViewDepthTexture>(view_entity) else {
            return Ok(());
        };
        let Some(view_uniform) = world.get::<VoxelComputeDirectViewUniform>(view_entity) else {
            return Ok(());
        };
        let Some(extracted_view) = world.get::<ExtractedView>(view_entity) else {
            return Ok(());
        };
        let Some(buffers) = world.get_resource::<VoxelComputeBuffers>() else {
            return Ok(());
        };
        let telemetry = world.resource::<VoxelComputeDirectRenderTelemetry>();

        let mut snapshot = VoxelComputeDirectRenderSnapshot {
            views: 1,
            arena_slots: buffers.direct_arena.slot_capacity,
            indirect_commands: buffers.direct_arena.command_count,
            draw_mode: buffers.direct_arena.draw_mode,
            ..default()
        };
        let mut drawable_outputs = buffers
            .output_buffers
            .values()
            .filter_map(|output| {
                let slot = output.direct_slot?;
                (output.direct_index_count > 0).then_some((slot, output.direct_index_count))
            })
            .collect::<Vec<_>>();
        drawable_outputs.sort_by_key(|(slot, _)| *slot);
        if drawable_outputs.is_empty() {
            telemetry.log_summary(snapshot);
            return Ok(());
        }
        let Some(terrain_bind_group) = buffers.direct_arena.terrain_bind_group.as_ref() else {
            snapshot.skipped_without_bind_group = drawable_outputs.len();
            telemetry.log_summary(snapshot);
            return Ok(());
        };
        let has_first_instance = render_context
            .render_device()
            .features()
            .contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE);
        let has_indirect_count = render_context
            .render_device()
            .features()
            .contains(WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT);
        let draw_mode = direct_draw_mode(
            settings,
            has_first_instance,
            buffers.direct_arena.command_count,
        );
        snapshot.draw_mode = draw_mode;
        snapshot.cull_count_supported = has_indirect_count;
        let cull_compact_enabled =
            compute_direct_gpu_cull_compact_enabled(settings, draw_mode, has_indirect_count);
        let all_indices_drawn = drawable_outputs
            .iter()
            .map(|(_, index_count)| u64::from(*index_count))
            .sum::<u64>();
        let clip_from_world = compute_direct_clip_from_world(extracted_view);
        let cull_estimate = compute_direct_cull_estimate(
            &buffers.direct_arena.cull_metadata,
            &buffers.direct_arena.commands,
            buffers.direct_arena.command_count,
            clip_from_world,
        );
        let mut indirect_buffer_override = None;
        let mut indirect_count_buffer_override = None;
        if settings.direct_gpu_cull
            && !matches!(draw_mode, VoxelComputeDirectDrawMode::Direct)
            && buffers.direct_arena.command_count > 0
            && let Some(cull_pipeline) = world.get_resource::<VoxelComputeDirectCullPipeline>()
            && let Some(compute_pipeline) =
                pipeline_cache.get_compute_pipeline(cull_pipeline.pipeline)
            && let (
                Some(source_indirect_buffer),
                Some(metadata_buffer),
                Some(output_indirect_buffer),
                Some(config_buffer),
                Some(count_buffer),
            ) = (
                buffers.direct_arena.indirect_buffer.as_ref(),
                buffers.direct_arena.cull_metadata_buffer.as_ref(),
                buffers.direct_arena.cull_output_indirect_buffer.as_ref(),
                buffers.direct_arena.cull_config_buffer.as_ref(),
                buffers.direct_arena.cull_count_buffer.as_ref(),
            )
        {
            let render_queue = world.resource::<RenderQueue>();
            let mut occlusion_guard = buffers.direct_arena.occlusion_texture.lock().unwrap();
            if occlusion_guard.view.is_none() {
                let render_device = render_context.render_device();
                let texture = render_device.create_texture(&TextureDescriptor {
                    label: Some("voxel_compute_dummy_occlusion_texture"),
                    size: Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Depth32Float,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&TextureViewDescriptor::default());
                occlusion_guard.texture = Some(texture);
                occlusion_guard.view = Some(view);
                occlusion_guard.width = 1;
                occlusion_guard.height = 1;
            }

            let occlusion_enabled = settings.direct_gpu_occlusion_cull
                && occlusion_guard.width > 1
                && occlusion_guard.height > 1;

            let cull_config = VoxelComputeDirectCullConfig {
                clip_from_world_columns: clip_from_world.to_cols_array_2d(),
                draw: [
                    buffers.direct_arena.command_count.min(u32::MAX as usize) as u32,
                    u32::from(cull_compact_enabled),
                    u32::from(occlusion_enabled),
                    0,
                ],
            };
            render_queue.write_buffer(config_buffer, 0, bytemuck::bytes_of(&cull_config));
            if cull_compact_enabled {
                render_queue.write_buffer(count_buffer, 0, bytemuck::bytes_of(&0_u32));
            }

            let depth_view = occlusion_guard.view.as_ref().unwrap();
            let cull_bind_group = render_context.render_device().create_bind_group(
                Some("voxel_compute_direct_cull_bind_group"),
                &cull_pipeline.bind_group_layout,
                &BindGroupEntries::sequential((
                    source_indirect_buffer.as_entire_buffer_binding(),
                    metadata_buffer.as_entire_buffer_binding(),
                    output_indirect_buffer.as_entire_buffer_binding(),
                    config_buffer.as_entire_buffer_binding(),
                    count_buffer.as_entire_buffer_binding(),
                    BindingResource::TextureView(depth_view),
                )),
            );
            let mut cull_pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("voxel_compute_direct_cull_pass"),
                        timestamp_writes: None,
                    });
            cull_pass.set_pipeline(compute_pipeline);
            cull_pass.set_bind_group(0, &cull_bind_group, &[]);
            let workgroups = buffers
                .direct_arena
                .command_count
                .div_ceil(COMPUTE_DIRECT_CULL_WORKGROUP_SIZE)
                .min(u32::MAX as usize) as u32;
            cull_pass.dispatch_workgroups(workgroups, 1, 1);
            drop(cull_pass);
            indirect_buffer_override = Some(output_indirect_buffer);
            if cull_compact_enabled {
                indirect_count_buffer_override = Some(count_buffer);
            }
            snapshot.cull_enabled = true;
            snapshot.cull_compact_enabled = cull_compact_enabled;
            snapshot.cull_candidate_commands = buffers.direct_arena.command_count;
            snapshot.cull_visible_commands = cull_estimate.visible_commands;
            snapshot.cull_culled_commands = buffers
                .direct_arena
                .command_count
                .saturating_sub(cull_estimate.visible_commands);
        }
        let visible_command_count = if snapshot.cull_enabled {
            cull_estimate.visible_commands
        } else {
            buffers.direct_arena.command_count
        };
        let visible_indices_drawn = if snapshot.cull_enabled {
            cull_estimate.visible_indices
        } else {
            all_indices_drawn
        };

        let mut render_pass =
            render_context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("voxel_compute_direct_render_pass"),
                    color_attachments: &[Some(view_target.get_color_attachment())],
                    depth_stencil_attachment: Some(depth_texture.get_attachment(StoreOp::Store)),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

        render_pass.set_pipeline(render_pipeline);
        render_pass.set_bind_group(0, &view_uniform.bind_group, &[]);
        render_pass.set_bind_group(1, terrain_bind_group, &[]);

        match draw_mode {
            VoxelComputeDirectDrawMode::MultiIndirect => {
                if let Some(indirect_buffer) =
                    indirect_buffer_override.or(buffers.direct_arena.indirect_buffer.as_ref())
                {
                    if let Some(count_buffer) = indirect_count_buffer_override {
                        render_pass.multi_draw_indirect_count(
                            indirect_buffer,
                            0,
                            count_buffer,
                            0,
                            buffers.direct_arena.command_count.min(u32::MAX as usize) as u32,
                        );
                    } else {
                        render_pass.multi_draw_indirect(
                            indirect_buffer,
                            0,
                            buffers.direct_arena.command_count.min(u32::MAX as usize) as u32,
                        );
                    }
                    snapshot.chunks_drawn = visible_command_count;
                    snapshot.draw_calls = usize::from(buffers.direct_arena.command_count > 0);
                    snapshot.indices_drawn = visible_indices_drawn;
                }
            }
            VoxelComputeDirectDrawMode::Indirect => {
                if let Some(indirect_buffer) =
                    indirect_buffer_override.or(buffers.direct_arena.indirect_buffer.as_ref())
                {
                    for command_index in 0..buffers.direct_arena.command_count {
                        render_pass.draw_indirect(
                            indirect_buffer,
                            command_index as u64 * COMPUTE_DIRECT_DRAW_COMMAND_BYTES,
                        );
                    }
                    snapshot.chunks_drawn = visible_command_count;
                    snapshot.draw_calls = buffers.direct_arena.command_count;
                    snapshot.indices_drawn = visible_indices_drawn;
                }
            }
            VoxelComputeDirectDrawMode::Direct => {
                for (slot, index_count) in &drawable_outputs {
                    let slot = *slot;
                    let instance = slot..slot.saturating_add(1);
                    render_pass.draw(0..*index_count, instance);
                    snapshot.chunks_drawn += 1;
                    snapshot.draw_calls += 1;
                    snapshot.indices_drawn += u64::from(*index_count);
                }
            }
        }
        drop(render_pass);

        if settings.direct_gpu_cull {
            let source_texture_opt = world
                .get::<bevy::core_pipeline::prepass::ViewPrepassTextures>(view_entity)
                .and_then(|prepass| prepass.depth.as_ref())
                .map(|prepass_depth| &prepass_depth.texture.texture);

            if let Some(source_texture) = source_texture_opt {
                let mut occlusion_guard = buffers.direct_arena.occlusion_texture.lock().unwrap();
                let width = source_texture.width();
                let height = source_texture.height();

                if occlusion_guard.width != width
                    || occlusion_guard.height != height
                    || occlusion_guard.texture.is_none()
                {
                    let render_device = render_context.render_device();
                    let texture = render_device.create_texture(&TextureDescriptor {
                        label: Some("voxel_compute_occlusion_depth_texture"),
                        size: Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: TextureFormat::Depth32Float,
                        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&TextureViewDescriptor::default());

                    occlusion_guard.texture = Some(texture);
                    occlusion_guard.view = Some(view);
                    occlusion_guard.width = width;
                    occlusion_guard.height = height;
                }

                if let Some(ref dst_texture) = occlusion_guard.texture {
                    render_context.command_encoder().copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: source_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::DepthOnly,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: dst_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::DepthOnly,
                        },
                        Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
        }

        telemetry.log_summary(snapshot);
        Ok(())
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn env_flag_default(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn compute_direct_render_enabled_from_env() -> bool {
    env_flag_default(COMPUTE_DIRECT_RENDER_ENV, true)
}

fn direct_draw_mode(
    settings: &VoxelComputeSettings,
    has_first_instance: bool,
    command_count: usize,
) -> VoxelComputeDirectDrawMode {
    if command_count == 0 || !settings.direct_indirect || !has_first_instance {
        return VoxelComputeDirectDrawMode::Direct;
    }
    if settings.direct_multi_indirect {
        VoxelComputeDirectDrawMode::MultiIndirect
    } else {
        VoxelComputeDirectDrawMode::Indirect
    }
}

const fn compute_direct_gpu_cull_compact_enabled(
    settings: &VoxelComputeSettings,
    draw_mode: VoxelComputeDirectDrawMode,
    has_indirect_count: bool,
) -> bool {
    settings.direct_gpu_cull_compact
        && has_indirect_count
        && matches!(draw_mode, VoxelComputeDirectDrawMode::MultiIndirect)
}

fn gpu_compute_max_jobs_per_frame_from_env() -> usize {
    std::env::var(GPU_COMPUTE_MAX_JOBS_PER_FRAME_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_GPU_COMPUTE_MAX_JOBS_PER_FRAME)
        .max(1)
}

fn gpu_compute_queue_radius_from_env() -> i32 {
    std::env::var(GPU_COMPUTE_QUEUE_RADIUS_ENV)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_GPU_COMPUTE_QUEUE_RADIUS)
        .clamp(0, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_queue_radius_zero_targets_parity_chunk_only() {
        assert_eq!(
            compute_queue_chunk_positions_for_radius(0),
            vec![COMPUTE_PARITY_CHUNK_POS]
        );
    }

    #[test]
    fn compute_queue_radius_one_targets_center_and_four_neighbors() {
        let positions = compute_queue_chunk_positions_for_radius(1);

        assert_eq!(positions.len(), 5);
        assert_eq!(positions[0], COMPUTE_PARITY_CHUNK_POS);
        assert!(positions.contains(&ChunkPos { x: -1, z: 0 }));
        assert!(positions.contains(&ChunkPos { x: 1, z: 0 }));
        assert!(positions.contains(&ChunkPos { x: 0, z: -1 }));
        assert!(positions.contains(&ChunkPos { x: 0, z: 1 }));
    }

    #[test]
    fn compute_neighbor_mask_detects_cardinal_neighbors() {
        let center = ComputeChunkKey {
            chunk_pos: ChunkPos { x: 0, z: 0 },
            y_base: 0,
        };
        let positions = HashSet::from_iter([
            center,
            ComputeChunkKey {
                chunk_pos: ChunkPos { x: 1, z: 0 },
                y_base: 0,
            },
            ComputeChunkKey {
                chunk_pos: ChunkPos { x: -1, z: 0 },
                y_base: 0,
            },
            ComputeChunkKey {
                chunk_pos: ChunkPos { x: 0, z: 1 },
                y_base: 0,
            },
            ComputeChunkKey {
                chunk_pos: ChunkPos { x: 0, z: 0 },
                y_base: COMPUTE_VERTICAL_LAYER_HEIGHT,
            },
        ]);

        assert_eq!(
            compute_neighbor_mask(center, &positions),
            COMPUTE_NEIGHBOR_PLUS_X
                | COMPUTE_NEIGHBOR_MINUS_X
                | COMPUTE_NEIGHBOR_PLUS_Z
                | COMPUTE_NEIGHBOR_PLUS_Y
        );
    }

    #[test]
    fn compute_lifecycle_requeues_dirty_source_generation() {
        let lifecycle = VoxelComputeLifecycle::default();
        let mesh_id = AssetId::<Mesh>::invalid();

        let initial = lifecycle.state_or_insert_pending(mesh_id, 0);
        assert_eq!(initial.state, ComputeLifecycleState::Pending);
        assert!(!initial.invalidated);

        lifecycle.mark_building(mesh_id, 0);
        lifecycle.mark_loaded(mesh_id, 0);
        let loaded = lifecycle.state_or_insert_pending(mesh_id, 0);
        assert_eq!(loaded.state, ComputeLifecycleState::Loaded);
        assert!(!loaded.invalidated);

        let dirty = lifecycle.state_or_insert_pending(mesh_id, 1);
        assert_eq!(dirty.state, ComputeLifecycleState::Pending);
        assert!(dirty.invalidated);
    }

    #[test]
    fn compute_lifecycle_ignores_stale_loaded_generation() {
        let lifecycle = VoxelComputeLifecycle::default();
        let mesh_id = AssetId::<Mesh>::invalid();

        let _ = lifecycle.state_or_insert_pending(mesh_id, 0);
        lifecycle.mark_building(mesh_id, 0);
        let dirty = lifecycle.state_or_insert_pending(mesh_id, 1);
        assert_eq!(dirty.state, ComputeLifecycleState::Pending);

        lifecycle.mark_loaded(mesh_id, 0);
        let after_stale_completion = lifecycle.state_or_insert_pending(mesh_id, 1);
        assert_eq!(after_stale_completion.state, ComputeLifecycleState::Pending);

        lifecycle.mark_building(mesh_id, 1);
        lifecycle.mark_loaded(mesh_id, 1);
        let after_current_completion = lifecycle.state_or_insert_pending(mesh_id, 1);
        assert_eq!(
            after_current_completion.state,
            ComputeLifecycleState::Loaded
        );
    }

    #[test]
    fn compute_lifecycle_evicts_vanished_mesh_entries() {
        let lifecycle = VoxelComputeLifecycle::default();
        let kept_mesh = AssetId::<Mesh>::default();
        let vanished_mesh = AssetId::<Mesh>::invalid();

        let _ = lifecycle.state_or_insert_pending(kept_mesh, 0);
        let _ = lifecycle.state_or_insert_pending(vanished_mesh, 0);
        lifecycle.mark_loaded(kept_mesh, 0);
        lifecycle.mark_loaded(vanished_mesh, 0);

        let evicted = lifecycle.retain_known_meshes(&HashSet::from_iter([kept_mesh]));
        assert_eq!(evicted, 1);
        assert_eq!(
            lifecycle.state_or_insert_pending(kept_mesh, 0).state,
            ComputeLifecycleState::Loaded
        );

        let reintroduced = lifecycle.state_or_insert_pending(vanished_mesh, 0);
        assert_eq!(reintroduced.state, ComputeLifecycleState::Pending);
        assert!(!reintroduced.invalidated);
    }

    #[test]
    fn compute_buffer_retention_tracks_unique_evicted_meshes() {
        let kept_mesh = AssetId::<Mesh>::default();
        let vanished_mesh = AssetId::<Mesh>::invalid();
        let current_meshes = HashSet::from_iter([kept_mesh]);
        let mut first_map = HashMap::default();
        first_map.insert(kept_mesh, 1);
        first_map.insert(vanished_mesh, 2);
        let mut second_map = HashMap::default();
        second_map.insert(vanished_mesh, 3);
        let mut evicted_meshes = HashSet::default();

        retain_compute_asset_map_entries(&mut first_map, &current_meshes, &mut evicted_meshes);
        retain_compute_asset_map_entries(&mut second_map, &current_meshes, &mut evicted_meshes);

        assert!(first_map.contains_key(&kept_mesh));
        assert!(!first_map.contains_key(&vanished_mesh));
        assert!(second_map.is_empty());
        assert_eq!(evicted_meshes.len(), 1);
        assert!(evicted_meshes.contains(&vanished_mesh));
    }

    #[test]
    fn compute_source_dirty_generation_advances() {
        let mut source = VoxelComputeChunkSource::new(
            Box::new([0; CHUNK_BLOCK_COUNT]),
            Box::new([0; COMPUTE_BOUNDARY_BLOCK_WORDS]),
            Box::new([0; COMPUTE_TEXTURE_PALETTE_WORDS]),
        );

        assert_eq!(source.generation(), 0);
        source.mark_dirty();
        assert_eq!(source.generation(), 1);
    }

    #[test]
    fn compute_block_edit_updates_source_generation_and_contract() {
        let (mut source, mut contract) =
            test_compute_source_and_contract(ChunkPos { x: 0, z: 0 }, 0, 0);
        let edit =
            VoxelComputeBlockEdit::new(ChunkPos { x: 0, z: 0 }, LocalBlockPos::new(3, 4, 5), 3);

        assert!(apply_compute_block_edit_to_source(
            &edit,
            &mut source,
            &mut contract
        ));
        assert_eq!(source.generation(), 1);
        assert_eq!(source.blocks[compute_block_index(3, 4, 5)], 3);
        assert_eq!(contract.solid_blocks, 1);
        assert_eq!(contract.expected_visible_faces, 6);
        assert_eq!(contract.expected_vertices, 24);
        assert_eq!(contract.expected_indices, 36);

        assert!(!apply_compute_block_edit_to_source(
            &edit,
            &mut source,
            &mut contract
        ));
        assert_eq!(source.generation(), 1);
    }

    #[test]
    fn compute_block_edit_updates_neighbor_boundary_contract() {
        let (mut observer_source, mut observer_contract) =
            test_compute_source_and_contract(ChunkPos { x: 0, z: 0 }, 0, COMPUTE_NEIGHBOR_PLUS_X);
        observer_source.blocks[compute_block_index(CHUNK_SIZE - 1, 4, 5)] = 3;
        observer_contract =
            compute_mesh_contract_from_compute_source(&observer_source, &observer_contract);
        assert_eq!(observer_contract.expected_visible_faces, 6);

        let edit =
            VoxelComputeBlockEdit::new(ChunkPos { x: 1, z: 0 }, LocalBlockPos::new(0, 4, 5), 3);

        assert!(apply_compute_block_edit_to_source(
            &edit,
            &mut observer_source,
            &mut observer_contract
        ));
        assert_eq!(observer_source.generation(), 1);
        assert_eq!(
            observer_source.boundary_blocks[BoundaryFace::PlusX.buffer_offset(4, 5)],
            3
        );
        assert_eq!(observer_contract.expected_visible_faces, 5);
    }

    #[test]
    fn compute_block_edit_ignores_out_of_range_local_position() {
        let (mut source, mut contract) =
            test_compute_source_and_contract(ChunkPos { x: 0, z: 0 }, 0, 0);
        let edit =
            VoxelComputeBlockEdit::new(ChunkPos { x: 0, z: 0 }, LocalBlockPos::new(33, 0, 0), 3);

        assert!(!apply_compute_block_edit_to_source(
            &edit,
            &mut source,
            &mut contract
        ));
        assert_eq!(source.generation(), 0);
        assert_eq!(contract.solid_blocks, 0);
    }

    #[test]
    fn compute_block_edit_converts_from_world_block_edit() {
        let world_edit =
            WorldBlockEdit::new(ChunkPos { x: 2, z: -1 }, LocalBlockPos::new(7, 40, 8), 5);
        let compute_edit = VoxelComputeBlockEdit::from(world_edit);

        assert_eq!(compute_edit.chunk_pos, ChunkPos { x: 2, z: -1 });
        assert_eq!(compute_edit.local_pos, LocalBlockPos::new(7, 40, 8));
        assert_eq!(compute_edit.block, 5);
    }

    #[test]
    fn compute_bridge_writes_voxel_compute_block_edits_from_world_edits() {
        let mut app = App::new();
        app.add_message::<WorldBlockEdit>();
        app.add_message::<VoxelComputeBlockEdit>();
        app.add_systems(Update, bridge_world_block_edits_to_voxel_compute_edits);
        let world_edit =
            WorldBlockEdit::new(ChunkPos { x: -2, z: 3 }, LocalBlockPos::new(9, 10, 11), 4);

        app.world_mut()
            .resource_mut::<bevy::ecs::message::Messages<WorldBlockEdit>>()
            .write(world_edit);
        app.update();

        let compute_messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<VoxelComputeBlockEdit>>();
        let edits = compute_messages
            .iter_current_update_messages()
            .copied()
            .collect::<Vec<_>>();

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].chunk_pos, world_edit.chunk_pos);
        assert_eq!(edits[0].local_pos, world_edit.local_pos);
        assert_eq!(edits[0].block, world_edit.block);
    }

    #[test]
    fn compute_setup_copy_seeds_single_chunk_extract_from_chunk_data() {
        let mut source = ChunkData::default();
        source.set_block(1, 2, 3, 5);
        source.set_block(4, 5, 6, 7);
        let mut target = SingleChunkExtract {
            blocks: Box::new([99; CHUNK_VOLUME]),
            has_changes: true,
        };

        copy_chunk_data_to_single_chunk_extract(&source, &mut target);

        assert_eq!(target.blocks[ChunkData::get_index(1, 2, 3)], 5);
        assert_eq!(target.blocks[ChunkData::get_index(4, 5, 6)], 7);
        assert_eq!(target.blocks[ChunkData::get_index(0, 0, 0)], 0);
        assert!(!target.has_changes);
    }

    #[test]
    fn compute_generation_applies_world_edit_store_overlay() {
        let registry = BlockRegistry::default();
        let context = WorldGenerationContext::from_registry(&registry);
        let mut edit_store = WorldEditStore::default();
        let chunk_pos = ChunkPos { x: 0, z: 0 };
        assert!(edit_store.apply_edit(WorldBlockEdit::new(
            chunk_pos,
            LocalBlockPos::new(7, 40, 8),
            77,
        )));

        let chunk = generate_compute_layer_chunk(chunk_pos, 32, &context, &edit_store);

        assert_eq!(chunk.get_block(7, 8, 8), 77);
    }

    #[test]
    fn compute_boundary_generation_applies_neighbor_world_edit_overlay() {
        let registry = BlockRegistry::default();
        let context = WorldGenerationContext::from_registry(&registry);
        let observer = ComputeChunkKey {
            chunk_pos: ChunkPos { x: 0, z: 0 },
            y_base: 0,
        };
        let neighbor = ComputeChunkKey {
            chunk_pos: ChunkPos { x: 1, z: 0 },
            y_base: 0,
        };
        let positions = HashSet::from_iter([observer, neighbor]);
        let mut edit_store = WorldEditStore::default();
        assert!(edit_store.apply_edit(WorldBlockEdit::new(
            neighbor.chunk_pos,
            LocalBlockPos::new(0, 4, 5),
            77,
        )));

        let boundary_blocks =
            copy_boundary_blocks_to_compute_buffer(observer, &context, &edit_store, &positions);

        assert_eq!(boundary_blocks[BoundaryFace::PlusX.buffer_offset(4, 5)], 77);
    }

    #[test]
    fn compute_owned_output_layout_uses_shader_vertex_stride() {
        let layout = VoxelComputeOutputLayout::new(2, 3);

        assert_eq!(layout.vertex_capacity, 2);
        assert_eq!(layout.index_capacity, 3);
        assert_eq!(layout.vertex_word_capacity, 26);
        assert_eq!(layout.vertex_bytes, 104);
        assert_eq!(layout.index_bytes, 12);
        assert_eq!(vertex_byte_offset(2), 104);
        assert_eq!(index_byte_offset(3), 12);
    }

    #[test]
    fn compute_owned_output_copy_bytes_clamps_to_capacity() {
        let layout = VoxelComputeOutputLayout::new(10, 15);
        let small_contract = VoxelComputeMeshContract {
            chunk_pos: ChunkPos { x: 0, z: 0 },
            y_base: 0,
            neighbor_mask: 0,
            solid_blocks: 1,
            expected_visible_faces: 1,
            expected_vertices: 4,
            expected_indices: 6,
        };
        let large_contract = VoxelComputeMeshContract {
            expected_visible_faces: 99,
            expected_vertices: 999,
            expected_indices: 999,
            ..small_contract.clone()
        };

        assert_eq!(
            compute_owned_output_copy_bytes(layout, Some(&small_contract)),
            (208, 24)
        );
        assert_eq!(
            compute_owned_output_copy_bytes(layout, Some(&large_contract)),
            (520, 60)
        );
        assert_eq!(compute_owned_output_copy_bytes(layout, None), (520, 60));
    }

    #[test]
    fn compute_direct_chunk_offset_uses_world_chunk_translation() {
        let contract = VoxelComputeMeshContract {
            chunk_pos: ChunkPos { x: -2, z: 3 },
            y_base: 32,
            neighbor_mask: 0,
            solid_blocks: 0,
            expected_visible_faces: 0,
            expected_vertices: 0,
            expected_indices: 0,
        };

        assert_eq!(
            compute_direct_chunk_offset(&contract),
            [-64.0, 32.0, 96.0, 0.0]
        );
    }

    #[test]
    fn compute_direct_shader_params_keep_storage_buffer_abi() {
        assert_eq!(std::mem::size_of::<VoxelComputeDirectViewBuffer>(), 64);
        assert_eq!(std::mem::align_of::<VoxelComputeDirectViewBuffer>(), 4);
        assert_eq!(std::mem::size_of::<VoxelComputeDirectChunkParams>(), 48);
        assert_eq!(std::mem::align_of::<VoxelComputeDirectChunkParams>(), 4);
        assert_eq!(std::mem::size_of::<VoxelComputeDirectCullMetadata>(), 32);
        assert_eq!(std::mem::align_of::<VoxelComputeDirectCullMetadata>(), 4);
        assert_eq!(std::mem::size_of::<VoxelComputeDirectCullConfig>(), 80);
        assert_eq!(std::mem::align_of::<VoxelComputeDirectCullConfig>(), 4);
        assert_eq!(std::mem::size_of::<VoxelComputeDirectDrawCommand>(), 16);
        assert_eq!(std::mem::align_of::<VoxelComputeDirectDrawCommand>(), 4);
    }

    #[test]
    fn compute_direct_arena_offsets_use_fixed_slot_stride() {
        let layout = VoxelComputeOutputLayout::new(10, 15);

        assert_eq!(compute_direct_vertex_word_offset(0, layout), 0);
        assert_eq!(compute_direct_index_offset(0, layout), 0);
        assert_eq!(compute_direct_vertex_word_offset(3, layout), 390);
        assert_eq!(compute_direct_index_offset(3, layout), 45);
    }

    #[test]
    fn compute_direct_cull_estimate_filters_outside_clip_bounds() {
        let metadata = vec![
            VoxelComputeDirectCullMetadata {
                bounds_min: [-0.5, -0.5, 0.1, 0.0],
                bounds_max: [0.5, 0.5, 0.9, 0.0],
            },
            VoxelComputeDirectCullMetadata {
                bounds_min: [2.0, 2.0, 0.1, 0.0],
                bounds_max: [3.0, 3.0, 0.9, 0.0],
            },
        ];
        let commands = vec![
            VoxelComputeDirectDrawCommand {
                vertex_count: 6,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            VoxelComputeDirectDrawCommand {
                vertex_count: 12,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 1,
            },
        ];

        let estimate = compute_direct_cull_estimate(&metadata, &commands, 2, Mat4::IDENTITY);

        assert_eq!(estimate.visible_commands, 1);
        assert_eq!(estimate.visible_indices, 6);
    }

    #[test]
    fn compute_direct_draw_mode_prefers_multi_indirect_when_enabled() {
        let mut settings = VoxelComputeSettings {
            diagnostic_counter_readback: false,
            direct_render: true,
            direct_indirect: true,
            direct_multi_indirect: true,
            direct_gpu_cull: true,
            direct_gpu_cull_compact: true,
            direct_gpu_occlusion_cull: true,
            max_jobs_per_frame: 1,
        };

        assert_eq!(
            direct_draw_mode(&settings, false, 4),
            VoxelComputeDirectDrawMode::Direct
        );
        assert_eq!(
            direct_draw_mode(&settings, true, 0),
            VoxelComputeDirectDrawMode::Direct
        );
        assert_eq!(
            direct_draw_mode(&settings, true, 4),
            VoxelComputeDirectDrawMode::MultiIndirect
        );

        settings.direct_multi_indirect = false;
        assert_eq!(
            direct_draw_mode(&settings, true, 4),
            VoxelComputeDirectDrawMode::Indirect
        );
    }

    #[test]
    fn compute_direct_cull_compact_requires_multi_indirect_and_count_feature() {
        let mut settings = VoxelComputeSettings {
            diagnostic_counter_readback: false,
            direct_render: true,
            direct_indirect: true,
            direct_multi_indirect: true,
            direct_gpu_cull: true,
            direct_gpu_cull_compact: true,
            direct_gpu_occlusion_cull: true,
            max_jobs_per_frame: 1,
        };

        assert!(compute_direct_gpu_cull_compact_enabled(
            &settings,
            VoxelComputeDirectDrawMode::MultiIndirect,
            true
        ));
        assert!(!compute_direct_gpu_cull_compact_enabled(
            &settings,
            VoxelComputeDirectDrawMode::MultiIndirect,
            false
        ));
        assert!(!compute_direct_gpu_cull_compact_enabled(
            &settings,
            VoxelComputeDirectDrawMode::Indirect,
            true
        ));

        settings.direct_gpu_cull_compact = false;
        assert!(!compute_direct_gpu_cull_compact_enabled(
            &settings,
            VoxelComputeDirectDrawMode::MultiIndirect,
            true
        ));
    }

    #[test]
    fn compute_texture_palette_uses_registry_face_tiles() {
        let grass: BlockId = 2;
        let mut mappings: HashMap<BlockId, [u32; 3]> = HashMap::default();
        mappings.insert(grass, [0, 1, 2]);
        let mut tiles = [COMPUTE_FALLBACK_TEXTURE_TILE; COMPUTE_TEXTURE_PALETTE_WORDS];
        copy_texture_mappings_to_buffer(&mut tiles, mappings.iter());
        let offset = usize::from(grass) * COMPUTE_TEXTURES_PER_BLOCK;

        assert_eq!(tiles[offset], 0);
        assert_eq!(tiles[offset + 1], 1);
        assert_eq!(tiles[offset + 2], 2);
    }

    fn test_compute_source_and_contract(
        chunk_pos: ChunkPos,
        y_base: i32,
        neighbor_mask: u32,
    ) -> (VoxelComputeChunkSource, VoxelComputeMeshContract) {
        let source = VoxelComputeChunkSource::new(
            Box::new([0; CHUNK_BLOCK_COUNT]),
            Box::new([0; COMPUTE_BOUNDARY_BLOCK_WORDS]),
            Box::new([0; COMPUTE_TEXTURE_PALETTE_WORDS]),
        );
        let contract = compute_mesh_contract_from_compute_blocks(
            chunk_pos,
            y_base,
            &source.blocks,
            &source.boundary_blocks,
            AIR_BLOCK_ID,
            neighbor_mask,
        );
        (source, contract)
    }
}
