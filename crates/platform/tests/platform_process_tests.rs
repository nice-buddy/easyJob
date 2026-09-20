use easyjob_platform::process::CommandBuilder;

#[tokio::test]
async fn test_spawn_and_capture_echo() {
    let mut cmd = CommandBuilder::new_shell("echo 'hello easyJob'");
    let output = cmd.output().await.expect("failed to execute shell command");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello easyJob"));
}

#[tokio::test]
async fn test_spawn_program() {
    #[cfg(unix)]
    {
        let mut cmd = CommandBuilder::new_program("echo", &[String::from("hello from program")]);
        let output = cmd.output().await.expect("failed to execute program");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("hello from program"));
    }
}

#[test]
fn test_command_builder_new_cmd_and_powershell() {
    let _cmd = CommandBuilder::new_cmd("echo hello", &None);
    let _ps = CommandBuilder::new_powershell("Write-Output hello", true, &None);
}

#[tokio::test]
async fn test_kill_process_tree_invalid_pid() {
    let res0 = easyjob_platform::kill_process_tree(0).await;
    assert!(res0.is_err());

    #[cfg(unix)]
    {
        let res1 = easyjob_platform::kill_process_tree(1).await;
        assert!(res1.is_err());

        let res_overflow = easyjob_platform::kill_process_tree(u32::MAX).await;
        assert!(res_overflow.is_err());
    }
}

#[tokio::test]
async fn test_kill_process_tree() {
    #[cfg(unix)]
    let shell_cmd = "sleep 30";
    #[cfg(windows)]
    let shell_cmd = "ping -n 30 127.0.0.1 >nul";

    let mut cmd = CommandBuilder::new_shell(shell_cmd);
    let mut child = cmd.spawn().expect("failed to spawn child");
    let pid = child.id().expect("child should have a pid");

    let kill_result = easyjob_platform::kill_process_tree(pid).await;
    assert!(kill_result.is_ok());

    let status = child.wait().await.expect("child wait should succeed");
    assert!(!status.success());
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_process_group_leader() {
    let mut cmd = CommandBuilder::new_shell("sleep 5");
    let mut child = cmd.spawn().expect("failed to spawn child");
    let pid = child.id().expect("child should have a pid");

    // Give it a few ms to execute pre_exec and launch
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let pgid = unsafe { libc::getpgid(pid as i32) };
    assert_eq!(
        pgid, pid as i32,
        "Child process should be its own process group leader"
    );

    let _ = easyjob_platform::kill_process_tree(pid).await;
    let _ = child.wait().await;
}
