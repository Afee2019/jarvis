use crate::config::Config;
use anyhow::{Context, Result};
use chrono::Utc;
use std::future::Future;
use std::path::PathBuf;
use tokio::task::JoinHandle;
use tokio::time::Duration;

const STATUS_FLUSH_SECONDS: u64 = 5;

/// PID 文件路径：~/.jarvis/daemon.pid
pub fn pid_file_path(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("daemon.pid")
}

#[cfg(unix)]
#[allow(clippy::cast_possible_wrap)]
fn pid_to_native(pid: u32) -> libc::pid_t {
    pid as libc::pid_t
}

/// 检查 daemon 是否在运行，返回活跃 PID 或 None
pub fn is_daemon_running(config: &Config) -> Option<u32> {
    let path = pid_file_path(config);
    let content = std::fs::read_to_string(&path).ok()?;
    let pid: u32 = content.trim().parse().ok()?;

    // 检查进程是否存活（kill -0）
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid_to_native(pid), 0) };
        if result == 0 {
            Some(pid)
        } else {
            // 进程不存在，清理过期 PID 文件
            let _ = std::fs::remove_file(&path);
            None
        }
    }

    #[cfg(not(unix))]
    {
        // 非 Unix 平台：仅检查 PID 文件存在
        Some(pid)
    }
}

/// 停止运行中的 daemon（发送 SIGTERM）
pub fn stop_daemon(config: &Config) -> Result<()> {
    let pid_path = pid_file_path(config);
    let Some(pid) = is_daemon_running(config) else {
        println!("守护进程未运行");
        return Ok(());
    };

    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid_to_native(pid), libc::SIGTERM) };
        if result != 0 {
            anyhow::bail!(
                "发送 SIGTERM 到进程 {pid} 失败: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    #[cfg(not(unix))]
    {
        anyhow::bail!("停止守护进程仅支持 Unix 平台");
    }

    // 等待进程退出（最多 10 秒）
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if is_daemon_running(config).is_none() {
            let _ = std::fs::remove_file(&pid_path);
            println!("✅ 守护进程已停止（PID {pid}）");
            return Ok(());
        }
    }

    // 超时后尝试 SIGKILL
    #[cfg(unix)]
    {
        let _ = unsafe { libc::kill(pid_to_native(pid), libc::SIGKILL) };
    }
    let _ = std::fs::remove_file(&pid_path);
    println!("⚠️  守护进程已强制终止（PID {pid}）");
    Ok(())
}

/// 写入 PID 文件
fn write_pid_file(config: &Config) -> Result<()> {
    let path = pid_file_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, std::process::id().to_string())
        .with_context(|| format!("写入 PID 文件失败: {}", path.display()))
}

/// 清理 PID 文件
fn remove_pid_file(config: &Config) {
    let _ = std::fs::remove_file(pid_file_path(config));
}

pub async fn run(config: Config, host: String, port: u16) -> Result<()> {
    write_pid_file(&config)?;

    let initial_backoff = config.reliability.channel_initial_backoff_secs.max(1);
    let max_backoff = config
        .reliability
        .channel_max_backoff_secs
        .max(initial_backoff);

    crate::health::mark_component_ok("daemon");

    if config.heartbeat.enabled {
        let _ =
            crate::heartbeat::engine::HeartbeatEngine::ensure_heartbeat_file(&config.workspace_dir)
                .await;
    }

    let mut handles: Vec<JoinHandle<()>> = vec![spawn_state_writer(config.clone())];

    {
        let gateway_cfg = config.clone();
        let gateway_host = host.clone();
        handles.push(spawn_component_supervisor(
            "gateway",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = gateway_cfg.clone();
                let host = gateway_host.clone();
                async move { crate::gateway::run_gateway(&host, port, cfg).await }
            },
        ));
    }

    {
        if has_supervised_channels(&config) {
            let channels_cfg = config.clone();
            handles.push(spawn_component_supervisor(
                "channels",
                initial_backoff,
                max_backoff,
                move || {
                    let cfg = channels_cfg.clone();
                    async move { crate::channels::start_channels(cfg).await }
                },
            ));
        } else {
            crate::health::mark_component_ok("channels");
            tracing::info!("未配置实时通道；通道 supervisor 已禁用");
        }
    }

    if config.heartbeat.enabled {
        let heartbeat_cfg = config.clone();
        handles.push(spawn_component_supervisor(
            "heartbeat",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = heartbeat_cfg.clone();
                async move { run_heartbeat_worker(cfg).await }
            },
        ));
    }

    {
        let scheduler_cfg = config.clone();
        handles.push(spawn_component_supervisor(
            "scheduler",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = scheduler_cfg.clone();
                async move { crate::cron::scheduler::run(cfg).await }
            },
        ));
    }

    println!("🧠 Jarvis 守护进程已启动");
    println!("   Gateway：http://{host}:{port}");
    println!("   组件：gateway, channels, heartbeat, scheduler");
    println!("   按 Ctrl+C 停止");

    tokio::signal::ctrl_c().await?;
    crate::health::mark_component_error("daemon", "shutdown requested");

    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        let _ = handle.await;
    }

    remove_pid_file(&config);
    // 清理状态文件
    let _ = std::fs::remove_file(state_file_path(&config));

    Ok(())
}

