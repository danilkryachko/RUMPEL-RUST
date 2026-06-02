use bevy::{
    image::{ImageArrayLayout, ImageLoaderSettings},
    mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey, MaterialPlugin},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
    },
    shader::ShaderRef,
};

pub const BLOCK_ATLAS_PATH: &str = "textures/blocks/voxel_texture_array.png";
pub const BLOCK_ATLAS_ROWS: u32 = 28;
pub const VOXEL_QUAD_SHADER_PATH: &str = "shaders/voxel_quads.wgsl";

pub const ATTRIBUTE_VOXEL_REPEAT_UV: MeshVertexAttribute =
    MeshVertexAttribute::new("VoxelRepeatUv", 0xA11A_7102, VertexFormat::Float32x2);
pub const ATTRIBUTE_VOXEL_TILE: MeshVertexAttribute =
    MeshVertexAttribute::new("VoxelTile", 0xA11A_7101, VertexFormat::Uint32);

pub struct VoxelQuadMaterialPlugin;

impl Plugin for VoxelQuadMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<VoxelQuadMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelQuadMaterial {
    #[texture(0, dimension = "2d_array")]
    #[sampler(1)]
    pub atlas: Handle<Image>,
}

impl Material for VoxelQuadMaterial {
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        VOXEL_QUAD_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        VOXEL_QUAD_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_VOXEL_REPEAT_UV.at_shader_location(3),
            ATTRIBUTE_VOXEL_TILE.at_shader_location(4),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = Some(Face::Back);
        Ok(())
    }
}

pub fn load_block_atlas(asset_server: &AssetServer) -> Handle<Image> {
    asset_server.load_with_settings(BLOCK_ATLAS_PATH, |settings: &mut ImageLoaderSettings| {
        settings.array_layout = Some(ImageArrayLayout::RowCount {
            rows: BLOCK_ATLAS_ROWS,
        });
    })
}
