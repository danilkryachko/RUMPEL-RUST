# Development Workflow

## Daily Commands

- `just dev`: run the local development build with Bevy dynamic linking.
- `just release`: run the release build without Bevy dynamic linking.
- `just verify`: run formatting, check, Clippy, and tests.
- `just changelog`: regenerate `CHANGELOG.md` from Conventional Commit history.

The project targets Bevy `0.18.x`. Keep Bevy ecosystem crates on versions compatible with that line.

## Hooks

Install local hooks with:

```bash
pre-commit install
pre-commit install --hook-type pre-push
```

Pre-commit runs lightweight file hygiene and formatting checks. Pre-push runs the full Clippy gate.

## Dependency Hygiene

- `just deny`: run `cargo-deny` for advisories, licenses, bans, and source checks.
- `just machete`: run `cargo-machete` for unused dependency detection.

Install optional tools when needed:

```bash
cargo install cargo-deny cargo-machete git-cliff
```