pub fn state_file_path(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("daemon_state.json")
}

fn spawn_state_writer(config: Config) -> JoinHandle<()> {
    tokio::spawn(async move {
        let path = state_file_path(&config);
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let mut interval = tokio::time::interval(Duration::from_secs(STATUS_FLUSH_SECONDS));
        loop {
            interval.tick().await;
            let mut json = crate::health::snapshot_json();
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "written_at".into(),
                    serde_json::json!(Utc::now().to_rfc3339()),
                );
            }
            let data = serde_json::to_vec_pretty(&json).unwrap_or_else(|_| b"{}".to_vec());
            let _ = tokio::fs::write(&path, data).await;
        }
    })
}

fn spawn_component_supervisor<F, Fut>(
    name: &'static str,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
    mut run_component: F,
) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);

        loop {
            crate::health::mark_component_ok(name);
            match run_component().await {
                Ok(()) => {
                    crate::health::mark_component_error(name, "component exited unexpectedly");
                    tracing::warn!("守护进程组件「{name}」意外退出");
                    // Clean exit — reset backoff since the component ran successfully
                    backoff = initial_backoff_secs.max(1);
                }
                Err(e) => {
                    crate::health::mark_component_error(name, e.to_string());
                    tracing::error!("守护进程组件「{name}」失败：{e}");
                }
            }

            crate::health::bump_component_restart(name);
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            // Double backoff AFTER sleeping so first error uses initial_backoff
            backoff = backoff.saturating_mul(2).min(max_backoff);
        }
    })
}

async fn run_heartbeat_worker(config: Config) -> Result<()> {
    let observer: std::sync::Arc<dyn crate::observability::Observer> =
        std::sync::Arc::from(crate::observability::create_observer(&config.observability));
    let engine = crate::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
        observer,
    );

    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_mins) * 60));

    loop {
        interval.tick().await;

        let tasks = engine.collect_tasks().await?;
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = config.default_temperature;
            if let Err(e) = crate::agent::run(config.clone(), Some(prompt), None, None, temp).await
            {
                crate::health::mark_component_error("heartbeat", e.to_string());
                tracing::warn!("Heartbeat 任务失败：{e}");
            } else {
                crate::health::mark_component_ok("heartbeat");
            }
        }
    }
}

fn has_supervised_channels(config: &Config) -> bool {
    config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> Config {
        let config = Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        config
    }

    #[test]
    fn state_file_path_uses_config_directory() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let path = state_file_path(&config);
        assert_eq!(path, tmp.path().join("daemon_state.json"));
    }

    #[tokio::test]
    async fn supervisor_marks_error_and_restart_on_failure() {
        let handle = spawn_component_supervisor("daemon-test-fail", 1, 1, || async {
            anyhow::bail!("boom")
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;

        let snapshot = crate::health::snapshot_json();
        let component = &snapshot["components"]["daemon-test-fail"];
        assert_eq!(component["status"], "error");
        assert!(component["restart_count"].as_u64().unwrap_or(0) >= 1);
        assert!(component["last_error"]
            .as_str()
            .unwrap_or("")
            .contains("boom"));
    }

    #[tokio::test]
    async fn supervisor_marks_unexpected_exit_as_error() {
        let handle = spawn_component_supervisor("daemon-test-exit", 1, 1, || async { Ok(()) });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;

        let snapshot = crate::health::snapshot_json();
        let component = &snapshot["components"]["daemon-test-exit"];
        assert_eq!(component["status"], "error");
        assert!(component["restart_count"].as_u64().unwrap_or(0) >= 1);
        assert!(component["last_error"]
            .as_str()
            .unwrap_or("")
            .contains("component exited unexpectedly"));
    }

    #[test]
    fn pid_file_path_uses_config_directory() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let path = pid_file_path(&config);
        assert_eq!(path, tmp.path().join("daemon.pid"));
    }

    #[test]
    fn is_daemon_running_returns_none_when_no_pid_file() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        assert!(is_daemon_running(&config).is_none());
    }

    #[test]
    fn is_daemon_running_returns_none_for_stale_pid() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        // 写入一个不存在的 PID
        std::fs::write(pid_file_path(&config), "999999999").unwrap();
        assert!(is_daemon_running(&config).is_none());
        // 过期 PID 文件应被自动清理
        assert!(!pid_file_path(&config).exists());
    }

    #[test]
    fn write_and_remove_pid_file() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        write_pid_file(&config).unwrap();
        let path = pid_file_path(&config);
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, std::process::id().to_string());

        remove_pid_file(&config);
        assert!(!path.exists());
    }

    #[test]
    fn stop_daemon_noop_when_not_running() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        // 不应报错
        stop_daemon(&config).unwrap();
    }

    #[test]
    fn detects_no_supervised_channels() {
        let config = Config::default();
        assert!(!has_supervised_channels(&config));
    }

    #[test]
    fn detects_supervised_channels_present() {
        let mut config = Config::default();
        config.channels_config.telegram = Some(crate::config::TelegramConfig {
            bot_token: "token".into(),
            allowed_users: vec![],
        });
        assert!(has_supervised_channels(&config));
    }
}
