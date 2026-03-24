use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

use super::super::EngineError;
use super::install_model::{
    DESKTOP_ICON_PNG, DESKTOP_ICON_SVG, DEFAULT_DESKTOP_ENTRY_PATH, DEFAULT_ICON_PNG_PATH,
    DEFAULT_ICON_SVG_PATH,
};
use super::remote_error;
use super::render::render_desktop_entry;

pub(super) fn remove_stale_socket(path: &Path) -> Result<(), EngineError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_socket() {
        fs::remove_file(path)
            .map_err(|err| remote_error(format!("failed to remove stale socket: {err}")))?;
        return Ok(());
    }
    Err(remote_error(format!(
        "refusing to replace non-socket path at {}",
        path.display()
    )))
}

pub(super) fn configure_socket_permissions(
    path: &Path,
    socket_gid: Option<u32>,
) -> Result<(), EngineError> {
    if let Some(gid) = socket_gid {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| remote_error("socket path contains interior NUL".to_string()))?;
        let rc = unsafe { libc::chown(c_path.as_ptr(), u32::MAX, gid) };
        if rc != 0 {
            return Err(remote_error(format!(
                "failed to change socket group: {}",
                io::Error::last_os_error()
            )));
        }
    }

    fs::set_permissions(path, fs::Permissions::from_mode(0o660))
        .map_err(|err| remote_error(format!("failed to chmod control socket: {err}")))
}

pub(super) fn lookup_group_gid(group: &str) -> Result<u32, EngineError> {
    let group_c = CString::new(group)
        .map_err(|_| remote_error("socket group contains interior NUL".to_string()))?;
    let mut grp = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0u8; 1024];

    loop {
        let rc = unsafe {
            libc::getgrnam_r(
                group_c.as_ptr(),
                grp.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return Err(remote_error(format!("socket group not found: {group}")));
            }
            let group = unsafe { grp.assume_init() };
            return Ok(group.gr_gid);
        }
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return Err(remote_error(format!(
            "failed to resolve socket group {group}: {}",
            io::Error::from_raw_os_error(rc)
        )));
    }
}

pub(super) fn ensure_group_exists(group: &str) -> Result<(), EngineError> {
    if lookup_group_gid(group).is_ok() {
        return Ok(());
    }
    super::systemd::run_command("groupadd", ["--system", group])
}

pub(super) fn install_binary(source_path: &Path, binary_path: &Path) -> Result<(), EngineError> {
    let install_dir = binary_path
        .parent()
        .ok_or_else(|| remote_error("binary install path has no parent".to_string()))?;
    fs::create_dir_all(install_dir).map_err(|err| {
        remote_error(format!(
            "failed to create install dir {}: {err}",
            install_dir.display()
        ))
    })?;
    fs::set_permissions(install_dir, fs::Permissions::from_mode(0o755)).map_err(|err| {
        remote_error(format!(
            "failed to chmod install dir {}: {err}",
            install_dir.display()
        ))
    })?;

    let temp_path = install_dir.join(".r-wg.install.tmp");
    fs::copy(source_path, &temp_path).map_err(|err| {
        remote_error(format!(
            "failed to copy {} -> {}: {err}",
            source_path.display(),
            temp_path.display()
        ))
    })?;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))
        .map_err(|err| remote_error(format!("failed to chmod installed binary: {err}")))?;
    fs::rename(&temp_path, binary_path).map_err(|err| {
        remote_error(format!(
            "failed to place installed binary {}: {err}",
            binary_path.display()
        ))
    })
}

pub(super) fn write_service_unit(unit_path: &Path, contents: String) -> Result<(), EngineError> {
    write_root_owned_file(unit_path, contents.as_bytes(), 0o644, "service unit")
}

pub(super) fn write_root_owned_file(
    path: &Path,
    contents: &[u8],
    mode: u32,
    label: &str,
) -> Result<(), EngineError> {
    let parent = path
        .parent()
        .ok_or_else(|| remote_error(format!("{label} path has no parent")))?;
    fs::create_dir_all(parent).map_err(|err| {
        remote_error(format!(
            "failed to create {label} dir {}: {err}",
            parent.display()
        ))
    })?;

    let temp_path = parent.join(".r-wg.install.tmp");
    fs::write(&temp_path, contents)
        .map_err(|err| remote_error(format!("failed to write temp {label}: {err}")))?;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(mode))
        .map_err(|err| remote_error(format!("failed to chmod {label}: {err}")))?;
    fs::rename(&temp_path, path)
        .map_err(|err| remote_error(format!("failed to place {label} {}: {err}", path.display())))
}

pub(super) fn remove_file_if_exists(path: &Path, label: &str) -> Result<(), EngineError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(remote_error(format!(
            "failed to remove {label} {}: {err}",
            path.display()
        ))),
    }
}

pub(super) fn install_desktop_integration(binary_path: &Path) -> Result<(), EngineError> {
    write_root_owned_file(
        Path::new(DEFAULT_DESKTOP_ENTRY_PATH),
        render_desktop_entry(binary_path).as_bytes(),
        0o644,
        "desktop entry",
    )?;
    write_root_owned_file(
        Path::new(DEFAULT_ICON_SVG_PATH),
        DESKTOP_ICON_SVG,
        0o644,
        "icon",
    )?;
    write_root_owned_file(
        Path::new(DEFAULT_ICON_PNG_PATH),
        DESKTOP_ICON_PNG,
        0o644,
        "icon",
    )?;
    refresh_desktop_caches();
    Ok(())
}

pub(super) fn remove_desktop_integration() -> Result<(), EngineError> {
    remove_file_if_exists(Path::new(DEFAULT_DESKTOP_ENTRY_PATH), "desktop entry")?;
    remove_file_if_exists(Path::new(DEFAULT_ICON_SVG_PATH), "icon")?;
    remove_file_if_exists(Path::new(DEFAULT_ICON_PNG_PATH), "icon")?;
    refresh_desktop_caches();
    Ok(())
}

fn refresh_desktop_caches() {
    let _ = Command::new("update-desktop-database")
        .arg("/usr/share/applications")
        .status();
    let _ = Command::new("gtk-update-icon-cache")
        .args(["-q", "-t", "/usr/share/icons/hicolor"])
        .status();
}

pub(super) fn cleanup_runtime_socket_dir(socket_path: &Path) -> Result<(), EngineError> {
    let Some(parent) = socket_path.parent() else {
        return Ok(());
    };

    match fs::remove_dir_all(parent) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(remote_error(format!(
            "failed to clean runtime socket dir {}: {err}",
            parent.display()
        ))),
    }
}
