use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

#[cfg(feature = "inspector")]
use bevy_egui::EguiPlugin;
#[cfg(feature = "inspector")]
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use crate::hud::{debug_camera_components, spawn_fps_hud, update_debug_hud};

const DEBUG_HUD_ENV: &str = "RUMPEL_DEBUG_HUD";

pub struct RumpelDebugPlugin;

impl Plugin for RumpelDebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        if debug_hud_enabled() {
            app.add_systems(Startup, spawn_fps_hud).add_systems(
                Update,
                (update_debug_hud, debug_camera_components.run_if(run_once)),
            );
        } else {
            info!("HUD: Debug HUD disabled by RUMPEL_DEBUG_HUD");
        }

        #[cfg(feature = "inspector")]
        app.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::new()));
    }
}

fn debug_hud_enabled() -> bool {
    std::env::var(DEBUG_HUD_ENV).map_or(true, |value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => true,
    })
}
