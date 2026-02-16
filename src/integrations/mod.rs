pub mod registry;

use crate::config::Config;
use anyhow::Result;

/// Integration status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationStatus {
    /// Fully implemented and ready to use
    Available,
    /// Configured and active
    Active,
    /// Planned but not yet implemented
    ComingSoon,
}

/// Integration category
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationCategory {
    Chat,
    AiModel,
    Productivity,
    MusicAudio,
    SmartHome,
    ToolsAutomation,
    MediaCreative,
    Social,
    Platform,
}

impl IntegrationCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "聊天通道",
            Self::AiModel => "AI 模型",
            Self::Productivity => "生产力工具",
            Self::MusicAudio => "音乐与音频",
            Self::SmartHome => "智能家居",
            Self::ToolsAutomation => "工具与自动化",
            Self::MediaCreative => "媒体与创意",
            Self::Social => "社交",
            Self::Platform => "平台",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Chat,
            Self::AiModel,
            Self::Productivity,
            Self::MusicAudio,
            Self::SmartHome,
            Self::ToolsAutomation,
            Self::MediaCreative,
            Self::Social,
            Self::Platform,
        ]
    }
}

/// A registered integration
pub struct IntegrationEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub category: IntegrationCategory,
    pub status_fn: fn(&Config) -> IntegrationStatus,
}

/// Handle the `integrations` CLI command
pub fn handle_command(command: crate::IntegrationCommands, config: &Config) -> Result<()> {
    match command {
        crate::IntegrationCommands::Info { name } => show_integration_info(config, &name),
    }
}

fn show_integration_info(config: &Config, name: &str) -> Result<()> {
    let entries = registry::all_integrations();
    let name_lower = name.to_lowercase();

    let Some(entry) = entries.iter().find(|e| e.name.to_lowercase() == name_lower) else {
        anyhow::bail!(
            "未知集成: {name}。请查看 README 了解支持的集成，或运行 `jarvis onboard --interactive` 配置通道/提供商。"
        );
    };

    let status = (entry.status_fn)(config);
    let (icon, label) = match status {
        IntegrationStatus::Active => ("✅", "已激活"),
        IntegrationStatus::Available => ("⚪", "可用"),
        IntegrationStatus::ComingSoon => ("🔜", "即将推出"),
    };

    println!();
    println!(
        "  {} {} — {}",
        icon,
        console::style(entry.name).white().bold(),
        entry.description
    );
    println!("  分类: {}", entry.category.label());
    println!("  状态: {label}");
    println!();

    // 根据集成类型显示配置提示
    match entry.name {
        "Telegram" => {
            println!("  配置步骤:");
            println!("    1. 在 Telegram 上联系 @BotFather");
            println!("    2. 创建机器人并复制 token");
            println!("    3. 运行: jarvis onboard");
            println!("    4. 启动: jarvis channel start");
        }
        "Discord" => {
            println!("  配置步骤:");
            println!("    1. 前往 https://discord.com/developers/applications");
            println!("    2. 创建应用 → Bot → 复制 token");
            println!("    3. 启用 MESSAGE CONTENT intent");
            println!("    4. 运行: jarvis onboard");
        }
        "Slack" => {
            println!("  配置步骤:");
            println!("    1. 前往 https://api.slack.com/apps");
            println!("    2. 创建应用 → Bot Token Scopes → 安装");
            println!("    3. 运行: jarvis onboard");
        }
        "OpenRouter" => {
            println!("  配置步骤:");
            println!("    1. 在 https://openrouter.ai/keys 获取 API key");
            println!("    2. 运行: jarvis onboard");
            println!("    一个 API key 即可访问 200+ 模型。");
        }
        "Ollama" => {
            println!("  配置步骤:");
            println!("    1. 安装: brew install ollama");
            println!("    2. 拉取模型: ollama pull llama3");
            println!("    3. 在 config.toml 中设置 provider 为 'ollama'");
        }
        "iMessage" => {
            println!("  配置步骤 (仅限 macOS):");
            println!("    通过 AppleScript 桥接收发 iMessage。");
            println!("    需要在「系统设置 → 隐私」中授予「完全磁盘访问权限」。");
        }
        "GitHub" => {
            println!("  配置步骤:");
            println!("    1. 在 https://github.com/settings/tokens 创建个人访问令牌");
            println!("    2. 添加到配置: [integrations.github] token = \"ghp_...\"");
        }
        "Browser" => {
            println!("  内置功能:");
            println!("    Jarvis 可控制 Chrome/Chromium 执行网页任务。");
            println!("    使用无头浏览器自动化。");
        }
        "Cron" => {
            println!("  内置功能:");
            println!("    在 ~/.jarvis/workspace/cron/ 中调度任务。");
            println!("    运行: jarvis cron list");
        }
        "Webhooks" => {
            println!("  内置功能:");
            println!("    用于外部触发的 HTTP 端点。");
            println!("    运行: jarvis gateway");
        }
        _ => {
            if status == IntegrationStatus::ComingSoon {
                println!("  此集成正在规划中，敬请期待！");
                println!("  跟踪进度: https://github.com/Afee2019/jarvis");
            }
        }
    }

    println!();
    Ok(())
}
