use easyjob_common::Result;
use tokio::process::Command;

pub struct CommandBuilder;

/// 按动作配置的编码解码子进程输出字节。
/// - None / Utf8：先按 UTF-8 严格解，失败则 lossy 兜底
/// - Gbk（Windows）：用系统 API 按当前 ANSI 代码页（中文系统即 GBK）解码，失败则 lossy 兜底
pub fn decode_output(
    bytes: &[u8],
    encoding: &Option<easyjob_domain::action::ScriptEncoding>,
) -> String {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Gbk) => decode_ansi(bytes),
        _ => match String::from_utf8(bytes.to_vec()) {
            Ok(s) => s,
            Err(_) => String::from_utf8_lossy(bytes).to_string(),
        },
    }
}

#[cfg(windows)]
fn decode_ansi(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_ACP, MB_ERR_INVALID_CHARS};
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        // 先查询所需宽字符数
        let needed = MultiByteToWideChar(
            CP_ACP,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        let mut wide = vec![0u16; needed as usize];
        let written = MultiByteToWideChar(
            CP_ACP,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            needed,
        );
        if written <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        String::from_utf16_lossy(&wide[..written as usize])
    }
}

#[cfg(not(windows))]
fn decode_ansi(bytes: &[u8]) -> String {
    // 非 Windows 上没有 ANSI 代码页概念，直接按 UTF-8 解
    String::from_utf8_lossy(bytes).to_string()
}

/// chcp 代码页前缀：Windows 控制台默认 GBK（936），中文输出直接按 UTF-8 解会乱码。
/// 默认切到 UTF-8（65001）执行；用户显式选 GBK 时回切 936。
#[cfg(windows)]
fn codepage_prefix(encoding: &Option<easyjob_domain::action::ScriptEncoding>) -> &'static str {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Gbk) => "chcp 936 >nul & ",
        _ => "chcp 65001 >nul & ",
    }
}

impl CommandBuilder {
    pub fn new_shell(script: &str) -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/c", script]);
            crate::windows::configure_windows_command(cmd.as_std_mut());
            cmd
        }
        #[cfg(unix)]
        {
            let shell = if std::path::Path::new("/bin/zsh").exists() {
                "/bin/zsh"
            } else {
                "/bin/sh"
            };
            let mut cmd = Command::new(shell);
            cmd.args(["-c", script]);
            crate::unix::configure_unix_command(cmd.as_std_mut());
            cmd
        }
        #[cfg(not(any(unix, windows)))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        }
    }

    pub fn new_cmd(
        command: &str,
        encoding: &Option<easyjob_domain::action::ScriptEncoding>,
    ) -> Command {
        let mut cmd = Command::new("cmd.exe");
        #[cfg(windows)]
        {
            let full = format!("{}{}", codepage_prefix(encoding), command);
            cmd.args(["/c", &full]);
        }
        #[cfg(not(windows))]
        {
            let _ = encoding;
            cmd.args(["/c", command]);
        }
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }

    pub fn new_powershell(
        script: &str,
        no_profile: bool,
        encoding: &Option<easyjob_domain::action::ScriptEncoding>,
    ) -> Command {
        let mut cmd = Command::new("powershell.exe");
        if no_profile {
            cmd.arg("-NoProfile");
        }
        cmd.args([
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
        ]);
        #[cfg(windows)]
        {
            let full = format!("{}; {}", chcp_powershell_prefix(encoding), script);
            cmd.arg(full);
        }
        #[cfg(not(windows))]
        {
            let _ = encoding;
            cmd.arg(script);
        }
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }

    pub fn new_program(program: &str, args: &[String]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args);
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }
}

/// PowerShell 内切换控制台输出编码：默认 UTF-8，GBK 时切回默认 OEM 代码页。
#[cfg(windows)]
fn chcp_powershell_prefix(encoding: &Option<easyjob_domain::action::ScriptEncoding>) -> &'static str {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Gbk) => "[Console]::OutputEncoding = [Text.Encoding]::GetEncoding(936)",
        _ => "[Console]::OutputEncoding = [Text.Encoding]::utf8",
    }
}

pub async fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        crate::unix::kill_unix_process_tree(pid).await
    }
    #[cfg(windows)]
    {
        crate::windows::kill_windows_process_tree(pid).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(easyjob_common::Error::Process(
            "Unsupported platform for kill_process_tree".to_string(),
        ))
    }
}

/// Placeholder for platform-specific process metadata or handle if needed
pub struct PlatformProcess;
