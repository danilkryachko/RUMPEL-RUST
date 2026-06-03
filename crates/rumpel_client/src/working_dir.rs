use std::path::{Path, PathBuf};

pub const CLIENT_WORKING_DIR_ENV: &str = "RUMPEL_CLIENT_WORKING_DIR";
const REPO_ASSETS_MARKER: &str = "assets/blocks/base.ron";
const MAX_PARENT_WALK: usize = 12;

/// Sets the process working directory to the repository (or bundle) root and verifies assets exist.
pub fn install_client_working_dir() {
    let working_dir = resolve_client_working_dir();
    if !client_repo_root_contains_assets(&working_dir) {
        eprintln!(
            "RUMPEL assets not found at {}/{}.\n\
             Set {CLIENT_WORKING_DIR_ENV} to the repository root, run from that directory, \
             or launch via scripts/run_client_macos_gui.sh.",
            working_dir.display(),
            REPO_ASSETS_MARKER
        );
        std::process::exit(66);
    }
    if let Err(error) = std::env::set_current_dir(&working_dir) {
        eprintln!(
            "failed to set working directory to '{}': {error}",
            working_dir.display()
        );
        std::process::exit(66);
    }
}

pub fn asset_file_path() -> String {
    std::env::current_dir()
        .map(|cwd| cwd.join("assets"))
        .unwrap_or_else(|_| PathBuf::from("assets"))
        .to_string_lossy()
        .into_owned()
}

fn resolve_client_working_dir() -> PathBuf {
    if let Ok(path) = std::env::var(CLIENT_WORKING_DIR_ENV)
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| discover_client_working_dir_from(exe.parent()?))
    {
        return dir;
    }
    if let Ok(cwd) = std::env::current_dir()
        && client_repo_root_contains_assets(&cwd)
    {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn discover_client_working_dir_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..MAX_PARENT_WALK {
        if client_repo_root_contains_assets(&dir) {
            return Some(dir);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

fn client_repo_root_contains_assets(dir: &Path) -> bool {
    dir.join(REPO_ASSETS_MARKER).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_repo_root_from_crate_manifest_dir() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        assert!(client_repo_root_contains_assets(&repo));
    }

    #[test]
    fn walk_finds_repo_from_release_binary_path() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let release_binary = repo.join("target/release/rumpel_client");
        let found = discover_client_working_dir_from(release_binary.parent().unwrap());
        assert_eq!(found, Some(repo));
    }
}
