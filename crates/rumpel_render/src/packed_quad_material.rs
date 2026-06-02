use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey, MaterialPlugin},
    prelude::*,
    reflect::TypePath,
    render::{
        render_asset::RenderAssets,
        render_resource::*,
        renderer::RenderDevice,
        texture::{FallbackImage, GpuImage},
    },
    shader::ShaderRef,
};

use crate::packed_quad_pipeline::{
    PackedQuadGpuArena, PreparedPackedQuadBatches, PreparedPackedQuadBlockTexturePalette,
};

pub const PACKED_VOXEL_MATERIAL_SHADER_PATH: &str = "shaders/packed_voxel_material.wgsl";
const PACKED_FACE_DEBUG_ENV: &str = "RUMPEL_PACKED_FACE_DEBUG";

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PackedVoxelUniform {
    pub chunk_translation_and_offset: Vec4, // xyz: translation, w: offset
    pub fog_color_and_start: Vec4,          // xyz: color, w: fog_start
    pub fog_end_and_padding: Vec4,          // x: fog_end, yzw: padding
}

unsafe impl bytemuck::Zeroable for PackedVoxelUniform {}
unsafe impl bytemuck::Pod for PackedVoxelUniform {}

impl PackedVoxelUniform {
    pub const SIZE: u64 = std::mem::size_of::<Self>() as u64;
}

#[derive(Asset, TypePath, Clone, Debug)]
pub struct PackedVoxelMaterial {
    pub atlas: Handle<Image>,
    pub batch_key: u64,
}

pub struct PackedVoxelMaterialPlugin;

impl Plugin for PackedVoxelMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PackedVoxelMaterial>::default());
    }
}

impl AsBindGroup for PackedVoxelMaterial {
    type Data = ();
    type Param = (
        Res<'static, PackedQuadGpuArena>,
        Res<'static, PreparedPackedQuadBlockTexturePalette>,
        Res<'static, RenderAssets<GpuImage>>,
        Res<'static, FallbackImage>,
        Res<'static, PreparedPackedQuadBatches>,
    );

    fn label() -> &'static str {
        "packed_voxel_material"
    }

    fn bind_group_data(&self) -> Self::Data {}

    fn bind_group_layout_entries(
        _render_device: &RenderDevice,
        _is_fallback: bool,
    ) -> Vec<BindGroupLayoutEntry> {
        vec![
            // @binding(0) quads: array<vec4<u32>> (storage buffer read-only)
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // @binding(1) draw_params: DrawParams (uniform buffer)
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(
                        std::num::NonZeroU64::new(PackedVoxelUniform::SIZE).unwrap(),
                    ),
                },
                count: None,
            },
            // @binding(2) block_tiles: array<vec4<u32>> (storage buffer read-only)
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // @binding(3) block_atlas: texture_2d_array<f32>
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            // @binding(4) block_atlas_sampler: sampler
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ]
    }

    fn unprepared_bind_group(
        &self,
        _layout: &BindGroupLayout,
        _render_device: &RenderDevice,
        param: &mut bevy::ecs::system::SystemParamItem<'_, '_, Self::Param>,
        _is_fallback: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let arena = &param.0;
        let palette = &param.1;
        let render_assets_images = &param.2;
        let fallback_image = &param.3;
        let prepared_batches = &param.4;

        // Извлекаем буфер арены квадов
        let arena_buffer = arena
            .buffer
            .as_ref()
            .ok_or(AsBindGroupError::RetryNextUpdate)?;

        // Извлекаем буфер палитры
        let palette_buffer = palette
            .buffer
            .as_ref()
            .ok_or(AsBindGroupError::RetryNextUpdate)?;

        let batch_info = prepared_batches
            .batches
            .get(&self.batch_key)
            .ok_or(AsBindGroupError::RetryNextUpdate)?;

        // Получаем текстуру атласа и сэмплер
        let (texture_view, sampler) = if let Some(gpu_image) = render_assets_images.get(&self.atlas)
        {
            (&gpu_image.texture_view, &gpu_image.sampler)
        } else {
            (
                &fallback_image.d2_array.texture_view,
                &fallback_image.d2_array.sampler,
            )
        };

        let bindings = vec![
            (0, OwnedBindingResource::Buffer(arena_buffer.clone())),
            (
                1,
                OwnedBindingResource::Buffer(batch_info.translation_buffer.clone()),
            ),
            (2, OwnedBindingResource::Buffer(palette_buffer.clone())),
            (
                3,
                OwnedBindingResource::TextureView(
                    TextureViewDimension::D2Array,
                    texture_view.clone(),
                ),
            ),
            (
                4,
                OwnedBindingResource::Sampler(SamplerBindingType::Filtering, sampler.clone()),
            ),
        ];

        Ok(UnpreparedBindGroup {
            bindings: BindingResources(bindings),
        })
    }
}

impl Material for PackedVoxelMaterial {
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        PACKED_VOXEL_MATERIAL_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        PACKED_VOXEL_MATERIAL_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Back);
        if env_flag(PACKED_FACE_DEBUG_ENV) {
            descriptor
                .vertex
                .shader_defs
                .push("PACKED_FACE_DEBUG".into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push("PACKED_FACE_DEBUG".into());
            }
        }
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
