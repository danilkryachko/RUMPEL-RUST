#!/bin/bash

if [ -z "$1" ]; then
    echo "Usage: ./scripts/new_module.sh <module_name>"
    echo "Example: ./scripts/new_module.sh inventory"
    exit 1
fi

MODULE_NAME=$1
CRATE_NAME="rumpel_$MODULE_NAME"
CRATE_PATH="crates/$CRATE_NAME"

if [ -d "$CRATE_PATH" ]; then
    echo "Error: Crate $CRATE_NAME already exists."
    exit 1
fi

echo "Creating library crate $CRATE_NAME..."
cargo new --lib "$CRATE_PATH"

echo "Updating Cargo.toml dependencies..."
cat <<EOF > "$CRATE_PATH/Cargo.toml"
[package]
name = "$CRATE_NAME"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true }
rumpel_prelude = { workspace = true }
EOF

echo "Setting up modular Bevy plugin structure..."
PLUGIN_NAME=$(echo "$CRATE_NAME" | awk -F'_' '{for(i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) tolower(substr($i,2))}1' OFS='')

# Create structured files
touch "$CRATE_PATH/src/plugin.rs"
touch "$CRATE_PATH/src/components.rs"
touch "$CRATE_PATH/src/systems.rs"
touch "$CRATE_PATH/src/events.rs"

cat <<EOF > "$CRATE_PATH/src/lib.rs"
pub mod plugin;
pub mod components;
pub mod systems;
pub mod events;

pub use plugin::*;
pub use components::*;
pub use systems::*;
pub use events::*;
EOF

cat <<EOF > "$CRATE_PATH/src/plugin.rs"
use bevy::prelude::*;

pub struct ${PLUGIN_NAME}Plugin;

impl Plugin for ${PLUGIN_NAME}Plugin {
    fn build(&self, _app: &mut App) {
    }
}
EOF

cat <<EOF > "$CRATE_PATH/src/components.rs"
EOF

cat <<EOF > "$CRATE_PATH/src/systems.rs"
EOF

cat <<EOF > "$CRATE_PATH/src/events.rs"
EOF

echo "Adding $CRATE_NAME to the workspace..."
if grep -q "^$CRATE_NAME = " Cargo.toml; then
    echo "$CRATE_NAME already exists in workspace dependencies."
else
    TMP_FILE=$(mktemp)
    awk -v dep="$CRATE_NAME = { path = \"crates/$CRATE_NAME\" }" '
        /^\[profile\.dev\]/ && !inserted {
            print dep
            print ""
            inserted = 1
        }
        { print }
        END {
            if (!inserted) {
                print dep
            }
        }
    ' Cargo.toml > "$TMP_FILE"
    mv "$TMP_FILE" Cargo.toml
fi

echo "Done! Crate $CRATE_NAME has been successfully created with a modular structure."
