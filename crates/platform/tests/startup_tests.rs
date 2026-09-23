use easyjob_platform::startup::*;
use std::path::Path;

#[test]
fn test_service_image_path_contains_service_and_data_dir() {
    let img = windows_service_image_path(
        Path::new(r"C:\Program Files\easyJob\easyjob-agent.exe"),
        Path::new(r"C:\Program Files\easyJob\data"),
    );
    assert!(img.contains("--service"), "{img}");
    assert!(img.contains(r"C:\Program Files\easyJob\data"), "{img}");
}

#[test]
fn test_parse_sc_query_running_and_missing() {
    assert_eq!(
        parse_sc_query_state("STATE              : 4  RUNNING"),
        WindowsServiceState::Running
    );
    assert_eq!(
        parse_sc_query_state("[SC] OpenService FAILED 1060"),
        WindowsServiceState::NotInstalled
    );
}

#[test]
fn test_macos_plist_has_run_at_load_and_keep_alive() {
    let plist = macos_daemon_plist(
        Path::new("/Applications/easyJob.app/Contents/Resources/easyjob-agent"),
        Path::new("/Library/Application Support/EasyJob"),
    );
    assert!(plist.contains("com.easyjob.agent"));
    assert!(plist.contains("<key>RunAtLoad</key>"));
    assert!(plist.contains("<key>KeepAlive</key>"));
    assert!(plist.contains("--data-dir"));
}

#[test]
fn test_ps_arg_quoting() {
    assert_eq!(quote_ps_arg(r"C:\a'b"), r"'C:\a''b'");
}

#[test]
fn test_system_data_dir_macos_fixed() {
    #[cfg(target_os = "macos")]
    {
        assert_eq!(
            system_data_dir(None),
            Path::new("/Library/Application Support/EasyJob")
        );
    }
}

#[test]
fn test_system_data_dir_windows_uses_exe_dir() {
    #[cfg(target_os = "windows")]
    {
        assert_eq!(
            system_data_dir(Some(Path::new("C:\\Program Files\\easyJob"))),
            Path::new("C:\\Program Files\\easyJob\\data")
        );
    }
}

#[test]
fn test_parse_sc_query_stopped() {
    assert_eq!(
        parse_sc_query_state("STATE              : 1  STOPPED"),
        WindowsServiceState::Stopped
    );
}

#[test]
fn test_macos_install_script_bootstraps() {
    let script = macos_install_script(
        "/Library/LaunchDaemons/com.easyjob.agent.plist",
        "PLIST",
    );
    assert!(script.contains("bootstrap system/"), "{script}");
    assert!(script.contains("chown root:wheel"), "{script}");
}
