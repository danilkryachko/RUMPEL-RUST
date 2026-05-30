use bevy::prelude::*;
use bevy_inspector_egui::quick::WorldInspectorPlugin;

pub struct RumpelDebugPlugin;

impl Plugin for RumpelDebugPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(debug_assertions)]
        {
            app.add_plugins(WorldInspectorPlugin::new());
        }
    }
}
