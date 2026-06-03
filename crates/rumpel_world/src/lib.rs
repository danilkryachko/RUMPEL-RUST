pub mod chunk;
pub mod physics;
pub mod voxel_runtime;
pub mod world_gen;

#[derive(bevy::prelude::Resource, Debug, Clone, Copy)]
pub struct RumpelTime {
    pub elapsed_time: f32,
    pub sun_angle: f32,
    pub is_raining: bool,
    pub lightning_flash: f32,
}

impl Default for RumpelTime {
    fn default() -> Self {
        Self {
            elapsed_time: std::f32::consts::FRAC_PI_2,
            sun_angle: 1.0,
            is_raining: false,
            lightning_flash: 0.0,
        }
    }
}
