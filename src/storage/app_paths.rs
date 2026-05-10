use std::path::PathBuf;

use super::permissions;

const STATE_FILE_NAME: &str = "state.json";
const CONFIGS_DIR_NAME: &str = "configs";

#[derive(Clone)]
pub struct AppStoragePaths {
    pub root: PathBuf,
    pub configs_dir: PathBuf,
    pub state_path: PathBuf,
}

impl AppStoragePaths {
    pub fn config_path(&self, id: u64) -> PathBuf {
        self.configs_dir.join(format!("{id}.conf"))
    }
}

pub fn ensure_app_storage_dirs() -> Result<AppStoragePaths, String> {
    let root = dirs::data_dir()
        .map(|dir| dir.join("r-wg"))
        .ok_or_else(|| "No data directory available".to_string())?;
    ensure_app_storage_dirs_at(root)
}

pub fn ensure_app_storage_dirs_at(root: PathBuf) -> Result<AppStoragePaths, String> {
    let configs_dir = root.join(CONFIGS_DIR_NAME);
    let state_path = root.join(STATE_FILE_NAME);
    permissions::ensure_private_dir(&root)
        .map_err(|err| format!("Create storage root failed: {err}"))?;
    permissions::ensure_private_dir(&configs_dir)
        .map_err(|err| format!("Create configs dir failed: {err}"))?;
    Ok(AppStoragePaths {
        root,
        configs_dir,
        state_path,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("r-wg-app-paths-{label}-{unique}"))
    }

    #[test]
    fn ensure_app_storage_dirs_builds_expected_paths() {
        let root = temp_root("paths");

        let paths = ensure_app_storage_dirs_at(root.clone()).expect("paths should be created");

        assert_eq!(paths.root, root);
        assert_eq!(paths.configs_dir, paths.root.join(CONFIGS_DIR_NAME));
        assert_eq!(paths.state_path, paths.root.join(STATE_FILE_NAME));
        assert_eq!(paths.config_path(42), paths.configs_dir.join("42.conf"));
        assert!(paths.configs_dir.is_dir());
        let _ = fs::remove_dir_all(paths.root);
    }
}
