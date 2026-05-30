use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        RenderApp,
    },
};
use std::sync::{mpsc, Mutex};

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct VoxelComputeLabel;

pub struct VoxelComputePlugin;

impl Plugin for VoxelComputePlugin {
    fn build(&self, app: &mut App) {
        // We will initialize the compute pipeline and render graph here
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<VoxelComputePipeline>();

        let (sender, receiver) = mpsc::channel();
        render_app.insert_resource(AsyncMeshChannel { 
            sender, 
            receiver: Mutex::new(receiver) 
        });

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(VoxelComputeLabel, VoxelComputeNode::default());
        // We will order this node later depending on our draw systems
    }
}

#[derive(Resource)]
struct VoxelComputePipeline {
    pipeline: CachedComputePipelineId,
    bind_group_layout: BindGroupLayout,
}

impl FromWorld for VoxelComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let bind_group_layout = render_device.create_bind_group_layout(
            "voxel_compute_bind_group_layout",
            &[
                // Binding 0: Chunk Data (Read)
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
                // Binding 1: Vertices (Read/Write)
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
                // Binding 2: Indices (Read/Write)
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

        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) else {
            return Ok(());
        };

        // TODO: Actually extract chunks and write them to a buffer
        let dummy_chunk_data = vec![0u32; 32768];
        let chunk_buffer = render_context.render_device().create_buffer_with_data(
            &BufferInitDescriptor {
                label: Some("chunk_data_buffer"),
                contents: bytemuck::cast_slice(&dummy_chunk_data),
                usage: BufferUsages::STORAGE,
            }
        );

        let vertex_buffer = render_context.render_device().create_buffer(&BufferDescriptor {
            label: Some("vertex_output_buffer"),
            size: 1024 * 1024 * 16, // 16MB max vertices per chunk
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let index_buffer = render_context.render_device().create_buffer(&BufferDescriptor {
            label: Some("index_output_buffer"),
            size: 1024 * 1024 * 4, // 4MB max indices
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let vertex_staging_buffer = render_context.render_device().create_buffer(&BufferDescriptor {
            label: Some("vertex_staging_buffer"),
            size: 1024 * 1024 * 16,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_staging_buffer = render_context.render_device().create_buffer(&BufferDescriptor {
            label: Some("index_staging_buffer"),
            size: 1024 * 1024 * 4,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = render_context.render_device().create_bind_group(
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

        {
            let mut pass = render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("voxel_compute_pass"),
                    timestamp_writes: None,
                });

            pass.set_pipeline(compute_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(8, 8, 8); // 32/4 = 8
        } // Drop the pass to release the encoder borrow

        // Copy from fast GPU Storage to slow CPU-mappable Staging Buffers
        render_context.command_encoder().copy_buffer_to_buffer(
            &vertex_buffer, 0,
            &vertex_staging_buffer, 0,
            1024 * 1024 * 16
        );

        render_context.command_encoder().copy_buffer_to_buffer(
            &index_buffer, 0,
            &index_staging_buffer, 0,
            1024 * 1024 * 4
        );

        // Map buffer async and read vertices back
        let vertex_slice = vertex_staging_buffer.slice(..);
        
        let (map_sender, map_receiver) = mpsc::channel();
        vertex_slice.map_async(MapMode::Read, move |result| {
            let _ = map_sender.send(result);
        });

        // In a real implementation we would not wait synchronously here,
        // but store the receiver in a resource and check it next frame.
        // For demonstration of the pipeline architecture, we prepare the hook:
        
        // if let Ok(Ok(())) = map_receiver.try_recv() {
        //     let data = vertex_slice.get_mapped_range();
        //     // Copy data out and send to Main World via AsyncMeshChannel
        //     // vertex_staging_buffer.unmap();
        // }
        
        Ok(())
    }
}

#[derive(Resource)]
pub struct AsyncMeshChannel {
    pub sender: mpsc::Sender<Vec<u8>>,
    pub receiver: Mutex<mpsc::Receiver<Vec<u8>>>,
}
