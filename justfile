set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check:
    cargo check --workspace --all-targets

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace

verify: fmt-check check clippy test

dev:
    cargo run -p rumpel_client --features dev_dynamic_linking

release:
    cargo run -p rumpel_client --release

deny:
    cargo deny check

machete:
    cargo machete

changelog:
    git-cliff -o CHANGELOG.md

new-module name:
    ./scripts/new_module.sh "{{name}}"
