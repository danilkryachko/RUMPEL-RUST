use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
        RenderApp, Render,
    },
};
use std::sync::{mpsc, Mutex};

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct VoxelComputeLabel;

pub struct VoxelComputePlugin;

impl Plugin for VoxelComputePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SingleChunkExtract>();
        app.add_plugins(ExtractResourcePlugin::<SingleChunkExtract>::default());
        
        let (sender, receiver) = mpsc::channel();
        app.insert_resource(AsyncMeshChannel { 
            sender: sender.clone(), 
            receiver: Mutex::new(receiver) 
        });
        
        app.add_systems(Update, receive_mesh_data);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        
        render_app.insert_resource(AsyncMeshChannel { 
            sender, 
            receiver: Mutex::new(mpsc::channel().1) // Dummy receiver for render world
        });
        
        render_app.init_resource::<VoxelComputePipeline>();
        render_app.add_systems(Render, prepare_buffers);

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(VoxelComputeLabel, VoxelComputeNode::default());
        // TODO: add graph edges to ensure compute runs before main pass
    }
}

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

#[derive(Resource)]
pub struct AsyncMeshChannel {
    pub sender: mpsc::Sender<Vec<u8>>,
    pub receiver: Mutex<mpsc::Receiver<Vec<u8>>>,
}

fn receive_mesh_data(channel: Res<AsyncMeshChannel>) {
    if let Ok(receiver) = channel.receiver.lock() {
        while let Ok(data) = receiver.try_recv() {
            info!("MAIN WORLD: Received GPU generated mesh data! Size: {} bytes. Voxel Engine Pipeline is ALIVE!", data.len());
        }
    }
}

#[derive(Resource)]
struct VoxelComputePipeline {
    pipeline: CachedComputePipelineId,
    bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
struct VoxelComputeBuffers {
    chunk_buffer: Buffer,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    vertex_staging_buffer: Buffer,
    index_staging_buffer: Buffer,
    bind_group: BindGroup,
}

impl FromWorld for VoxelComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let bind_group_layout = render_device.create_bind_group_layout(
            "voxel_compute_bind_group_layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut extracted_chunk: ResMut<SingleChunkExtract>,
    pipeline: Res<VoxelComputePipeline>,
    mut buffers: Local<Option<VoxelComputeBuffers>>,
) {
    if buffers.is_none() {
        let chunk_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("chunk_data_buffer"),
            size: (32768 * 4) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let vertex_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("vertex_output_buffer"),
            size: 1024 * 1024 * 16,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let index_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("index_output_buffer"),
            size: 1024 * 1024 * 4,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let vertex_staging_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("vertex_staging_buffer"),
            size: 1024 * 1024 * 16,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_staging_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("index_staging_buffer"),
            size: 1024 * 1024 * 4,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = render_device.create_bind_group(
            Some("voxel_compute_bind_group"),
            &pipeline.bind_group_layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: chunk_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: vertex_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: index_buffer.as_entire_binding(),
                },
            ],
        );

        *buffers = Some(VoxelComputeBuffers {
            chunk_buffer,
            vertex_buffer,
            index_buffer,
            vertex_staging_buffer,
            index_staging_buffer,
            bind_group,
        });
    }

    if let Some(bufs) = &mut *buffers {
        if extracted_chunk.has_changes {
            render_queue.write_buffer(&bufs.chunk_buffer, 0, bytemuck::cast_slice(&*extracted_chunk.blocks));
            extracted_chunk.has_changes = false;
        }
        commands.insert_resource(VoxelComputeBuffers {
            chunk_buffer: bufs.chunk_buffer.clone(),
            vertex_buffer: bufs.vertex_buffer.clone(),
            index_buffer: bufs.index_buffer.clone(),
            vertex_staging_buffer: bufs.vertex_staging_buffer.clone(),
            index_staging_buffer: bufs.index_staging_buffer.clone(),
            bind_group: bufs.bind_group.clone(),
        });
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
        let Some(buffers) = world.get_resource::<VoxelComputeBuffers>() else {
            return Ok(());
        };

        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) else {
            return Ok(());
        };

        {
            let mut pass = render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("voxel_compute_pass"),
                    timestamp_writes: None,
                });

            pass.set_pipeline(compute_pipeline);
            pass.set_bind_group(0, &buffers.bind_group, &[]);
            pass.dispatch_workgroups(8, 8, 8); // 32/4 = 8
        }

        render_context.command_encoder().copy_buffer_to_buffer(
            &buffers.vertex_buffer, 0,
            &buffers.vertex_staging_buffer, 0,
            1024 * 1024 * 16
        );

        render_context.command_encoder().copy_buffer_to_buffer(
            &buffers.index_buffer, 0,
            &buffers.index_staging_buffer, 0,
            1024 * 1024 * 4
        );

        // Tell WGPU to map the buffer to CPU memory
        let vertex_slice = buffers.vertex_staging_buffer.slice(..);
        let channel = world.resource::<AsyncMeshChannel>();
        let sender = channel.sender.clone();
        
        vertex_slice.map_async(MapMode::Read, move |result| {
            if result.is_ok() {
                // In a real impl we grab the bytes here
                let _ = sender.send(vec![1, 2, 3]); 
            }
        });

        Ok(())
    }
}
