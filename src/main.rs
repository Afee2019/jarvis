#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    dead_code
)]

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::FmtSubscriber;

mod agent;
mod channels;
mod config;
mod cron;
mod daemon;
mod doctor;
mod gateway;
mod health;
mod heartbeat;
mod integrations;
mod memory;
mod migration;
mod observability;
mod onboard;
mod providers;
mod runtime;
mod security;
mod service;
mod skillforge;
mod skills;
mod tools;
mod tui;
mod tunnel;
mod util;

use config::Config;

/// `Jarvis` - 你的 AI，你做主。
#[derive(Parser, Debug)]
#[command(name = "jarvis")]
#[command(author = "Afee2019")]
#[command(version = "0.1.0")]
#[command(about = "最快、最轻量的 AI 助手。", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum ServiceCommands {
    /// 安装守护进程服务单元，支持自动启动和重启
    Install,
    /// 启动守护进程服务
    Start,
    /// 停止守护进程服务
    Stop,
    /// 查看守护进程服务状态
    Status,
    /// 卸载守护进程服务单元
    Uninstall,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 初始化工作区和配置
    Onboard {
        /// 运行完整的交互式向导（默认为快速设置）
        #[arg(long)]
        interactive: bool,

        /// 仅重新配置通道（快速修复流程）
        #[arg(long)]
        channels_only: bool,

        /// API 密钥（快速模式下使用，--interactive 时忽略）
        #[arg(long)]
        api_key: Option<String>,

        /// Provider 名称（快速模式下使用，默认：openrouter）
        #[arg(long)]
        provider: Option<String>,

        /// 记忆后端（sqlite、markdown、none）- 快速模式下使用，默认：sqlite
        #[arg(long)]
        memory: Option<String>,
    },

    /// 启动 AI agent 循环
    Agent {
        /// 单消息模式（不进入交互模式）
        #[arg(short, long, conflicts_with = "tui")]
        message: Option<String>,

        /// 使用的 Provider（openrouter、anthropic、openai）
        #[arg(short, long)]
        provider: Option<String>,

        /// 使用的模型
        #[arg(long)]
        model: Option<String>,

        /// 温度参数（0.0 - 2.0）
        #[arg(short, long, default_value = "0.7")]
        temperature: f64,

        /// 启动终端用户界面
        #[arg(long)]
        tui: bool,
    },

    /// 启动终端用户界面（`agent --tui` 的快捷方式）
    Tui {
        /// 使用的 Provider（openrouter、anthropic、openai）
        #[arg(short, long)]
        provider: Option<String>,

        /// 使用的模型
        #[arg(long)]
        model: Option<String>,

        /// 温度参数（0.0 - 2.0）
        #[arg(short, long, default_value = "0.7")]
        temperature: f64,
    },

    /// 启动 Gateway 服务器（webhooks、websockets）
    Gateway {
        /// 监听端口（使用 0 表示随机可用端口）
        #[arg(short, long, default_value = "8299")]
        port: u16,

        /// 绑定主机地址
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// 启动长期运行的自主运行时（gateway + 通道 + 心跳 + 调度器）
    Daemon {
        /// 监听端口（使用 0 表示随机可用端口）
        #[arg(short, long, default_value = "8299")]
        port: u16,

        /// 绑定主机地址
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// 前台运行（不后台化，供 service/调试用）
        #[arg(long)]
        foreground: bool,

        /// 停止正在运行的守护进程
        #[arg(long)]
        stop: bool,
    },

    /// 管理操作系统服务生命周期（launchd/systemd 用户服务）
    Service {
        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// 运行诊断检查（守护进程/调度器/通道健康状态）
    Doctor,

    /// 显示系统状态（完整详情）
    Status,

    /// 配置和管理定时任务
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// 管理通道（telegram、discord、slack）
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// 浏览 50+ 集成
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// 管理技能（用户自定义能力）
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    /// 从其他 Agent 运行时迁移数据
    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },
}

