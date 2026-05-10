use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use super::atomic;

#[derive(Clone, Default)]
pub struct ConfigRepository;

impl ConfigRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn read_text(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    pub fn write_text(&self, path: &Path, text: &str) -> io::Result<()> {
        atomic::write_atomic(path, text.as_bytes())
    }

    pub fn delete_files(&self, paths: &[PathBuf]) -> io::Result<()> {
        for path in paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("r-wg-config-repo-{label}-{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn read_and_write_text_round_trips() {
        let dir = temp_dir("round-trip");
        let path = dir.join("alpha.conf");
        let repository = ConfigRepository::new();

        repository
            .write_text(&path, "[Interface]\nPrivateKey = secret\n")
            .expect("config should write");

        assert_eq!(
            repository.read_text(&path).expect("config should read"),
            "[Interface]\nPrivateKey = secret\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_text_uses_private_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("mode");
        let path = dir.join("secret.conf");
        let repository = ConfigRepository::new();

        repository
            .write_text(&path, "PrivateKey = secret\n")
            .expect("config should write");

        let mode = fs::metadata(&path)
            .expect("metadata should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_files_ignores_missing_paths() {
        let dir = temp_dir("delete");
        let existing = dir.join("existing.conf");
        let missing = dir.join("missing.conf");
        fs::write(&existing, "config").expect("fixture should write");
        let repository = ConfigRepository::new();

        repository
            .delete_files(&[existing.clone(), missing])
            .expect("delete should ignore missing files");

        assert!(!existing.exists());
        let _ = fs::remove_dir_all(dir);
    }
}
