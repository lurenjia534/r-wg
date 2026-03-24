use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::auth::{is_peer_allowed, PeerCredentials};
use super::entry::parse_linux_entry_command;
use super::fs_ops::cleanup_runtime_socket_dir;
use super::install_model::{
    InstallAuthMode, InstallOptions, LinuxEntryCommand, ManageCommand, RemoveOptions,
    ServiceOptions, DEFAULT_INSTALLED_BINARY, DEFAULT_SOCKET_GROUP, DEFAULT_SOCKET_UNIT_PATH,
    DEFAULT_STARTUP_REPAIR_UNIT_PATH, DEFAULT_UNIT_PATH,
};
use super::render::{
    load_existing_install_auth_mode, render_desktop_entry, render_service_unit,
    render_socket_unit, render_startup_repair_unit,
};

fn parse(args: &[&str]) -> LinuxEntryCommand {
    parse_linux_entry_command(args.iter().map(std::ffi::OsString::from))
        .expect("args should parse")
        .expect("linux entry command should be detected")
}

#[test]
fn parse_service_mode_accepts_defaults() {
    let LinuxEntryCommand::ServiceMode(ServiceOptions {
        socket_path,
        socket_group,
        allowed_uid,
    }) = parse(&["r-wg", "--linux-service"])
    else {
        panic!("expected service mode");
    };
    assert_eq!(socket_path, PathBuf::from("/run/r-wg/control.sock"));
    assert_eq!(socket_group, None);
    assert_eq!(allowed_uid, None);
}

#[test]
fn parse_service_mode_accepts_overrides() {
    let LinuxEntryCommand::ServiceMode(ServiceOptions {
        socket_path,
        socket_group,
        allowed_uid,
    }) = parse(&[
        "r-wg",
        "--linux-service",
        "--socket",
        "/tmp/r-wg.sock",
        "--socket-group",
        "vpnusers",
        "--allowed-uid",
        "1000",
    ])
    else {
        panic!("expected service mode");
    };
    assert_eq!(socket_path, PathBuf::from("/tmp/r-wg.sock"));
    assert_eq!(socket_group.as_deref(), Some("vpnusers"));
    assert_eq!(allowed_uid, Some(1000));
}

#[test]
fn parse_install_command_uses_defaults() {
    let LinuxEntryCommand::Manage(ManageCommand::Install(InstallOptions {
        source_path,
        binary_path,
        unit_path,
        socket_unit_path,
        startup_repair_unit_path,
        socket_group,
        socket_user,
        allowed_uid,
    })) = parse(&["r-wg", "service", "install", "--source", "/tmp/r-wg"])
    else {
        panic!("expected install command");
    };
    assert_eq!(source_path, PathBuf::from("/tmp/r-wg"));
    assert_eq!(binary_path, PathBuf::from(DEFAULT_INSTALLED_BINARY));
    assert_eq!(unit_path, PathBuf::from(DEFAULT_UNIT_PATH));
    assert_eq!(socket_unit_path, PathBuf::from(DEFAULT_SOCKET_UNIT_PATH));
    assert_eq!(
        startup_repair_unit_path,
        PathBuf::from(DEFAULT_STARTUP_REPAIR_UNIT_PATH)
    );
    assert_eq!(socket_group.as_deref(), Some(DEFAULT_SOCKET_GROUP));
    assert_eq!(socket_user, None);
    assert_eq!(allowed_uid, None);
}

#[test]
fn parse_remove_command_uses_defaults() {
    let LinuxEntryCommand::Manage(ManageCommand::Remove(RemoveOptions {
        binary_path,
        unit_path,
        socket_unit_path,
        startup_repair_unit_path,
    })) = parse(&["r-wg", "service", "remove"])
    else {
        panic!("expected remove command");
    };
    assert_eq!(binary_path, PathBuf::from(DEFAULT_INSTALLED_BINARY));
    assert_eq!(unit_path, PathBuf::from(DEFAULT_UNIT_PATH));
    assert_eq!(socket_unit_path, PathBuf::from(DEFAULT_SOCKET_UNIT_PATH));
    assert_eq!(
        startup_repair_unit_path,
        PathBuf::from(DEFAULT_STARTUP_REPAIR_UNIT_PATH)
    );
}

