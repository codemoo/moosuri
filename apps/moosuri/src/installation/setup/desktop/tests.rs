use super::*;
#[test]
fn loaded_or_autostart_enabled_never_proves_server_readiness() {
    assert_eq!(classify(false, false, true), Phase::Stopped);
    assert_eq!(classify(true, false, true), Phase::Error);
    assert_eq!(classify(true, true, true), Phase::Ready);
    assert_eq!(classify(false, false, false), Phase::Error);
}
#[test]
fn desktop_and_control_locks_are_independent_and_single_instance() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(
        temp.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let desktop = named_lock(temp.path(), "desktop.lock").unwrap();
    let control = control_lock(temp.path()).unwrap();
    assert!(named_lock(temp.path(), "desktop.lock").is_err());
    assert!(control_lock(temp.path()).is_err());
    drop(desktop);
    assert!(named_lock(temp.path(), "desktop.lock").is_ok());
    drop(control);
}

#[test]
fn browser_dispatch_returns_while_the_viewer_is_still_running() {
    let start = Instant::now();
    assert!(dispatch_browser(
        Process::new("/bin/sh").args(["-c", "sleep 2"])
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn embedded_product_icon_decodes_to_the_tray_dimensions() {
    assert_eq!(icon_rgba().len(), 24 * 24 * 4);
}
