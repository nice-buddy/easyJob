use easyjob_common::Result;

/// 发送系统原生桌面横幅通知（macOS / Windows）
pub async fn send_system_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let escaped_title = title.replace('\\', "\\\\").replace('\"', "\\\"");
        let escaped_body = body.replace('\\', "\\\\").replace('\"', "\\\"");

        let script = if let Some(sub) = subtitle {
            let escaped_sub = sub.replace('\\', "\\\\").replace('\"', "\\\"");
            format!(
                "display notification \"{}\" with title \"{}\" subtitle \"{}\" sound name \"default\"",
                escaped_body, escaped_title, escaped_sub
            )
        } else {
            format!(
                "display notification \"{}\" with title \"{}\" sound name \"default\"",
                escaped_body, escaped_title
            )
        };

        let mut cmd = tokio::process::Command::new("osascript");
        cmd.arg("-e").arg(script);

        match tokio::time::timeout(std::time::Duration::from_secs(3), cmd.output()).await {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!("osascript notification failed: {}", err);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to execute osascript: {}", e);
            }
            Err(_) => {
                tracing::warn!("osascript notification timed out after 3s");
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        let header = if let Some(sub) = subtitle {
            format!("{} - {}", title, sub)
        } else {
            title.to_string()
        };

        let safe_header = header.replace('\'', "''");
        let safe_body = body.replace('\'', "''");

        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $xml = [xml]$template.GetXml(); \
             $nodes = $xml.GetElementsByTagName('text'); \
             $nodes[0].AppendChild($xml.CreateTextNode('{safe_header}')) > $null; \
             $nodes[1].AppendChild($xml.CreateTextNode('{safe_body}')) > $null; \
             $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('easyJob').Show($toast);"
        );

        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ]);
        crate::windows::configure_windows_command(cmd.as_std_mut());

        match tokio::time::timeout(std::time::Duration::from_secs(3), cmd.output()).await {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!("PowerShell Toast notification failed: {}", err);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to execute powershell toast: {}", e);
            }
            Err(_) => {
                tracing::warn!("PowerShell toast notification timed out after 3s");
            }
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (title, subtitle, body);
        tracing::debug!("System notification not supported on this platform");
        Ok(())
    }
}
