use bevy::{
    prelude::*,
    platform::collections::HashSet,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{
            binding_types::{storage_buffer, uniform_buffer, storage_buffer_read_only},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        RenderApp, Render,
        mesh::allocator::MeshAllocator,
        mesh::*,
    },
};
use std::ops::Not;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct VoxelComputeLabel;

pub struct VoxelComputePlugin;

impl Plugin for VoxelComputePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SingleChunkExtract>();
        app.add_plugins(ExtractResourcePlugin::<SingleChunkExtract>::default());
        app.add_plugins(ExtractComponentPlugin::<GenerateChunkMesh>::default());
        app.add_systems(Startup, setup_test_chunk);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        
        render_app.init_resource::<VoxelComputePipeline>();
        render_app.init_resource::<ChunksToProcess>();
        render_app.add_systems(Render, prepare_buffers);
        render_app.add_systems(Render, prepare_chunks);

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(VoxelComputeLabel, VoxelComputeNode::default());
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .world_mut()
            .resource_mut::<bevy::render::mesh::allocator::MeshAllocator>()
            .extra_buffer_usages = BufferUsages::STORAGE;
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub struct GenerateChunkMesh(pub Handle<Mesh>);

#[derive(Resource, Clone, ExtractResource)]
pub struct SingleChunkExtract {
    pub blocks: Box<[u32; 32768]>, // WGSL expects array<u32>
    pub has_changes: bool,
}

impl Default for SingleChunkExtract {
    fn default() -> Self {
        Self {
            blocks: Box::new([0; 32768]),
            has_changes: false,
        }
    }
}

fn setup_test_chunk(
    mut commands: Commands,
    mut chunk: ResMut<SingleChunkExtract>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Generate a simple test shape: A 10x10x10 cube of blocks at the center
    for x in 10..20 {
        for y in 0..10 {
            for z in 10..20 {
                let idx = x + y * 32 + z * 32 * 32;
                chunk.blocks[idx] = 1; // Solid block
            }
        }
    }
    chunk.has_changes = true;
    
    // Allocate a giant empty mesh (capacity for up to 15,000 vertices and 30,000 indices)
    let empty_mesh = {
        let mut mesh = Mesh::new(
            bevy::render::render_resource::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 15000])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.; 3]; 15000])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.; 2]; 15000])
        .with_inserted_indices(bevy::mesh::Indices::U32(vec![0; 30000]));

        mesh.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
        mesh
    };

    let handle = meshes.add(empty_mesh);

    // Spawn the chunk into the world with standard PBR material
    commands.spawn((
        GenerateChunkMesh(handle.clone()),
        Mesh3d(handle.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 0.2),
            ..default()
        })),
        Transform::default(),
    ));
}

#[derive(Component)]
pub struct VoxelComputeMesh;

#[derive(Resource, Default)]
struct ChunksToProcess(Vec<AssetId<Mesh>>);

fn prepare_chunks(
    meshes_to_generate: Query<&GenerateChunkMesh>,
    mut chunks: ResMut<ChunksToProcess>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<VoxelComputePipeline>,
    mut processed: Local<HashSet<AssetId<Mesh>>>,
) {
    if pipeline_cache
        .get_compute_pipeline(pipeline.pipeline)
        .is_some()
    {
        let chunk_data: Vec<AssetId<Mesh>> = meshes_to_generate
            .iter()
            .filter_map(|gmesh| {
                let id = gmesh.0.id();
                processed.contains(&id).not().then_some(id)
            })
            .collect();

        for id in &chunk_data {
            processed.insert(*id);
        }

        chunks.0 = chunk_data;
    }
}

#[derive(ShaderType)]
struct DataRanges {
    vertex_start: u32,
    vertex_end: u32,
    index_start: u32,
    index_end: u32,
}

#[derive(Resource)]
struct VoxelComputePipeline {
    pipeline: CachedComputePipelineId,
    bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
struct VoxelComputeBuffers {
    chunk_buffer: Buffer,
}

impl FromWorld for VoxelComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let bind_group_layout = render_device.create_bind_group_layout(
            "voxel_compute_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    storage_buffer_read_only::<[u32; 32768]>(false), // binding 0: VoxelData
                    uniform_buffer::<DataRanges>(false),             // binding 1: DataRanges
                    storage_buffer::<Vec<u32>>(false),               // binding 2: Vertex buffer
                    storage_buffer::<Vec<u32>>(false),               // binding 3: Index buffer
                ),
            ),
        );

        let shader = world.resource::<AssetServer>().load("shaders/mesh_generator.wgsl");
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("voxel_compute_pipeline".into()),
            layout: vec![], // TODO: Fix layout descriptor
            shader,
            shader_defs: vec![],
            entry_point: Some(std::borrow::Cow::Borrowed("main")),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}

fn prepare_buffers(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut extracted_chunk: ResMut<SingleChunkExtract>,
    mut buffers: Local<Option<VoxelComputeBuffers>>,
) {
    if buffers.is_none() {
        let chunk_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("chunk_data_buffer"),
            size: (32768 * 4) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        *buffers = Some(VoxelComputeBuffers {
            chunk_buffer,
        });
    }

    if let Some(bufs) = &mut *buffers {
        if extracted_chunk.has_changes {
            render_queue.write_buffer(&bufs.chunk_buffer, 0, bytemuck::cast_slice(&*extracted_chunk.blocks));
            extracted_chunk.has_changes = false;
        }
    }
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
        let Some(buffers) = world.get_resource::<VoxelComputeBuffers>() else {
            return Ok(());
        };

        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) else {
            return Ok(());
        };

        for mesh_id in &chunks.0 {
            info!(?mesh_id, "GPU processing mesh chunk!");

            let vertex_buffer_slice = mesh_allocator.mesh_vertex_slice(mesh_id).unwrap();
            let index_buffer_slice = mesh_allocator.mesh_index_slice(mesh_id).unwrap();

            let first = DataRanges {
                // 8 f32s per vertex (pos: 3, normal: 3, uv: 2)
                vertex_start: vertex_buffer_slice.range.start * 8,
                vertex_end: vertex_buffer_slice.range.end * 8,
                index_start: index_buffer_slice.range.start,
                index_end: index_buffer_slice.range.end,
            };

            let mut uniforms = UniformBuffer::from(first);
            uniforms.write_buffer(render_context.render_device(), &render_queue);

            let bind_group = render_context.render_device().create_bind_group(
                Some("voxel_compute_bind_group"),
                &pipeline.bind_group_layout,
                &BindGroupEntries::sequential((
                    buffers.chunk_buffer.as_entire_buffer_binding(),
                    &uniforms,
                    vertex_buffer_slice.buffer.as_entire_buffer_binding(),
                    index_buffer_slice.buffer.as_entire_buffer_binding(),
                )),
            );

            let mut pass = render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("voxel_compute_pass"),
                    timestamp_writes: None,
                });

            pass.set_pipeline(compute_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(8, 8, 8); // 32/4 = 8
        }

        Ok(())
    }
}
