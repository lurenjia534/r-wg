use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(windows)]
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_atomic_with_options(path, contents, WriteOptions::default())
}

#[cfg(unix)]
pub fn write_atomic_with_mode(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    write_atomic_with_options(path, contents, WriteOptions { mode })
}

struct WriteOptions {
    #[cfg(unix)]
    mode: u32,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            #[cfg(unix)]
            mode: 0o600,
        }
    }
}

fn write_atomic_with_options(
    path: &Path,
    contents: &[u8],
    options: WriteOptions,
) -> io::Result<()> {
    let tmp_path = unique_temp_path(path);
    let write_result = write_temp_file(&tmp_path, contents, options)
        .and_then(|()| commit_temp_file(&tmp_path, path))
        .and_then(|()| sync_parent_dir(path));

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_result
}

fn write_temp_file(
    tmp_path: &Path,
    contents: &[u8],
    write_options: WriteOptions,
) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(write_options.mode);

    let mut file = options.open(tmp_path)?;
    file.write_all(contents)?;
    file.sync_all()
}

fn commit_temp_file(tmp_path: &Path, path: &Path) -> io::Result<()> {
    match fs::rename(tmp_path, path) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(error) if error.kind() == ErrorKind::AlreadyExists && path.is_file() => {
            fs::remove_file(path)?;
            fs::rename(tmp_path, path)
        }
        Err(error) => Err(error),
    }
}

fn sync_parent_dir(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => File::open(parent)?.sync_all(),
        None => Ok(()),
    }
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("atomic");
    path.with_file_name(format!(".{file_name}.{pid}.{counter}.tmp"))
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
        let dir = std::env::temp_dir().join(format!("r-wg-atomic-{label}-{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn write_atomic_creates_file() {
        let dir = temp_dir("create");
        let path = dir.join("state.json");

        write_atomic(&path, b"hello").expect("atomic write should succeed");

        assert_eq!(fs::read(&path).expect("file should be readable"), b"hello");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = temp_dir("replace");
        let path = dir.join("config.conf");
        fs::write(&path, b"old").expect("fixture should write");

        write_atomic(&path, b"new").expect("atomic write should replace");

        assert_eq!(fs::read(&path).expect("file should be readable"), b"new");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_atomic_removes_temp_file_after_commit_failure() {
        let dir = temp_dir("cleanup");
        let path = dir.join("target");
        fs::create_dir(&path).expect("directory target should be created");

        let err = write_atomic(&path, b"new").expect_err("rename over directory should fail");

        assert!(err.kind() != io::ErrorKind::NotFound);
        let leaked_temp = fs::read_dir(&dir)
            .expect("temp dir should be readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .any(|file_name| file_name.starts_with(".target.") && file_name.ends_with(".tmp"));
        assert!(!leaked_temp);
        assert!(path.is_dir());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_sets_private_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("mode");
        let path = dir.join("secret.conf");

        write_atomic(&path, b"private").expect("atomic write should succeed");

        let mode = fs::metadata(&path)
            .expect("metadata should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_with_mode_uses_requested_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("custom-mode");
        let path = dir.join("unit.service");

        write_atomic_with_mode(&path, b"unit", 0o644).expect("atomic write should succeed");

        let mode = fs::metadata(&path)
            .expect("metadata should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
        let _ = fs::remove_dir_all(dir);
    }
}
