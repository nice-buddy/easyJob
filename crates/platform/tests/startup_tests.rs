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
    assert!(plist.contains("<key>Umask</key>"));
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
        Path::new("/Library/Application Support/EasyJob"),
    );
    assert!(script.contains("bootstrap system "), "{script}");
    assert!(script.contains("chown root:wheel"), "{script}");
    assert!(script.contains("mkdir -p \"$DATA_DIR/logs\""), "{script}");
}

#[test]
fn test_windows_prepare_data_dir_grants_authenticated_users() {
    let (program, args) =
        windows_prepare_data_dir_elevated_ps(Path::new("C:\\Program Files\\easyJob\\data"));
    assert_eq!(program, "powershell.exe");
    let joined = args.join(" ");
    assert!(joined.contains("S-1-5-11"), "{joined}");
    assert!(joined.contains("icacls"), "{joined}");
}

#[test]
fn test_macos_prepare_data_dir_script_grants_staff() {
    let script = macos_prepare_data_dir_script(Path::new("/Library/Application Support/EasyJob"));
    assert!(script.contains("chown -R root:staff"), "{script}");
    assert!(script.contains("g+rwX"), "{script}");
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_plist_passes_plutil_lint() {
    let dir = tempfile::tempdir().unwrap();
    let plist_path = dir.path().join("com.easyjob.agent.plist");
    let plist = macos_daemon_plist(
        Path::new("/Applications/easyJob.app/Contents/Resources/easyjob-agent"),
        Path::new("/Library/Application Support/EasyJob"),
    );
    std::fs::write(&plist_path, plist).unwrap();
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-lint", plist_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "plutil lint failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_install_script_passes_sh_syntax_check() {
    let script = macos_install_script(
        "/Library/LaunchDaemons/com.easyjob.agent.plist",
        "PLIST",
        Path::new("/Library/Application Support/EasyJob"),
    );
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("install.sh");
    std::fs::write(&script_path, script).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args(["-n", script_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sh -n failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires a user launchd session; run with --ignored"]
fn test_macos_launchd_bootstraps_generated_plist() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let logs_dir = data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir).unwrap();

    let script_path = dir.path().join("fake-agent.sh");
    std::fs::write(
        &script_path,
        "#!/bin/sh\nDATA=''\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"--data-dir\" ]; then\n    shift\n    DATA=\"$1\"\n  fi\n  shift\ndone\necho launchd-ok > \"$DATA/launchd.out\"\nsleep 30\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    let label = format!("com.easyjob.agent.test.{}", std::process::id());
    let plist_path = dir.path().join(format!("{label}.plist"));
    std::fs::write(
        &plist_path,
        macos_daemon_plist_for_label(&label, &script_path, &data_dir),
    )
    .unwrap();

    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let plist_arg = plist_path.to_string_lossy().to_string();
    let bootstrap = std::process::Command::new("/bin/launchctl")
        .args(["bootstrap", &domain, &plist_arg])
        .output()
        .unwrap();
    assert!(
        bootstrap.status.success(),
        "launchctl bootstrap failed: {}{}",
        String::from_utf8_lossy(&bootstrap.stdout),
        String::from_utf8_lossy(&bootstrap.stderr)
    );

    struct BootoutGuard {
        domain: String,
        plist: String,
    }
    impl Drop for BootoutGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("/bin/launchctl")
                .args(["bootout", &self.domain, &self.plist])
                .output();
        }
    }
    let _guard = BootoutGuard {
        domain,
        plist: plist_arg,
    };

    let marker = data_dir.join("launchd.out");
    for _ in 0..50 {
        if marker.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(marker.exists(), "launchd did not start the generated plist");
    assert!(std::fs::read_to_string(&marker)
        .unwrap()
        .contains("launchd-ok"));
}