#[test]
fn render_service_unit_uses_binary_and_group() {
    let unit = render_service_unit(Path::new("/opt/r-wg/r-wg"), Some("vpnusers"), Some(1000));
    assert!(unit.contains(
        "ExecStart=/opt/r-wg/r-wg --linux-service --socket-group vpnusers --allowed-uid 1000"
    ));
    assert!(unit.contains("StateDirectory=r-wg"));
    assert!(!unit.contains("RuntimeDirectory="));
    assert!(unit.contains("WantedBy=multi-user.target"));
}

#[test]
fn render_socket_unit_uses_group_and_socket_target() {
    let unit = render_socket_unit(None, Some("vpnusers"));
    assert!(unit.contains("DirectoryMode=0711"));
    assert!(unit.contains("SocketGroup=vpnusers"));
    assert!(unit.contains("WantedBy=sockets.target"));
}

#[test]
fn render_socket_unit_uses_socket_user_when_present() {
    let unit = render_socket_unit(Some("luren"), None);
    assert!(unit.contains("DirectoryMode=0711"));
    assert!(unit.contains("SocketUser=luren"));
    assert!(unit.contains("SocketMode=0600"));
}

#[test]
fn cleanup_runtime_socket_dir_removes_existing_parent_dir() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let parent = std::env::temp_dir().join(format!("r-wg-runtime-{unique}"));
    let socket_path = parent.join("control.sock");
    std::fs::create_dir_all(&parent).expect("runtime dir should be created");
    std::fs::write(&socket_path, b"").expect("socket placeholder should be created");

    cleanup_runtime_socket_dir(&socket_path).expect("runtime dir cleanup should succeed");

    assert!(!parent.exists());
}

#[test]
fn is_peer_allowed_requires_uid_match_when_only_uid_is_configured() {
    let peer = PeerCredentials {
        pid: 1234,
        uid: 2000,
    };
    assert!(!is_peer_allowed(peer, Some(1000), None));
}

#[test]
fn is_peer_allowed_defaults_to_open_only_when_no_restrictions_exist() {
    let peer = PeerCredentials {
        pid: 1234,
        uid: 2000,
    };
    assert!(is_peer_allowed(peer, None, None));
}

#[test]
fn load_existing_install_auth_mode_preserves_group_model() {
    let dir = std::env::temp_dir().join(format!("r-wg-auth-mode-group-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");
    let service = dir.join("r-wg.service");
    let socket = dir.join("r-wg.socket");
    std::fs::write(
        &service,
        render_service_unit(Path::new("/opt/r-wg/r-wg"), Some("vpnusers"), None),
    )
    .expect("service unit should write");
    std::fs::write(&socket, render_socket_unit(None, Some("vpnusers")))
        .expect("socket unit should write");

    let mode = load_existing_install_auth_mode(&service, &socket)
        .expect("mode should load")
        .expect("mode should exist");
    assert_eq!(
        mode,
        InstallAuthMode {
            socket_group: Some("vpnusers".to_string()),
            socket_user: None,
            allowed_uid: None,
        }
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_existing_install_auth_mode_preserves_single_user_model() {
    let dir = std::env::temp_dir().join(format!("r-wg-auth-mode-user-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");
    let service = dir.join("r-wg.service");
    let socket = dir.join("r-wg.socket");
    std::fs::write(
        &service,
        render_service_unit(Path::new("/opt/r-wg/r-wg"), None, Some(1000)),
    )
    .expect("service unit should write");
    std::fs::write(&socket, render_socket_unit(Some("luren"), None))
        .expect("socket unit should write");

    let mode = load_existing_install_auth_mode(&service, &socket)
        .expect("mode should load")
        .expect("mode should exist");
    assert_eq!(
        mode,
        InstallAuthMode {
            socket_group: None,
            socket_user: Some("luren".to_string()),
            allowed_uid: Some(1000),
        }
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_startup_repair_unit_targets_recovery_journal() {
    let unit = render_startup_repair_unit(Path::new("/opt/r-wg/r-wg"));
    assert!(unit.contains("ExecStart=/opt/r-wg/r-wg service startup-repair"));
    assert!(unit.contains("ConditionPathExists=/var/lib/r-wg/recovery.json"));
    assert!(unit.contains("StateDirectory=r-wg"));
}

#[test]
fn render_desktop_entry_targets_installed_binary() {
    let entry = render_desktop_entry(Path::new("/usr/local/libexec/r-wg/r-wg"));
    assert!(entry.contains("Exec=/usr/local/libexec/r-wg/r-wg"));
    assert!(entry.contains("Icon=r-wg"));
    assert!(entry.contains("StartupWMClass=r-wg"));
}
