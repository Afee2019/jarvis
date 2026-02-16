use crate::config::Config;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SERVICE_LABEL: &str = "com.jarvis.daemon";

pub fn handle_command(command: &crate::ServiceCommands, config: &Config) -> Result<()> {
    match command {
        crate::ServiceCommands::Install => install(config),
        crate::ServiceCommands::Start => start(config),
        crate::ServiceCommands::Stop => stop(config),
        crate::ServiceCommands::Status => status(config),
        crate::ServiceCommands::Uninstall => uninstall(config),
    }
}

fn install(config: &Config) -> Result<()> {
    if cfg!(target_os = "macos") {
        install_macos(config)
    } else if cfg!(target_os = "linux") {
        install_linux(config)
    } else {
        anyhow::bail!("服务管理仅支持 macOS 和 Linux");
    }
}

fn start(config: &Config) -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist = macos_service_file()?;
        run_checked(Command::new("launchctl").arg("load").arg("-w").arg(&plist))?;
        run_checked(Command::new("launchctl").arg("start").arg(SERVICE_LABEL))?;
        println!("✅ 服务已启动");
        Ok(())
    } else if cfg!(target_os = "linux") {
        run_checked(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        run_checked(Command::new("systemctl").args(["--user", "start", "jarvis.service"]))?;
        println!("✅ 服务已启动");
        Ok(())
    } else {
        let _ = config;
        anyhow::bail!("服务管理仅支持 macOS 和 Linux")
    }
}

fn stop(config: &Config) -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist = macos_service_file()?;
        let _ = run_checked(Command::new("launchctl").arg("stop").arg(SERVICE_LABEL));
        let _ = run_checked(
            Command::new("launchctl")
                .arg("unload")
                .arg("-w")
                .arg(&plist),
        );
        println!("✅ 服务已停止");
        Ok(())
    } else if cfg!(target_os = "linux") {
        let _ = run_checked(Command::new("systemctl").args(["--user", "stop", "jarvis.service"]));
        println!("✅ 服务已停止");
        Ok(())
    } else {
        let _ = config;
        anyhow::bail!("服务管理仅支持 macOS 和 Linux")
    }
}

fn status(config: &Config) -> Result<()> {
    if cfg!(target_os = "macos") {
        let out = run_capture(Command::new("launchctl").arg("list"))?;
        let running = out.lines().any(|line| line.contains(SERVICE_LABEL));
        println!(
            "服务: {}",
            if running {
                "✅ 运行中/已加载"
            } else {
                "❌ 未加载"
            }
        );
        println!("单元文件: {}", macos_service_file()?.display());
        return Ok(());
    }

    if cfg!(target_os = "linux") {
        let out =
            run_capture(Command::new("systemctl").args(["--user", "is-active", "jarvis.service"]))
                .unwrap_or_else(|_| "unknown".into());
        println!("服务状态: {}", out.trim());
        println!("单元文件: {}", linux_service_file(config)?.display());
        return Ok(());
    }

    anyhow::bail!("服务管理仅支持 macOS 和 Linux")
}

fn uninstall(config: &Config) -> Result<()> {
    stop(config)?;

    if cfg!(target_os = "macos") {
        let file = macos_service_file()?;
        if file.exists() {
            fs::remove_file(&file).with_context(|| format!("删除失败 {}", file.display()))?;
        }
        println!("✅ 服务已卸载 ({})", file.display());
        return Ok(());
    }

    if cfg!(target_os = "linux") {
        let file = linux_service_file(config)?;
        if file.exists() {
            fs::remove_file(&file).with_context(|| format!("删除失败 {}", file.display()))?;
        }
        let _ = run_checked(Command::new("systemctl").args(["--user", "daemon-reload"]));
        println!("✅ 服务已卸载 ({})", file.display());
        return Ok(());
    }

    anyhow::bail!("服务管理仅支持 macOS 和 Linux")
}

fn install_macos(config: &Config) -> Result<()> {
    let file = macos_service_file()?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().context("解析当前可执行文件路径失败")?;
    let logs_dir = config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("logs");
    fs::create_dir_all(&logs_dir)?;

    let stdout = logs_dir.join("daemon.stdout.log");
    let stderr = logs_dir.join("daemon.stderr.log");

    let plist = format!(
        r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>daemon</string>
    <string>--foreground</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
        label = SERVICE_LABEL,
        exe = xml_escape(&exe.display().to_string()),
        stdout = xml_escape(&stdout.display().to_string()),
        stderr = xml_escape(&stderr.display().to_string())
    );

    fs::write(&file, plist)?;
    println!("✅ 已安装 launchd 服务: {}", file.display());
    println!("   启动命令: jarvis service start");
    Ok(())
}

fn install_linux(config: &Config) -> Result<()> {
    let file = linux_service_file(config)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().context("解析当前可执行文件路径失败")?;
    let unit = format!(
        "[Unit]\nDescription=Jarvis daemon\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={} daemon --foreground\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    );

    fs::write(&file, unit)?;
    let _ = run_checked(Command::new("systemctl").args(["--user", "daemon-reload"]));
    let _ = run_checked(Command::new("systemctl").args(["--user", "enable", "jarvis.service"]));
    println!("✅ 已安装 systemd 用户服务: {}", file.display());
    println!("   启动命令: jarvis service start");
    Ok(())
}

fn macos_service_file() -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("无法找到用户主目录")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist")))
}

fn linux_service_file(config: &Config) -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("无法找到用户主目录")?;
    let _ = config;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("jarvis.service"))
}

fn run_checked(command: &mut Command) -> Result<()> {
    let output = command.output().context("启动命令失败")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("命令执行失败: {}", stderr.trim());
    }
    Ok(())
}

fn run_capture(command: &mut Command) -> Result<String> {
    let output = command.output().context("启动命令失败")?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).to_string();
    }
    Ok(text)
}

fn xml_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_escapes_reserved_chars() {
        let escaped = xml_escape("<&>\"' and text");
        assert_eq!(escaped, "&lt;&amp;&gt;&quot;&apos; and text");
    }

    #[test]
    fn run_capture_reads_stdout() {
        let out = run_capture(Command::new("sh").args(["-lc", "echo hello"]))
            .expect("stdout capture should succeed");
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn run_capture_falls_back_to_stderr() {
        let out = run_capture(Command::new("sh").args(["-lc", "echo warn 1>&2"]))
            .expect("stderr capture should succeed");
        assert_eq!(out.trim(), "warn");
    }

    #[test]
    fn run_checked_errors_on_non_zero_status() {
        let err = run_checked(Command::new("sh").args(["-lc", "exit 17"]))
            .expect_err("non-zero exit should error");
        assert!(err.to_string().contains("命令执行失败"));
    }

    #[test]
    fn linux_service_file_has_expected_suffix() {
        let file = linux_service_file(&Config::default()).unwrap();
        let path = file.to_string_lossy();
        assert!(path.ends_with(".config/systemd/user/jarvis.service"));
    }
}
