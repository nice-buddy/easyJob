use std::process::Command;

#[test]
fn test_help_lists_service_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_easyjob-agent"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--service"), "{help}");
    assert!(help.contains("--install-service"), "{help}");
    assert!(help.contains("--uninstall-service"), "{help}");
}
