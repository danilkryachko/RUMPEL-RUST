#!/bin/bash

if [ -z "$1" ]; then
    echo "Usage: ./scripts/new_module.sh <module_name>"
    echo "Example: ./scripts/new_module.sh inventory"
    exit 1
fi

MODULE_NAME=$1
CRATE_NAME="rumpel_$MODULE_NAME"
CRATE_PATH="crates/$CRATE_NAME"

# Check if the crate already exists
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

echo "Setting up basic Bevy plugin template in src/lib.rs..."
# Convert snake_case to PascalCase for the Plugin name
PLUGIN_NAME=$(echo "$CRATE_NAME" | awk -F'_' '{for(i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) tolower(substr($i,2))}1' OFS='')

cat <<EOF > "$CRATE_PATH/src/lib.rs"
use bevy::prelude::*;
use rumpel_prelude::*;

pub struct ${PLUGIN_NAME}Plugin;

impl Plugin for ${PLUGIN_NAME}Plugin {
    fn build(&self, app: &mut App) {
        // Add systems and resources here
    }
}
EOF

echo "Adding $CRATE_NAME to the workspace..."
# Assuming [workspace.dependencies] exists and the root Cargo.toml is properly formatted.
# This simple append adds it to the root Cargo.toml.
echo "$CRATE_NAME = { path = \"crates/$CRATE_NAME\" }" >> Cargo.toml

echo "Done! Crate $CRATE_NAME has been successfully created."
echo "Don't forget to:"
echo "1. Export its types in rumpel_prelude (if needed)."
echo "2. Add ${PLUGIN_NAME}Plugin to the main App in rumpel_client."
