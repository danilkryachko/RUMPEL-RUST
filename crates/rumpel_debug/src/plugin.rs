#[cfg(debug_assertions)]
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

#[cfg(feature = "inspector")]
use bevy_egui::EguiPlugin;
#[cfg(feature = "inspector")]
use bevy_inspector_egui::quick::WorldInspectorPlugin;

#[cfg(debug_assertions)]
use crate::hud::{spawn_fps_hud, update_debug_hud};

pub struct RumpelDebugPlugin;

impl Plugin for RumpelDebugPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(debug_assertions)]
        {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default())
                .add_systems(Startup, spawn_fps_hud)
                .add_systems(Update, update_debug_hud);

            #[cfg(feature = "inspector")]
            app.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::new()));
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = app;
        }
    }
}