#[derive(Subcommand, Debug)]
enum MigrateCommands {
    /// 从 `OpenClaw` 工作区导入记忆到当前 `Jarvis` 工作区
    Openclaw {
        /// `OpenClaw` 工作区路径（可选，默认 ~/.openclaw/workspace）
        #[arg(long)]
        source: Option<std::path::PathBuf>,

        /// 仅验证和预览迁移，不写入任何数据
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CronCommands {
    /// 列出所有定时任务
    List,
    /// 添加新的定时任务
    Add {
        /// Cron 表达式
        expression: String,
        /// 要执行的命令
        command: String,
    },
    /// 移除定时任务
    Remove {
        /// 任务 ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ChannelCommands {
    /// 列出已配置的通道
    List,
    /// 启动所有已配置的通道（Telegram、Discord、Slack）
    Start,
    /// 运行已配置通道的健康检查
    Doctor,
    /// 添加新通道
    Add {
        /// 通道类型
        channel_type: String,
        /// 配置 JSON
        config: String,
    },
    /// 移除通道
    Remove {
        /// 通道名称
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum SkillCommands {
    /// 列出已安装的技能
    List,
    /// 从 GitHub URL 或本地路径安装技能
    Install {
        /// GitHub URL 或本地路径
        source: String,
    },
    /// 移除已安装的技能
    Remove {
        /// 技能名称
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum IntegrationCommands {
    /// 显示指定集成的详细信息
    Info {
        /// 集成名称
        name: String,
    },
}

struct CompactTimer;

impl FormatTime for CompactTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.format("%Y%m%d %H:%M:%S"))
    }
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_timer(CompactTimer)
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Onboard runs quick setup by default, or the interactive wizard with --interactive
    if let Commands::Onboard {
        interactive,
        channels_only,
        api_key,
        provider,
        memory,
    } = &cli.command
    {
        if *interactive && *channels_only {
            bail!("请使用 --interactive 或 --channels-only 其中之一，不能同时使用");
        }
        if *channels_only && (api_key.is_some() || provider.is_some() || memory.is_some()) {
            bail!("--channels-only 不接受 --api-key、--provider 或 --memory 参数");
        }

        let config = if *channels_only {
            onboard::run_channels_repair_wizard()?
        } else if *interactive {
            onboard::run_wizard()?
        } else {
            onboard::run_quick_setup(api_key.as_deref(), provider.as_deref(), memory.as_deref())?
        };
        // Auto-start channels if user said yes during wizard
        if std::env::var("JARVIS_AUTOSTART_CHANNELS").as_deref() == Ok("1") {
            channels::start_channels(config).await?;
        }
        return Ok(());
    }

    // All other commands need config loaded first
    let config = Config::load_or_init()?;

    match cli.command {
        Commands::Onboard { .. } => unreachable!(),

        Commands::Agent {
            message,
            provider,
            model,
            temperature,
            tui: use_tui,
        } => {
            if use_tui {
                tui::run(config, provider, model, temperature).await
            } else {
                agent::run(config, message, provider, model, temperature).await
            }
        }

        Commands::Tui {
            provider,
            model,
            temperature,
        } => tui::run(config, provider, model, temperature).await,

        Commands::Gateway { port, host } => {
            if port == 0 {
                info!("🚀 正在启动 Jarvis Gateway，地址 {host}（随机端口）");
            } else {
                info!("🚀 正在启动 Jarvis Gateway，地址 {host}:{port}");
            }
            gateway::run_gateway(&host, port, config).await
        }

        Commands::Daemon {
            port,
            host,
            foreground,
            stop,
        } => {
            if stop {
                return daemon::stop_daemon(&config);
            }

            if foreground {
                if port == 0 {
                    info!("🧠 正在启动 Jarvis 守护进程，地址 {host}（随机端口）");
                } else {
                    info!("🧠 正在启动 Jarvis 守护进程，地址 {host}:{port}");
                }
                daemon::run(config, host, port).await
            } else {
                // 后台启动模式
                if let Some(pid) = daemon::is_daemon_running(&config) {
                    println!("守护进程已在运行（PID {pid}）");
                    return Ok(());
                }

                // 创建日志目录
                let logs_dir = config
                    .config_path
                    .parent()
                    .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
                    .join("logs");
                std::fs::create_dir_all(&logs_dir)?;

                let stdout_log = logs_dir.join("daemon.stdout.log");
                let stderr_log = logs_dir.join("daemon.stderr.log");

                let exe = std::env::current_exe().context("无法获取当前可执行文件路径")?;

                let stdout_file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stdout_log)
                    .context("打开 stdout 日志文件失败")?;
                let stderr_file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stderr_log)
                    .context("打开 stderr 日志文件失败")?;

                let mut cmd = std::process::Command::new(exe);
                cmd.args(["daemon", "--foreground"])
                    .args(["--port", &port.to_string()])
                    .args(["--host", &host])
                    .stdout(stdout_file)
                    .stderr(stderr_file);

                // Unix: 使进程脱离当前会话
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    cmd.process_group(0);
                }

                let child = cmd.spawn().context("启动守护进程失败")?;
                let child_pid = child.id();

                // 等待短暂时间确认进程启动成功
                std::thread::sleep(std::time::Duration::from_millis(500));
                if daemon::is_daemon_running(&config).is_some() {
                    println!("🧠 Jarvis 守护进程已在后台启动（PID {child_pid}）");
                    println!("   Gateway：http://{host}:{port}");
                    println!("   日志：{}", logs_dir.display());
                    println!("   停止：jarvis daemon --stop");
                } else {
                    println!("⚠️  守护进程可能启动失败，请查看日志：");
                    println!("   {}", stderr_log.display());
                }
                Ok(())
            }
        }

        Commands::Status => {
            println!("🤖 Jarvis 状态");
            println!();
            println!("版本：       {}", env!("CARGO_PKG_VERSION"));
            println!("工作区：     {}", config.workspace_dir.display());
            println!("配置文件：   {}", config.config_path.display());
            println!();
            println!(
                "🤖 Provider：     {}",
                config.default_provider.as_deref().unwrap_or("openrouter")
            );
            println!(
                "   模型：         {}",
                config.default_model.as_deref().unwrap_or("（默认）")
            );
            println!("📊 可观测性：     {}", config.observability.backend);
            println!("🛡️  自主等级：     {:?}", config.autonomy.level);
            println!("⚙️  运行时：       {}", config.runtime.kind);
            println!(
                "💓 心跳：         {}",
                if config.heartbeat.enabled {
                    format!("每 {} 分钟", config.heartbeat.interval_minutes)
                } else {
                    "已禁用".into()
                }
            );
            println!(
                "🧠 记忆：         {}（自动保存：{}）",
                config.memory.backend,
                if config.memory.auto_save {
                    "开"
                } else {
                    "关"
                }
            );

            println!();
            println!("安全设置：");
            println!("  仅限工作区：     {}", config.autonomy.workspace_only);
            println!(
                "  允许的命令：     {}",
                config.autonomy.allowed_commands.join(", ")
            );
            println!(
                "  每小时最大操作数：{}",
                config.autonomy.max_actions_per_hour
            );
            println!(
                "  每日最大费用：   ${:.2}",
                f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
            );
            // 守护进程运行时状态
            println!();
            if let Some(pid) = daemon::is_daemon_running(&config) {
                println!("守护进程：    ✅ 运行中（PID {pid}）");
                let state_path = daemon::state_file_path(&config);
                if let Ok(data) = std::fs::read_to_string(&state_path) {
                    if let Ok(state) = serde_json::from_str::<serde_json::Value>(&data) {
                        if let Some(uptime) = state
                            .get("uptime_seconds")
                            .and_then(serde_json::Value::as_u64)
                        {
                            let hours = uptime / 3600;
                            let mins = (uptime % 3600) / 60;
                            if hours > 0 {
                                println!("  运行时间：  {hours}小时{mins}分钟");
                            } else {
                                println!("  运行时间：  {mins}分钟");
                            }
                        }
                        if let Some(components) = state
                            .get("components")
                            .and_then(serde_json::Value::as_object)
                        {
                            println!("  组件：");
                            for (name, info) in components {
                                let status = info
                                    .get("status")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("未知");
                                let icon = if status == "ok" { "✅" } else { "❌" };
                                println!("    {name:12} {icon} {status}");
                            }
                        }
                    }
                }
            } else {
                println!("守护进程：    ❌ 未运行");
                println!("  提示：使用 jarvis daemon 启动");
            }

            println!();
            println!("通道：");
            println!("  CLI：     ✅ 始终启用");
            for (name, configured) in [
                ("Telegram", config.channels_config.telegram.is_some()),
                ("Discord", config.channels_config.discord.is_some()),
                ("Slack", config.channels_config.slack.is_some()),
                ("Webhook", config.channels_config.webhook.is_some()),
            ] {
                println!(
                    "  {name:9} {}",
                    if configured {
                        "✅ 已配置"
                    } else {
                        "❌ 未配置"
                    }
                );
            }

            Ok(())
        }

        Commands::Cron { cron_command } => cron::handle_command(cron_command, &config),

        Commands::Service { service_command } => service::handle_command(&service_command, &config),

        Commands::Doctor => doctor::run(&config),

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => channels::start_channels(config).await,
            ChannelCommands::Doctor => channels::doctor_channels(config).await,
            other => channels::handle_command(other, &config),
        },

        Commands::Integrations {
            integration_command,
        } => integrations::handle_command(integration_command, &config),

        Commands::Skills { skill_command } => {
            skills::handle_command(skill_command, &config.workspace_dir)
        }

        Commands::Migrate { migrate_command } => {
            migration::handle_command(migrate_command, &config).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }
}
