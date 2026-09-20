use easyjob_common::Result;
use tokio::process::Command;

pub struct CommandBuilder;

/// 按动作配置的编码解码子进程输出字节。
/// - Utf8：按 UTF-8 解，失败则 lossy 兜底
/// - Gbk：按 GBK（代码页 936）解
/// - None（旧数据 / 未显式配置）：Windows 下等同 Gbk，其他平台等同 Utf8
pub fn decode_output(
    bytes: &[u8],
    encoding: &Option<easyjob_domain::action::ScriptEncoding>,
) -> String {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Utf8) => decode_utf8(bytes),
        Some(ScriptEncoding::Gbk) => decode_gbk(bytes),
        None => default_decode(bytes),
    }
}

fn decode_utf8(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(bytes).to_string(),
    }
}

#[cfg(windows)]
fn default_decode(bytes: &[u8]) -> String {
    // 中文 Windows 控制台与多数命令行工具默认输出 GBK
    decode_gbk(bytes)
}

#[cfg(not(windows))]
fn default_decode(bytes: &[u8]) -> String {
    decode_utf8(bytes)
}

/// 按 GBK（代码页 936）解码。显式指定 936 而不是 CP_ACP，
/// 这样在非中文 Windows 上也能正确解出 GBK 脚本的输出。
#[cfg(windows)]
fn decode_gbk(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    const CP_GBK: u32 = 936;
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        // 先查询所需宽字符数
        let needed = MultiByteToWideChar(
            CP_GBK,
            0,
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
            CP_GBK,
            0,
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
fn decode_gbk(bytes: &[u8]) -> String {
    // 非 Windows 上没有 GBK 代码页，退回 UTF-8 解析
    String::from_utf8_lossy(bytes).to_string()
}

/// chcp 代码页前缀：让子进程输出编码与我们的解码方式一致。
/// 默认（未配置）与显式 GBK 都用 936；只有显式选 UTF-8 才切 65001。
#[cfg(windows)]
fn codepage_prefix(encoding: &Option<easyjob_domain::action::ScriptEncoding>) -> &'static str {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Utf8) => "chcp 65001 >nul & ",
        _ => "chcp 936 >nul & ",
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

/// PowerShell 内切换输出编码：默认（未配置）与显式 GBK 都用 936，
/// 只有显式选 UTF-8 才切到 utf8，保证与解码方式一致。
#[cfg(windows)]
fn chcp_powershell_prefix(encoding: &Option<easyjob_domain::action::ScriptEncoding>) -> &'static str {
    use easyjob_domain::action::ScriptEncoding;
    match encoding {
        Some(ScriptEncoding::Utf8) => "[Console]::OutputEncoding = [Text.Encoding]::utf8",
        _ => "[Console]::OutputEncoding = [Text.Encoding]::GetEncoding(936)",
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
