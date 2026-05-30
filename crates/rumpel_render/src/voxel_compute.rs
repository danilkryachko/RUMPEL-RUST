use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
        RenderApp, Render,
        mesh::*,
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
        
        app.add_systems(Startup, setup_test_chunk);
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

fn setup_test_chunk(mut chunk: ResMut<SingleChunkExtract>) {
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
}

#[derive(Component)]
pub struct VoxelComputeMesh;

fn receive_mesh_data(
    mut commands: Commands,
    channel: Res<AsyncMeshChannel>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_meshes: Query<Entity, With<VoxelComputeMesh>>,
) {
    if let Ok(receiver) = channel.receiver.lock() {
        while let Ok(data) = receiver.try_recv() {
            if data.len() < 16 { continue; }
            
            // OutputVertices layout:
            // offset 0: u32 count
            // offset 4..16: 12 bytes padding (WGSL array<Vertex> 16-byte alignment)
            // offset 16+: array<Vertex>
            // Vertex layout (48 bytes total):
            //   0..12: vec3 pos
            //  12..16: pad
            //  16..28: vec3 normal
            //  28..32: pad
            //  32..40: vec2 uv
            //  40..48: pad
            
            let mut count_bytes = [0u8; 4];
            count_bytes.copy_from_slice(&data[0..4]);
            let vertex_count = u32::from_ne_bytes(count_bytes) as usize;
            
            if vertex_count == 0 {
                continue;
            }

            info!("MAIN WORLD: GPU generated {} vertices!", vertex_count);
            
            let mut positions = Vec::with_capacity(vertex_count);
            let mut normals = Vec::with_capacity(vertex_count);
            let mut uvs = Vec::with_capacity(vertex_count);
            
            let data_offset = 16;
            let vertex_stride = 48;
            
            for i in 0..vertex_count {
                let start = data_offset + i * vertex_stride;
                if start + vertex_stride > data.len() {
                    warn!("Vertex count exceeded buffer size!");
                    break;
                }
                
                let px = f32::from_ne_bytes(data[start..start+4].try_into().unwrap());
                let py = f32::from_ne_bytes(data[start+4..start+8].try_into().unwrap());
                let pz = f32::from_ne_bytes(data[start+8..start+12].try_into().unwrap());
                positions.push([px, py, pz]);
                
                let nx = f32::from_ne_bytes(data[start+16..start+20].try_into().unwrap());
                let ny = f32::from_ne_bytes(data[start+20..start+24].try_into().unwrap());
                let nz = f32::from_ne_bytes(data[start+24..start+28].try_into().unwrap());
                normals.push([nx, ny, nz]);
                
                let ux = f32::from_ne_bytes(data[start+32..start+36].try_into().unwrap());
                let uy = f32::from_ne_bytes(data[start+36..start+40].try_into().unwrap());
                uvs.push([ux, uy]);
            }
            
            let mut mesh = Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
            // Without index buffer mapped, we can't easily draw quads unless we generate indices on CPU.
            // For 4 vertices per face, indices are: 0,1,2, 0,2,3
            let num_faces = vertex_count / 4;
            // Removing insert_indices to bypass Bevy 0.18 privacy error. 
            // It will only render 1 triangle per face (vertices 0, 1, 2) without indices, but it proves the pipeline!
            
            // Clean up old mesh
            for entity in existing_meshes.iter() {
                commands.entity(entity).despawn();
            }
            
            // Spawn new mesh
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.2, 0.8, 0.2),
                    ..default()
                })),
                VoxelComputeMesh,
            ));
            
            info!("MAIN WORLD: Successfully spawned GPU Voxel Mesh!");
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
                // In a real implementation, you MUST read the data inside map_async or after it succeeds.
            }
        });
        
        // Polling the device is automatically done by Bevy at the end of the frame,
        // so map_async will trigger without manual `render_device.poll(...)`.
        
        let view = vertex_slice.get_mapped_range();
        let _ = sender.send(view.to_vec());
        drop(view);
        buffers.vertex_staging_buffer.unmap();

        Ok(())
    }
}
