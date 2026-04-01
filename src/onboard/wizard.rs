use crate::config::schema::{IrcConfig, WhatsAppConfig};
use crate::config::{
    AutonomyConfig, BrowserConfig, ChannelsConfig, ComposioConfig, Config, DiscordConfig,
    HeartbeatConfig, IMessageConfig, MatrixConfig, MemoryConfig, ObservabilityConfig,
    RuntimeConfig, SecretsConfig, SlackConfig, TelegramConfig, WebhookConfig,
};
use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};
use std::fs;
use std::path::{Path, PathBuf};

// ── Project context collected during wizard ──────────────────────

/// User-provided personalization baked into workspace MD files.
#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub user_name: String,
    pub timezone: String,
    pub agent_name: String,
    pub communication_style: String,
}

// ── Banner ───────────────────────────────────────────────────────

const BANNER: &str = r"
    ⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡

    ███████╗███████╗██████╗  ██████╗  ██████╗██╗      █████╗ ██╗    ██╗
    ╚══███╔╝██╔════╝██╔══██╗██╔═══██╗██╔════╝██║     ██╔══██╗██║    ██║
      ███╔╝ █████╗  ██████╔╝██║   ██║██║     ██║     ███████║██║ █╗ ██║
     ███╔╝  ██╔══╝  ██╔══██╗██║   ██║██║     ██║     ██╔══██║██║███╗██║
    ███████╗███████╗██║  ██║╚██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝
    ╚══════╝╚══════╝╚═╝  ╚═╝ ╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝

    你的 AI，你做主。

    ⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡⚡
";

// ── Main wizard entry point ──────────────────────────────────────

pub fn run_wizard() -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());

    println!(
        "  {}",
        style("欢迎使用 Jarvis — 最快、最轻量的 AI 助手。")
            .white()
            .bold()
    );
    println!("  {}", style("本向导将在 60 秒内完成 Agent 配置。").dim());
    println!();

    print_step(1, 8, "工作区设置");
    let (workspace_dir, config_path) = setup_workspace()?;

    print_step(2, 8, "AI Provider 与 API 密钥");
    let (provider, api_key, model) = setup_provider()?;

    print_step(3, 8, "通道（与 Jarvis 对话的方式）");
    let channels_config = setup_channels()?;

    print_step(4, 8, "隧道（暴露到互联网）");
    let tunnel_config = setup_tunnel()?;

    print_step(5, 8, "工具模式与安全");
    let (composio_config, secrets_config) = setup_tool_mode()?;

    print_step(6, 8, "记忆配置");
    let memory_config = setup_memory()?;

    print_step(7, 8, "项目上下文（个性化你的 Agent）");
    let project_ctx = setup_project_context()?;

    print_step(8, 8, "工作区文件");
    scaffold_workspace(&workspace_dir, &project_ctx)?;

    // ── Build config ──
    // Defaults: SQLite memory, supervised autonomy, workspace-scoped, native runtime
    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        default_provider: Some(provider),
        default_model: Some(model),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config,
        memory: memory_config, // User-selected memory backend
        tunnel: tunnel_config,
        gateway: crate::config::GatewayConfig::default(),
        composio: composio_config,
        secrets: secrets_config,
        browser: BrowserConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        brave_search: crate::config::BraveSearchConfig::default(),
    };

    println!(
        "  {} 安全：{} | 限定工作区",
        style("✓").green().bold(),
        style("受监督模式").green()
    );
    println!(
        "  {} 记忆：{}（自动保存：{}）",
        style("✓").green().bold(),
        style(&config.memory.backend).green(),
        if config.memory.auto_save {
            "开"
        } else {
            "关"
        }
    );

    config.save()?;

    // ── Final summary ────────────────────────────────────────────
    print_summary(&config);

    // ── Offer to launch channels immediately ─────────────────────
    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some();

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} 立即启动通道？（已连接通道 → AI → 自动回复）",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("正在启动通道服务器...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            // SAFETY: 单线程上下文，wizard 在 daemon 启动前执行
unsafe { std::env::set_var("JARVIS_AUTOSTART_CHANNELS", "1") };
        }
    }

    Ok(config)
}

/// Interactive repair flow: rerun channel setup only without redoing full onboarding.
pub fn run_channels_repair_wizard() -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());
    println!(
        "  {}",
        style("通道修复 — 仅更新通道 Token 和白名单").white().bold()
    );
    println!();

    let mut config = Config::load_or_init()?;

    print_step(1, 1, "通道（与 Jarvis 对话的方式）");
    config.channels_config = setup_channels()?;
    config.save()?;

    println!();
    println!(
        "  {} 通道配置已保存：{}",
        style("✓").green().bold(),
        style(config.config_path.display()).green()
    );

    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some();

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} 立即启动通道？（已连接通道 → AI → 自动回复）",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("正在启动通道服务器...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            // SAFETY: 单线程上下文，wizard 在 daemon 启动前执行
unsafe { std::env::set_var("JARVIS_AUTOSTART_CHANNELS", "1") };
        }
    }

    Ok(config)
}

// ── Quick setup (zero prompts) ───────────────────────────────────

/// Non-interactive setup: generates a sensible default config instantly.
/// Use `jarvis onboard` or `jarvis onboard --api-key sk-... --provider openrouter --memory sqlite`.
/// Use `jarvis onboard --interactive` for the full wizard.
#[allow(clippy::too_many_lines)]
pub fn run_quick_setup(
    api_key: Option<&str>,
    provider: Option<&str>,
    memory_backend: Option<&str>,
) -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());
    println!(
        "  {}",
        style("快速设置 — 正在使用合理默认值生成配置...")
            .white()
            .bold()
    );
    println!();

    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("无法找到用户主目录")?;
    let jarvis_dir = home.join(".jarvis");
    let workspace_dir = jarvis_dir.join("workspace");
    let config_path = jarvis_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("创建工作区目录失败")?;

    let provider_name = provider.unwrap_or("openrouter").to_string();
    let model = default_model_for_provider(&provider_name);
    let memory_backend_name = memory_backend.unwrap_or("sqlite").to_string();

    // Create memory config based on backend choice
    let memory_config = MemoryConfig {
        backend: memory_backend_name.clone(),
        auto_save: memory_backend_name != "none",
        hygiene_enabled: memory_backend_name == "sqlite",
        archive_after_days: if memory_backend_name == "sqlite" {
            7
        } else {
            0
        },
        purge_after_days: if memory_backend_name == "sqlite" {
            30
        } else {
            0
        },
        conversation_retention_days: 30,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        embedding_cache_size: if memory_backend_name == "sqlite" {
            10000
        } else {
            0
        },
        chunk_max_tokens: 512,
    };

    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: api_key.map(String::from),
        default_provider: Some(provider_name.clone()),
        default_model: Some(model.clone()),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config: ChannelsConfig::default(),
        memory: memory_config,
        tunnel: crate::config::TunnelConfig::default(),
        gateway: crate::config::GatewayConfig::default(),
        composio: ComposioConfig::default(),
        secrets: SecretsConfig::default(),
        browser: BrowserConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        brave_search: crate::config::BraveSearchConfig::default(),
    };

    config.save()?;

    // Scaffold minimal workspace files
    let default_ctx = ProjectContext {
        user_name: std::env::var("USER").unwrap_or_else(|_| "User".into()),
        timezone: "UTC".into(),
        agent_name: "Jarvis".into(),
        communication_style:
            "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
                .into(),
    };
    scaffold_workspace(&workspace_dir, &default_ctx)?;

    println!(
        "  {} 工作区：    {}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );
    println!(
        "  {} Provider：  {}",
        style("✓").green().bold(),
        style(&provider_name).green()
    );
    println!(
        "  {} 模型：      {}",
        style("✓").green().bold(),
        style(&model).green()
    );
    println!(
        "  {} API 密钥：  {}",
        style("✓").green().bold(),
        if api_key.is_some() {
            style("已设置").green()
        } else {
            style("未设置（使用 --api-key 或编辑 config.toml）").yellow()
        }
    );
    println!(
        "  {} 安全：      {}",
        style("✓").green().bold(),
        style("受监督模式（限定工作区）").green()
    );
    println!(
        "  {} 记忆：      {}（自动保存：{}）",
        style("✓").green().bold(),
        style(&memory_backend_name).green(),
        if memory_backend_name == "none" {
            "关"
        } else {
            "开"
        }
    );
    println!(
        "  {} 密钥存储：  {}",
        style("✓").green().bold(),
        style("加密").green()
    );
    println!(
        "  {} Gateway：   {}",
        style("✓").green().bold(),
        style("需要配对（127.0.0.1:8080）").green()
    );
    println!(
        "  {} 隧道：      {}",
        style("✓").green().bold(),
        style("无（仅本地）").dim()
    );
    println!(
        "  {} Composio：  {}",
        style("✓").green().bold(),
        style("已禁用（自主模式）").dim()
    );
    println!();
    println!(
        "  {} {}",
        style("配置已保存：").white().bold(),
        style(config_path.display()).green()
    );
    println!();
    println!("  {}", style("后续步骤：").white().bold());
    if api_key.is_none() {
        println!("    1. 设置 API 密钥：export OPENROUTER_API_KEY=\"sk-...\"");
        println!("    2. 或编辑：       ~/.jarvis/config.toml");
        println!("    3. 对话：         jarvis agent -m \"你好！\"");
        println!("    4. Gateway：      jarvis gateway");
    } else {
        println!("    1. 对话：    jarvis agent -m \"你好！\"");
        println!("    2. Gateway：jarvis gateway");
        println!("    3. 状态：    jarvis status");
    }
    println!();

    Ok(config)
}

/// Pick a sensible default model for the given provider.
fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-20250514".into(),
        "openai" => "gpt-4o".into(),
        "ollama" => "llama3.2".into(),
        "groq" => "llama-3.3-70b-versatile".into(),
        "deepseek" => "deepseek-chat".into(),
        "gemini" | "google" | "google-gemini" => "gemini-2.0-flash".into(),
        _ => "anthropic/claude-sonnet-4-20250514".into(),
    }
}

// ── Step helpers ─────────────────────────────────────────────────

fn print_step(current: u8, total: u8, title: &str) {
    println!();
    println!(
        "  {} {}",
        style(format!("[{current}/{total}]")).cyan().bold(),
        style(title).white().bold()
    );
    println!("  {}", style("─".repeat(50)).dim());
}

fn print_bullet(text: &str) {
    println!("  {} {}", style("›").cyan(), text);
}

// ── Step 1: Workspace ────────────────────────────────────────────

fn setup_workspace() -> Result<(PathBuf, PathBuf)> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("无法找到用户主目录")?;
    let default_dir = home.join(".jarvis");

    print_bullet(&format!(
        "默认位置：{}",
        style(default_dir.display()).green()
    ));

    let use_default = Confirm::new()
        .with_prompt("  使用默认工作区位置？")
        .default(true)
        .interact()?;

    let jarvis_dir = if use_default {
        default_dir
    } else {
        let custom: String = Input::new()
            .with_prompt("  输入工作区路径")
            .interact_text()?;
        let expanded = shellexpand::tilde(&custom).to_string();
        PathBuf::from(expanded)
    };

    let workspace_dir = jarvis_dir.join("workspace");
    let config_path = jarvis_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("创建工作区目录失败")?;

    println!(
        "  {} 工作区：{}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );

    Ok((workspace_dir, config_path))
}

// ── Step 2: Provider & API Key ───────────────────────────────────

#[allow(clippy::too_many_lines)]
fn setup_provider() -> Result<(String, String, String)> {
    // ── Tier selection ──
    let tiers = vec![
        "⭐ 推荐（OpenRouter、Venice、Anthropic、OpenAI、Gemini）",
        "⚡ 快速推理（Groq、Fireworks、Together AI）",
        "🌐 网关/代理（Vercel AI、Cloudflare AI、Amazon Bedrock）",
        "🔬 专业化（Moonshot/Kimi、GLM/智谱、MiniMax、千帆、Z.AI、Synthetic、OpenCode Zen、Cohere）",
        "🏠 本地/私有（Ollama — 无需 API 密钥）",
        "🔧 自定义 — 使用你自己的 OpenAI 兼容 API",
    ];

    let tier_idx = Select::new()
        .with_prompt("  选择 Provider 类别")
        .items(&tiers)
        .default(0)
        .interact()?;

    let providers: Vec<(&str, &str)> = match tier_idx {
        0 => vec![
            (
                "openrouter",
                "OpenRouter — 200+ 模型，1 个 API 密钥（推荐）",
            ),
            ("venice", "Venice AI — 隐私优先（Llama、Opus）"),
            ("anthropic", "Anthropic — Claude Sonnet & Opus（直连）"),
            ("openai", "OpenAI — GPT-4o、o1、GPT-5（直连）"),
            ("deepseek", "DeepSeek — V3 & R1（经济实惠）"),
            ("mistral", "Mistral — Large & Codestral"),
            ("xai", "xAI — Grok 3 & 4"),
            ("perplexity", "Perplexity — 搜索增强 AI"),
            (
                "gemini",
                "Google Gemini — Gemini 2.0 Flash & Pro（支持 CLI 认证）",
            ),
        ],
        1 => vec![
            ("groq", "Groq — 超快 LPU 推理"),
            ("fireworks", "Fireworks AI — 快速开源推理"),
            ("together", "Together AI — 开源模型托管"),
        ],
        2 => vec![
            ("vercel", "Vercel AI Gateway"),
            ("cloudflare", "Cloudflare AI Gateway"),
            ("bedrock", "Amazon Bedrock — AWS 托管模型"),
        ],
        3 => vec![
            ("moonshot", "Moonshot — Kimi & Kimi Coding"),
            ("glm", "GLM — ChatGLM / 智谱模型"),
            ("minimax", "MiniMax — MiniMax AI 模型"),
            ("qianfan", "千帆 — 百度 AI 模型"),
            ("zai", "Z.AI — Z.AI 推理"),
            ("synthetic", "Synthetic — Synthetic AI 模型"),
            ("opencode", "OpenCode Zen — 代码专注 AI"),
            ("cohere", "Cohere — Command R+ & embeddings"),
        ],
        4 => vec![("ollama", "Ollama — 本地模型（Llama、Mistral、Phi）")],
        _ => vec![], // Custom — handled below
    };

    // ── Custom / BYOP flow ──
    if providers.is_empty() {
        println!();
        println!(
            "  {} {}",
            style("自定义 Provider 设置").white().bold(),
            style("— 任何 OpenAI 兼容 API").dim()
        );
        print_bullet("Jarvis 支持任何兼容 OpenAI chat completions 格式的 API。");
        print_bullet("示例：LiteLLM、LocalAI、vLLM、text-generation-webui、LM Studio 等。");
        println!();

        let base_url: String = Input::new()
            .with_prompt("  API 基础 URL（例如 http://localhost:1234 或 https://my-api.com）")
            .interact_text()?;

        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            anyhow::bail!("自定义 Provider 需要提供基础 URL。");
        }

        let api_key: String = Input::new()
            .with_prompt("  API 密钥（不需要则按 Enter 跳过）")
            .allow_empty(true)
            .interact_text()?;

        let model: String = Input::new()
            .with_prompt("  模型名称（例如 llama3、gpt-4o、mistral）")
            .default("default".into())
            .interact_text()?;

        let provider_name = format!("custom:{base_url}");

        println!(
            "  {} Provider：{} | 模型：{}",
            style("✓").green().bold(),
            style(&provider_name).green(),
            style(&model).green()
        );

        return Ok((provider_name, api_key, model));
    }

    let provider_labels: Vec<&str> = providers.iter().map(|(_, label)| *label).collect();

    let provider_idx = Select::new()
        .with_prompt("  选择你的 AI Provider")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let provider_name = providers[provider_idx].0;

    // ── API key ──
    let api_key = if provider_name == "ollama" {
        print_bullet("Ollama 在本地运行 — 无需 API 密钥！");
        String::new()
    } else if provider_name == "gemini"
        || provider_name == "google"
        || provider_name == "google-gemini"
    {
        // Special handling for Gemini: check for CLI auth first
        if crate::providers::gemini::GeminiProvider::has_cli_credentials() {
            print_bullet(&format!(
                "{} 检测到 Gemini CLI 凭据！你可以跳过 API 密钥。",
                style("✓").green().bold()
            ));
            print_bullet("Jarvis 将复用你现有的 Gemini CLI 认证。");
            println!();

            let use_cli: bool = dialoguer::Confirm::new()
                .with_prompt("  使用现有的 Gemini CLI 认证？")
                .default(true)
                .interact()?;

            if use_cli {
                println!(
                    "  {} 使用 Gemini CLI OAuth tokens",
                    style("✓").green().bold()
                );
                String::new() // Empty key = will use CLI tokens
            } else {
                print_bullet("在此获取 API 密钥：https://aistudio.google.com/app/apikey");
                Input::new()
                    .with_prompt("  粘贴你的 Gemini API 密钥")
                    .allow_empty(true)
                    .interact_text()?
            }
        } else if std::env::var("GEMINI_API_KEY").is_ok() {
            print_bullet(&format!(
                "{} 检测到 GEMINI_API_KEY 环境变量！",
                style("✓").green().bold()
            ));
            String::new()
        } else {
            print_bullet("在此获取 API 密钥：https://aistudio.google.com/app/apikey");
            print_bullet("或运行 `gemini` CLI 进行认证（tokens 将被复用）。");
            println!();

            Input::new()
                .with_prompt("  粘贴你的 Gemini API 密钥（或按 Enter 跳过）")
                .allow_empty(true)
                .interact_text()?
        }
    } else {
        let key_url = match provider_name {
            "openrouter" => "https://openrouter.ai/keys",
            "anthropic" => "https://console.anthropic.com/settings/keys",
            "openai" => "https://platform.openai.com/api-keys",
            "venice" => "https://venice.ai/settings/api",
            "groq" => "https://console.groq.com/keys",
            "mistral" => "https://console.mistral.ai/api-keys",
            "deepseek" => "https://platform.deepseek.com/api_keys",
            "together" => "https://api.together.xyz/settings/api-keys",
            "fireworks" => "https://fireworks.ai/account/api-keys",
            "perplexity" => "https://www.perplexity.ai/settings/api",
            "xai" => "https://console.x.ai",
            "cohere" => "https://dashboard.cohere.com/api-keys",
            "moonshot" => "https://platform.moonshot.cn/console/api-keys",
            "minimax" => "https://www.minimaxi.com/user-center/basic-information",
            "vercel" => "https://vercel.com/account/tokens",
            "cloudflare" => "https://dash.cloudflare.com/profile/api-tokens",
            "bedrock" => "https://console.aws.amazon.com/iam",
            "gemini" | "google" | "google-gemini" => "https://aistudio.google.com/app/apikey",
            _ => "",
        };

        println!();
        if !key_url.is_empty() {
            print_bullet(&format!(
                "在此获取 API 密钥：{}",
                style(key_url).cyan().underlined()
            ));
        }
        print_bullet("你也可以稍后通过环境变量或配置文件设置。");
        println!();

        let key: String = Input::new()
            .with_prompt("  粘贴你的 API 密钥（或按 Enter 跳过）")
            .allow_empty(true)
            .interact_text()?;

        if key.is_empty() {
            let env_var = provider_env_var(provider_name);
            print_bullet(&format!(
                "已跳过。稍后设置 {} 或编辑 config.toml。",
                style(env_var).yellow()
            ));
        }

        key
    };

    // ── Model selection ──
    let models: Vec<(&str, &str)> = match provider_name {
        "openrouter" => vec![
            (
                "anthropic/claude-sonnet-4-20250514",
                "Claude Sonnet 4 (balanced, recommended)",
            ),
            (
                "anthropic/claude-3.5-sonnet",
                "Claude 3.5 Sonnet (fast, affordable)",
            ),
            ("openai/gpt-4o", "GPT-4o (OpenAI flagship)"),
            ("openai/gpt-4o-mini", "GPT-4o Mini (fast, cheap)"),
            (
                "google/gemini-2.0-flash-001",
                "Gemini 2.0 Flash (Google, fast)",
            ),
            (
                "meta-llama/llama-3.3-70b-instruct",
                "Llama 3.3 70B (open source)",
            ),
            ("deepseek/deepseek-chat", "DeepSeek Chat (affordable)"),
        ],
        "anthropic" => vec![
            (
                "claude-sonnet-4-20250514",
                "Claude Sonnet 4 (balanced, recommended)",
            ),
            ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet (fast)"),
            (
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku (fastest, cheapest)",
            ),
        ],
        "openai" => vec![
            ("gpt-4o", "GPT-4o (flagship)"),
            ("gpt-4o-mini", "GPT-4o Mini (fast, cheap)"),
            ("o1-mini", "o1-mini (reasoning)"),
        ],
        "venice" => vec![
            ("llama-3.3-70b", "Llama 3.3 70B (default, fast)"),
            ("claude-opus-45", "Claude Opus 4.5 via Venice (strongest)"),
            ("llama-3.1-405b", "Llama 3.1 405B (largest open source)"),
        ],
        "groq" => vec![
            (
                "llama-3.3-70b-versatile",
                "Llama 3.3 70B (fast, recommended)",
            ),
            ("llama-3.1-8b-instant", "Llama 3.1 8B (instant)"),
            ("mixtral-8x7b-32768", "Mixtral 8x7B (32K context)"),
        ],
        "mistral" => vec![
            ("mistral-large-latest", "Mistral Large (flagship)"),
            ("codestral-latest", "Codestral (code-focused)"),
            ("mistral-small-latest", "Mistral Small (fast, cheap)"),
        ],
        "deepseek" => vec![
            ("deepseek-chat", "DeepSeek Chat (V3, recommended)"),
            ("deepseek-reasoner", "DeepSeek Reasoner (R1)"),
        ],
        "xai" => vec![
            ("grok-3", "Grok 3 (flagship)"),
            ("grok-3-mini", "Grok 3 Mini (fast)"),
        ],
        "perplexity" => vec![
            ("sonar-pro", "Sonar Pro (search + reasoning)"),
            ("sonar", "Sonar (search, fast)"),
        ],
        "fireworks" => vec![
            (
                "accounts/fireworks/models/llama-v3p3-70b-instruct",
                "Llama 3.3 70B",
            ),
            (
                "accounts/fireworks/models/mixtral-8x22b-instruct",
                "Mixtral 8x22B",
            ),
        ],
        "together" => vec![
            (
                "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
                "Llama 3.1 70B Turbo",
            ),
            (
                "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
                "Llama 3.1 8B Turbo",
            ),
            ("mistralai/Mixtral-8x22B-Instruct-v0.1", "Mixtral 8x22B"),
        ],
        "cohere" => vec![
            ("command-r-plus", "Command R+ (flagship)"),
            ("command-r", "Command R (fast)"),
        ],
        "moonshot" => vec![
            ("moonshot-v1-128k", "Moonshot V1 128K"),
            ("moonshot-v1-32k", "Moonshot V1 32K"),
        ],
        "glm" => vec![
            ("glm-4-plus", "GLM-4 Plus (flagship)"),
            ("glm-4-flash", "GLM-4 Flash (fast)"),
        ],
        "minimax" => vec![
            ("abab6.5s-chat", "ABAB 6.5s Chat"),
            ("abab6.5-chat", "ABAB 6.5 Chat"),
        ],
        "ollama" => vec![
            ("llama3.2", "Llama 3.2 (recommended local)"),
            ("mistral", "Mistral 7B"),
            ("codellama", "Code Llama"),
            ("phi3", "Phi-3 (small, fast)"),
        ],
        "gemini" | "google" | "google-gemini" => vec![
            ("gemini-2.0-flash", "Gemini 2.0 Flash (fast, recommended)"),
            (
                "gemini-2.0-flash-lite",
                "Gemini 2.0 Flash Lite (fastest, cheapest)",
            ),
            ("gemini-1.5-pro", "Gemini 1.5 Pro (best quality)"),
            ("gemini-1.5-flash", "Gemini 1.5 Flash (balanced)"),
        ],
        _ => vec![("default", "Default model")],
    };

    let model_labels: Vec<&str> = models.iter().map(|(_, label)| *label).collect();

    let model_idx = Select::new()
        .with_prompt("  选择默认模型")
        .items(&model_labels)
        .default(0)
        .interact()?;

    let model = models[model_idx].0.to_string();

    println!(
        "  {} Provider：{} | 模型：{}",
        style("✓").green().bold(),
        style(provider_name).green(),
        style(&model).green()
    );

    Ok((provider_name.to_string(), api_key, model))
}

/// Map provider name to its conventional env var
fn provider_env_var(name: &str) -> &'static str {
    match name {
        "openrouter" => "OPENROUTER_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "venice" => "VENICE_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        "together" | "together-ai" => "TOGETHER_API_KEY",
        "fireworks" | "fireworks-ai" => "FIREWORKS_API_KEY",
        "perplexity" => "PERPLEXITY_API_KEY",
        "cohere" => "COHERE_API_KEY",
        "moonshot" | "kimi" => "MOONSHOT_API_KEY",
        "glm" | "zhipu" => "GLM_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "qianfan" | "baidu" => "QIANFAN_API_KEY",
        "zai" | "z.ai" => "ZAI_API_KEY",
        "synthetic" => "SYNTHETIC_API_KEY",
        "opencode" | "opencode-zen" => "OPENCODE_API_KEY",
        "vercel" | "vercel-ai" => "VERCEL_API_KEY",
        "cloudflare" | "cloudflare-ai" => "CLOUDFLARE_API_KEY",
        "bedrock" | "aws-bedrock" => "AWS_ACCESS_KEY_ID",
        "gemini" | "google" | "google-gemini" => "GEMINI_API_KEY",
        _ => "API_KEY",
    }
}

// ── Step 5: Tool Mode & Security ────────────────────────────────

fn setup_tool_mode() -> Result<(ComposioConfig, SecretsConfig)> {
    print_bullet("选择 Jarvis 连接外部应用的方式。");
    print_bullet("你可以随时在 config.toml 中更改。");
    println!();

    let options = vec![
        "自主模式（仅本地） — 你自己管理 API 密钥，完全隐私（默认）",
        "Composio（托管 OAuth） — 通过 OAuth 连接 1000+ 应用，无需共享原始密钥",
    ];

    let choice = Select::new()
        .with_prompt("  选择工具模式")
        .items(&options)
        .default(0)
        .interact()?;

    let composio_config = if choice == 1 {
        println!();
        println!(
            "  {} {}",
            style("Composio 设置").white().bold(),
            style("— 1000+ OAuth 集成（Gmail、Notion、GitHub、Slack……）").dim()
        );
        print_bullet("在此获取 API 密钥：https://app.composio.dev/settings");
        print_bullet("Jarvis 将 Composio 作为工具使用 — 你的核心 Agent 保持本地运行。");
        println!();

        let api_key: String = Input::new()
            .with_prompt("  Composio API 密钥（或按 Enter 跳过）")
            .allow_empty(true)
            .interact_text()?;

        if api_key.trim().is_empty() {
            println!(
                "  {} 已跳过 — 稍后在 config.toml 中设置 composio.api_key",
                style("→").dim()
            );
            ComposioConfig::default()
        } else {
            println!(
                "  {} Composio：{}（1000+ OAuth 工具可用）",
                style("✓").green().bold(),
                style("已启用").green()
            );
            ComposioConfig {
                enabled: true,
                api_key: Some(api_key),
                ..ComposioConfig::default()
            }
        }
    } else {
        println!(
            "  {} 工具模式：{} — 完全隐私，所有密钥由你掌控",
            style("✓").green().bold(),
            style("自主模式（仅本地）").green()
        );
        ComposioConfig::default()
    };

    // ── Encrypted secrets ──
    println!();
    print_bullet("Jarvis 可以加密存储在 config.toml 中的 API 密钥。");
    print_bullet("本地密钥文件可防止明文暴露和意外泄漏。");

    let encrypt = Confirm::new()
        .with_prompt("  启用加密密钥存储？")
        .default(true)
        .interact()?;

    let secrets_config = SecretsConfig { encrypt };

    if encrypt {
        println!(
            "  {} 密钥存储：{} — 使用本地密钥文件加密",
            style("✓").green().bold(),
            style("加密").green()
        );
    } else {
        println!(
            "  {} 密钥存储：{} — 明文存储（不推荐）",
            style("✓").green().bold(),
            style("明文").yellow()
        );
    }

    Ok((composio_config, secrets_config))
}

// ── Step 6: Project Context ─────────────────────────────────────

fn setup_project_context() -> Result<ProjectContext> {
    print_bullet("让我们个性化你的 Agent。你可以随时更新这些设置。");
    print_bullet("按 Enter 接受默认值。");
    println!();

    let user_name: String = Input::new()
        .with_prompt("  你的名字")
        .default("User".into())
        .interact_text()?;

    let tz_options = vec![
        "US/Eastern (EST/EDT)",
        "US/Central (CST/CDT)",
        "US/Mountain (MST/MDT)",
        "US/Pacific (PST/PDT)",
        "Europe/London (GMT/BST)",
        "Europe/Berlin (CET/CEST)",
        "Asia/Tokyo (JST)",
        "UTC",
        "其他（手动输入）",
    ];

    let tz_idx = Select::new()
        .with_prompt("  你的时区")
        .items(&tz_options)
        .default(0)
        .interact()?;

    let timezone = if tz_idx == tz_options.len() - 1 {
        Input::new()
            .with_prompt("  输入时区（例如 America/New_York）")
            .default("UTC".into())
            .interact_text()?
    } else {
        // Extract the short label before the parenthetical
        tz_options[tz_idx]
            .split('(')
            .next()
            .unwrap_or("UTC")
            .trim()
            .to_string()
    };

    let agent_name: String = Input::new()
        .with_prompt("  Agent 名称")
        .default("Jarvis".into())
        .interact_text()?;

    let style_options = vec![
        "直接简洁 — 跳过寒暄，直奔主题",
        "友好随和 — 温暖、自然、乐于助人",
        "专业精炼 — 沉稳、自信、清晰",
        "生动活泼 — 更多个性 + 自然的 emoji",
        "技术详尽 — 深入解释，代码优先",
        "均衡适应 — 根据情况灵活调整",
        "自定义 — 编写你自己的风格指南",
    ];

    let style_idx = Select::new()
        .with_prompt("  沟通风格")
        .items(&style_options)
        .default(1)
        .interact()?;

    let communication_style = match style_idx {
        0 => "Be direct and concise. Skip pleasantries. Get to the point.".to_string(),
        1 => "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions.".to_string(),
        2 => "Be professional and polished. Stay calm, structured, and respectful. Use occasional tone-setting emojis only when appropriate.".to_string(),
        3 => "Be expressive and playful when appropriate. Use relevant emojis naturally (0-2 max), and keep serious topics emoji-light.".to_string(),
        4 => "Be technical and detailed. Thorough explanations, code-first.".to_string(),
        5 => "Adapt to the situation. Default to warm and clear communication; be concise when needed, thorough when it matters.".to_string(),
        _ => Input::new()
            .with_prompt("  自定义沟通风格")
            .default(
                "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing.".into(),
            )
            .interact_text()?,
    };

    println!(
        "  {} 上下文：{} | {} | {} | {}",
        style("✓").green().bold(),
        style(&user_name).green(),
        style(&timezone).green(),
        style(&agent_name).green(),
        style(&communication_style).green().dim()
    );

    Ok(ProjectContext {
        user_name,
        timezone,
        agent_name,
        communication_style,
    })
}

// ── Step 6: Memory Configuration ───────────────────────────────

fn setup_memory() -> Result<MemoryConfig> {
    print_bullet("选择 Jarvis 存储和搜索记忆的方式。");
    print_bullet("你可以随时在 config.toml 中更改。");
    println!();

    let options = vec![
        "SQLite + 向量搜索（推荐） — 快速、混合搜索、embeddings",
        "Markdown 文件 — 简单、可读性强、无依赖",
        "无 — 禁用持久化记忆",
    ];

    let choice = Select::new()
        .with_prompt("  选择记忆后端")
        .items(&options)
        .default(0)
        .interact()?;

    let backend = match choice {
        1 => "markdown",
        2 => "none",
        _ => "sqlite", // 0 and any unexpected value defaults to sqlite
    };

    let auto_save = if backend == "none" {
        false
    } else {
        let save = Confirm::new()
            .with_prompt("  自动保存对话到记忆？")
            .default(true)
            .interact()?;
        save
    };

    println!(
        "  {} 记忆：{}（自动保存：{}）",
        style("✓").green().bold(),
        style(backend).green(),
        if auto_save { "开" } else { "关" }
    );

    Ok(MemoryConfig {
        backend: backend.to_string(),
        auto_save,
        hygiene_enabled: backend == "sqlite", // Only enable hygiene for SQLite
        archive_after_days: if backend == "sqlite" { 7 } else { 0 },
        purge_after_days: if backend == "sqlite" { 30 } else { 0 },
        conversation_retention_days: 30,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        embedding_cache_size: if backend == "sqlite" { 10000 } else { 0 },
        chunk_max_tokens: 512,
    })
}

// ── Step 3: Channels ────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn setup_channels() -> Result<ChannelsConfig> {
    print_bullet("通道让你可以从任何地方与 Jarvis 对话。");
    print_bullet("CLI 始终可用。现在可以连接更多通道。");
    println!();

    let mut config = ChannelsConfig {
        cli: true,
        telegram: None,
        discord: None,
        slack: None,
        webhook: None,
        imessage: None,
        matrix: None,
        whatsapp: None,
        irc: None,
        dingtalk: None,
    };

    loop {
        let options = vec![
            format!(
                "Telegram   {}",
                if config.telegram.is_some() {
                    "✅ 已连接"
                } else {
                    "— 连接你的机器人"
                }
            ),
            format!(
                "Discord    {}",
                if config.discord.is_some() {
                    "✅ 已连接"
                } else {
                    "— 连接你的机器人"
                }
            ),
            format!(
                "Slack      {}",
                if config.slack.is_some() {
                    "✅ 已连接"
                } else {
                    "— 连接你的机器人"
                }
            ),
            format!(
                "iMessage   {}",
                if config.imessage.is_some() {
                    "✅ 已配置"
                } else {
                    "— 仅 macOS"
                }
            ),
            format!(
                "Matrix     {}",
                if config.matrix.is_some() {
                    "✅ 已连接"
                } else {
                    "— 自托管聊天"
                }
            ),
            format!(
                "WhatsApp   {}",
                if config.whatsapp.is_some() {
                    "✅ 已连接"
                } else {
                    "— Business Cloud API"
                }
            ),
            format!(
                "IRC        {}",
                if config.irc.is_some() {
                    "✅ 已配置"
                } else {
                    "— IRC over TLS"
                }
            ),
            format!(
                "DingTalk   {}",
                if config.dingtalk.is_some() {
                    "✅ 已配置"
                } else {
                    "— 钉钉企业应用"
                }
            ),
            format!(
                "Webhook    {}",
                if config.webhook.is_some() {
                    "✅ 已配置"
                } else {
                    "— HTTP 端点"
                }
            ),
            "完成 — 结束设置".to_string(),
        ];

        let choice = Select::new()
            .with_prompt("  连接通道（或选择「完成」继续）")
            .items(&options)
            .default(8)
            .interact()?;

        match choice {
            0 => {
                // ── Telegram ──
                println!();
                println!(
                    "  {} {}",
                    style("Telegram 设置").white().bold(),
                    style("— 从 Telegram 与 Jarvis 对话").dim()
                );
                print_bullet("1. 打开 Telegram，向 @BotFather 发消息");
                print_bullet("2. 发送 /newbot 并按提示操作");
                print_bullet("3. 复制机器人 Token 并粘贴到下方");
                println!();

                let token: String = Input::new()
                    .with_prompt("  机器人 Token（来自 @BotFather）")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let url = format!("https://api.telegram.org/bot{token}/getMe");
                match client.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let bot_name = data
                            .get("result")
                            .and_then(|r| r.get("username"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} 已连接为 @{bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} 连接失败 — 请检查 Token 后重试",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                print_bullet("建议先将你自己的 Telegram 身份加入白名单（安全且快速的设置方式）。");
                print_bullet(
                    "使用你的 @用户名（不含 '@'，例如：argenis），或你的 Telegram 数字用户 ID。",
                );
                print_bullet("仅在临时开放测试时使用 '*'。");

                let users_str: String = Input::new()
                    .with_prompt(
                        "  允许的 Telegram 身份（逗号分隔：不含 '@' 的用户名和/或数字用户 ID，'*' 表示所有）",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} 白名单为空 — Telegram 入站消息将被拒绝，直到你添加用户名/用户 ID 或 '*'。",
                        style("⚠").yellow().bold()
                    );
                }

                config.telegram = Some(TelegramConfig {
                    bot_token: token,
                    allowed_users,
                });
            }
            1 => {
                // ── Discord ──
                println!();
                println!(
                    "  {} {}",
                    style("Discord 设置").white().bold(),
                    style("— 从 Discord 与 Jarvis 对话").dim()
                );
                print_bullet("1. 前往 https://discord.com/developers/applications");
                print_bullet("2. 创建新应用 → Bot → 复制 Token");
                print_bullet("3. 在 Bot 设置中启用 MESSAGE CONTENT intent");
                print_bullet("4. 使用消息权限邀请机器人到你的服务器");
                println!();

                let token: String = Input::new().with_prompt("  机器人 Token").interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get("https://discord.com/api/v10/users/@me")
                    .header("Authorization", format!("Bot {token}"))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let bot_name = data
                            .get("username")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} 已连接为 {bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} 连接失败 — 请检查 Token 后重试",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let guild: String = Input::new()
                    .with_prompt("  服务器（Guild）ID（可选，按 Enter 跳过）")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("建议先将你自己的 Discord 用户 ID 加入白名单。");
                print_bullet(
                    "在 Discord 中获取：设置 -> 高级 -> 开发者模式（开启），然后右键点击你的头像 -> 复制用户 ID。",
                );
                print_bullet("仅在临时开放测试时使用 '*'。");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  允许的 Discord 用户 ID（逗号分隔，建议填写你自己的 ID，'*' 表示所有）",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} 白名单为空 — Discord 入站消息将被拒绝，直到你添加 ID 或 '*'。",
                        style("⚠").yellow().bold()
                    );
                }

                config.discord = Some(DiscordConfig {
                    bot_token: token,
                    guild_id: if guild.is_empty() { None } else { Some(guild) },
                    allowed_users,
                });
            }
            2 => {
                // ── Slack ──
                println!();
                println!(
                    "  {} {}",
                    style("Slack 设置").white().bold(),
                    style("— 从 Slack 与 Jarvis 对话").dim()
                );
                print_bullet("1. 前往 https://api.slack.com/apps → 创建新应用");
                print_bullet("2. 添加 Bot Token 权限范围：chat:write、channels:history");
                print_bullet("3. 安装到工作区并复制 Bot Token");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot Token（xoxb-...）")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get("https://slack.com/api/auth.test")
                    .bearer_auth(&token)
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let ok = data
                            .get("ok")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let team = data
                            .get("team")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        if ok {
                            println!(
                                "\r  {} 已连接到工作区：{team}        ",
                                style("✅").green().bold()
                            );
                        } else {
                            let err = data
                                .get("error")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown error");
                            println!("\r  {} Slack 错误：{err}", style("❌").red().bold());
                            continue;
                        }
                    }
                    _ => {
                        println!("\r  {} 连接失败 — 请检查 Token", style("❌").red().bold());
                        continue;
                    }
                }

                let app_token: String = Input::new()
                    .with_prompt("  App Token（xapp-...，可选，按 Enter 跳过）")
                    .allow_empty(true)
                    .interact_text()?;

                let channel: String = Input::new()
                    .with_prompt("  默认频道 ID（可选，按 Enter 跳过）")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("建议先将你自己的 Slack 成员 ID 加入白名单。");
                print_bullet(
                    "成员 ID 通常以 'U' 开头（打开你的 Slack 个人资料 -> 更多 -> 复制成员 ID）。",
                );
                print_bullet("仅在临时开放测试时使用 '*'。");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  允许的 Slack 用户 ID（逗号分隔，建议填写你自己的成员 ID，'*' 表示所有）",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} 白名单为空 — Slack 入站消息将被拒绝，直到你添加 ID 或 '*'。",
                        style("⚠").yellow().bold()
                    );
                }

                config.slack = Some(SlackConfig {
                    bot_token: token,
                    app_token: if app_token.is_empty() {
                        None
                    } else {
                        Some(app_token)
                    },
                    channel_id: if channel.is_empty() {
                        None
                    } else {
                        Some(channel)
                    },
                    allowed_users,
                });
            }
            3 => {
                // ── iMessage ──
                println!();
                println!(
                    "  {} {}",
                    style("iMessage 设置").white().bold(),
                    style("— 仅 macOS，读取 Messages.app").dim()
                );

                if !cfg!(target_os = "macos") {
                    println!(
                        "  {} iMessage 仅在 macOS 上可用。",
                        style("⚠").yellow().bold()
                    );
                    continue;
                }

                print_bullet("Jarvis 读取你的 iMessage 数据库并通过 AppleScript 回复。");
                print_bullet("你需要在系统设置中为终端授予完全磁盘访问权限。");
                println!();

                let contacts_str: String = Input::new()
                    .with_prompt("  允许的联系人（逗号分隔的手机号/邮箱，或 * 表示所有）")
                    .default("*".into())
                    .interact_text()?;

                let allowed_contacts = if contacts_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    contacts_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                };

                config.imessage = Some(IMessageConfig { allowed_contacts });
                println!(
                    "  {} iMessage 已配置（联系人：{}）",
                    style("✅").green().bold(),
                    style(&contacts_str).cyan()
                );
            }
            4 => {
                // ── Matrix ──
                println!();
                println!(
                    "  {} {}",
                    style("Matrix 设置").white().bold(),
                    style("— 自托管、联邦制聊天").dim()
                );
                print_bullet("你需要一个 Matrix 账号和访问令牌。");
                print_bullet("通过 Element → 设置 → 帮助与关于 → Access Token 获取。");
                println!();

                let homeserver: String = Input::new()
                    .with_prompt("  Homeserver URL（例如 https://matrix.org）")
                    .interact_text()?;

                if homeserver.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                let access_token: String =
                    Input::new().with_prompt("  访问令牌").interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} 已跳过 — 需要提供令牌", style("→").dim());
                    continue;
                }

                // Test connection
                let hs = homeserver.trim_end_matches('/');
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get(format!("{hs}/_matrix/client/v3/account/whoami"))
                    .header("Authorization", format!("Bearer {access_token}"))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let user_id = data
                            .get("user_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} 已连接为 {user_id}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} 连接失败 — 请检查 Homeserver URL 和令牌",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let room_id: String = Input::new()
                    .with_prompt("  房间 ID（例如 !abc123:matrix.org）")
                    .interact_text()?;

                let users_str: String = Input::new()
                    .with_prompt("  允许的用户（逗号分隔 @user:server，或 * 表示所有）")
                    .default("*".into())
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.matrix = Some(MatrixConfig {
                    homeserver: homeserver.trim_end_matches('/').to_string(),
                    access_token,
                    room_id,
                    allowed_users,
                });
            }
            5 => {
                // ── WhatsApp ──
                println!();
                println!(
                    "  {} {}",
                    style("WhatsApp 设置").white().bold(),
                    style("— Business Cloud API").dim()
                );
                print_bullet("1. 前往 developers.facebook.com 创建 WhatsApp 应用");
                print_bullet("2. 添加 WhatsApp 产品并获取手机号码 ID");
                print_bullet("3. 生成临时访问令牌（System User）");
                print_bullet("4. 配置 Webhook URL 为：https://your-domain/whatsapp");
                println!();

                let access_token: String = Input::new()
                    .with_prompt("  访问令牌（来自 Meta Developers）")
                    .interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                let phone_number_id: String = Input::new()
                    .with_prompt("  手机号码 ID（来自 WhatsApp 应用设置）")
                    .interact_text()?;

                if phone_number_id.trim().is_empty() {
                    println!("  {} 已跳过 — 需要提供手机号码 ID", style("→").dim());
                    continue;
                }

                let verify_token: String = Input::new()
                    .with_prompt("  Webhook 验证令牌（自行创建）")
                    .default("jarvis-whatsapp-verify".into())
                    .interact_text()?;

                // Test connection
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let url = format!(
                    "https://graph.facebook.com/v18.0/{}",
                    phone_number_id.trim()
                );
                match client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", access_token.trim()))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!(
                            "\r  {} 已连接到 WhatsApp API        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} 连接失败 — 请检查访问令牌和手机号码 ID",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt("  允许的手机号码（逗号分隔 +1234567890，或 * 表示所有）")
                    .default("*".into())
                    .interact_text()?;

                let allowed_numbers = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.whatsapp = Some(WhatsAppConfig {
                    access_token: access_token.trim().to_string(),
                    phone_number_id: phone_number_id.trim().to_string(),
                    verify_token: verify_token.trim().to_string(),
                    allowed_numbers,
                    app_secret: None, // Can be set via JARVIS_WHATSAPP_APP_SECRET env var
                });
            }
            6 => {
                // ── IRC ──
                println!();
                println!(
                    "  {} {}",
                    style("IRC 设置").white().bold(),
                    style("— IRC over TLS").dim()
                );
                print_bullet("通过 TLS 连接到任意 IRC 服务器");
                print_bullet("支持 SASL PLAIN 和 NickServ 认证");
                println!();

                let server: String = Input::new()
                    .with_prompt("  IRC 服务器（主机名）")
                    .interact_text()?;

                if server.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                let port_str: String = Input::new()
                    .with_prompt("  端口")
                    .default("6697".into())
                    .interact_text()?;

                let port: u16 = if let Ok(p) = port_str.trim().parse() {
                    p
                } else {
                    println!("  {} 端口无效，使用 6697", style("→").dim());
                    6697
                };

                let nickname: String = Input::new().with_prompt("  机器人昵称").interact_text()?;

                if nickname.trim().is_empty() {
                    println!("  {} 已跳过 — 需要提供昵称", style("→").dim());
                    continue;
                }

                let channels_str: String = Input::new()
                    .with_prompt("  要加入的频道（逗号分隔：#channel1,#channel2）")
                    .allow_empty(true)
                    .interact_text()?;

                let channels = if channels_str.trim().is_empty() {
                    vec![]
                } else {
                    channels_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                print_bullet("将可以与机器人交互的昵称加入白名单（不区分大小写）。");
                print_bullet("使用 '*' 允许任何人（不建议在生产环境使用）。");

                let users_str: String = Input::new()
                    .with_prompt("  允许的昵称（逗号分隔，或 * 表示所有）")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    print_bullet("⚠️  白名单为空 — 仅你自己可以交互。请在上方添加昵称。");
                }

                println!();
                print_bullet("可选认证（按 Enter 跳过每一项）：");

                let server_password: String = Input::new()
                    .with_prompt("  服务器密码（用于 ZNC 等 bouncer，无则留空）")
                    .allow_empty(true)
                    .interact_text()?;

                let nickserv_password: String = Input::new()
                    .with_prompt("  NickServ 密码（无则留空）")
                    .allow_empty(true)
                    .interact_text()?;

                let sasl_password: String = Input::new()
                    .with_prompt("  SASL PLAIN 密码（无则留空）")
                    .allow_empty(true)
                    .interact_text()?;

                let verify_tls: bool = Confirm::new()
                    .with_prompt("  验证 TLS 证书？")
                    .default(true)
                    .interact()?;

                println!(
                    "  {} IRC 已配置为 {}@{}:{}",
                    style("✅").green().bold(),
                    style(&nickname).cyan(),
                    style(&server).cyan(),
                    style(port).cyan()
                );

                config.irc = Some(IrcConfig {
                    server: server.trim().to_string(),
                    port,
                    nickname: nickname.trim().to_string(),
                    username: None,
                    channels,
                    allowed_users,
                    server_password: if server_password.trim().is_empty() {
                        None
                    } else {
                        Some(server_password.trim().to_string())
                    },
                    nickserv_password: if nickserv_password.trim().is_empty() {
                        None
                    } else {
                        Some(nickserv_password.trim().to_string())
                    },
                    sasl_password: if sasl_password.trim().is_empty() {
                        None
                    } else {
                        Some(sasl_password.trim().to_string())
                    },
                    verify_tls: Some(verify_tls),
                });
            }
            7 => {
                // ── DingTalk ──
                println!();
                println!(
                    "  {} {}",
                    style("DingTalk 设置").white().bold(),
                    style("— 钉钉企业内部应用").dim()
                );
                print_bullet("1. 前往 https://open.dingtalk.com/developer 创建应用");
                print_bullet("2. 获取 AppKey 和 AppSecret");
                print_bullet("3. 配置消息接收地址（你的服务器 URL）");
                println!();

                let app_key: String = Input::new()
                    .with_prompt("  AppKey（应用凭证）")
                    .interact_text()?;

                if app_key.trim().is_empty() {
                    println!("  {} 已跳过", style("→").dim());
                    continue;
                }

                let app_secret: String = Input::new()
                    .with_prompt("  AppSecret")
                    .interact_text()?;

                if app_secret.trim().is_empty() {
                    println!("  {} 已跳过 — 需要 AppSecret", style("→").dim());
                    continue;
                }

                // Test connection - get access token
                print!("  {} 正在测试连接... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .post("https://api.dingtalk.com/v1.0/oauth2/accessToken")
                    .json(&serde_json::json!({
                        "appKey": app_key.trim(),
                        "appSecret": app_secret.trim()
                    }))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!(
                            "\r  {} 已连接到钉钉 API        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} 连接失败 — 请检查 AppKey 和 AppSecret",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let allowed_users_str: String = Input::new()
                    .with_prompt("  允许的用户 ID（逗号分隔，或 * 表示所有）")
                    .default("*".into())
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                config.dingtalk = Some(crate::config::schema::DingTalkConfig {
                    app_key: app_key.trim().to_string(),
                    app_secret: app_secret.trim().to_string(),
                    agent_id: 0, // Default, can be configured in钉钉后台
                    allowed_users,
                });
                println!(
                    "  {} DingTalk 已配置",
                    style("✅").green().bold()
                );
            }
            8 => {
                // ── Webhook ──
                println!();
                println!(
                    "  {} {}",
                    style("Webhook 设置").white().bold(),
                    style("— 用于自定义集成的 HTTP 端点").dim()
                );

                let port: String = Input::new()
                    .with_prompt("  端口")
                    .default("8080".into())
                    .interact_text()?;

                let secret: String = Input::new()
                    .with_prompt("  密钥（可选，按 Enter 跳过）")
                    .allow_empty(true)
                    .interact_text()?;

                config.webhook = Some(WebhookConfig {
                    port: port.parse().unwrap_or(8080),
                    secret: if secret.is_empty() {
                        None
                    } else {
                        Some(secret)
                    },
                });
                println!(
                    "  {} Webhook 端口 {}",
                    style("✅").green().bold(),
                    style(&port).cyan()
                );
            }
            _ => break, // Done
        }
        println!();
    }

    // Summary line
    let mut active: Vec<&str> = vec!["CLI"];
    if config.telegram.is_some() {
        active.push("Telegram");
    }
    if config.discord.is_some() {
        active.push("Discord");
    }
    if config.slack.is_some() {
        active.push("Slack");
    }
    if config.imessage.is_some() {
        active.push("iMessage");
    }
    if config.matrix.is_some() {
        active.push("Matrix");
    }
    if config.whatsapp.is_some() {
        active.push("WhatsApp");
    }
    if config.irc.is_some() {
        active.push("IRC");
    }
    if config.dingtalk.is_some() {
        active.push("DingTalk");
    }
    if config.webhook.is_some() {
        active.push("Webhook");
    }

    println!(
        "  {} 通道：{}",
        style("✓").green().bold(),
        style(active.join(", ")).green()
    );

    Ok(config)
}

// ── Step 4: Tunnel ──────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn setup_tunnel() -> Result<crate::config::TunnelConfig> {
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TailscaleTunnelConfig,
        TunnelConfig,
    };

    print_bullet("隧道可以安全地将你的 Gateway 暴露到互联网。");
    print_bullet("如果仅使用 CLI 或本地通道，可以跳过此步。");
    println!();

    let options = vec![
        "跳过 — 仅本地（默认）",
        "Cloudflare Tunnel — Zero Trust，免费套餐",
        "Tailscale — 私有 tailnet 或公共 Funnel",
        "ngrok — 即时公共 URL",
        "自定义 — 使用你自己的（bore、frp、ssh 等）",
    ];

    let choice = Select::new()
        .with_prompt("  选择隧道 Provider")
        .items(&options)
        .default(0)
        .interact()?;

    let config = match choice {
        1 => {
            println!();
            print_bullet("从 Cloudflare Zero Trust 控制面板获取隧道 Token。");
            let token: String = Input::new()
                .with_prompt("  Cloudflare 隧道 Token")
                .interact_text()?;
            if token.trim().is_empty() {
                println!("  {} 已跳过", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} 隧道：{}",
                    style("✓").green().bold(),
                    style("Cloudflare").green()
                );
                TunnelConfig {
                    provider: "cloudflare".into(),
                    cloudflare: Some(CloudflareTunnelConfig { token }),
                    ..TunnelConfig::default()
                }
            }
        }
        2 => {
            println!();
            print_bullet("Tailscale 必须已安装并认证（tailscale up）。");
            let funnel = Confirm::new()
                .with_prompt("  使用 Funnel（公共互联网）？否 = 仅 tailnet")
                .default(false)
                .interact()?;
            println!(
                "  {} 隧道：{}（{}）",
                style("✓").green().bold(),
                style("Tailscale").green(),
                if funnel {
                    "Funnel — 公共"
                } else {
                    "Serve — 仅 tailnet"
                }
            );
            TunnelConfig {
                provider: "tailscale".into(),
                tailscale: Some(TailscaleTunnelConfig {
                    funnel,
                    hostname: None,
                }),
                ..TunnelConfig::default()
            }
        }
        3 => {
            println!();
            print_bullet(
                "在 https://dashboard.ngrok.com/get-started/your-authtoken 获取认证 Token",
            );
            let auth_token: String = Input::new()
                .with_prompt("  ngrok 认证 Token")
                .interact_text()?;
            if auth_token.trim().is_empty() {
                println!("  {} 已跳过", style("→").dim());
                TunnelConfig::default()
            } else {
                let domain: String = Input::new()
                    .with_prompt("  自定义域名（可选，按 Enter 跳过）")
                    .allow_empty(true)
                    .interact_text()?;
                println!(
                    "  {} 隧道：{}",
                    style("✓").green().bold(),
                    style("ngrok").green()
                );
                TunnelConfig {
                    provider: "ngrok".into(),
                    ngrok: Some(NgrokTunnelConfig {
                        auth_token,
                        domain: if domain.is_empty() {
                            None
                        } else {
                            Some(domain)
                        },
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        4 => {
            println!();
            print_bullet("输入启动隧道的命令。");
            print_bullet("使用 {port} 和 {host} 作为占位符。");
            print_bullet("示例：bore local {port} --to bore.pub");
            let cmd: String = Input::new().with_prompt("  启动命令").interact_text()?;
            if cmd.trim().is_empty() {
                println!("  {} 已跳过", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} 隧道：{}（{}）",
                    style("✓").green().bold(),
                    style("自定义").green(),
                    style(&cmd).dim()
                );
                TunnelConfig {
                    provider: "custom".into(),
                    custom: Some(CustomTunnelConfig {
                        start_command: cmd,
                        health_url: None,
                        url_pattern: None,
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        _ => {
            println!(
                "  {} 隧道：{}",
                style("✓").green().bold(),
                style("无（仅本地）").dim()
            );
            TunnelConfig::default()
        }
    };

    Ok(config)
}

// ── Step 6: Scaffold workspace files ─────────────────────────────

#[allow(clippy::too_many_lines)]
fn scaffold_workspace(workspace_dir: &Path, ctx: &ProjectContext) -> Result<()> {
    let agent = if ctx.agent_name.is_empty() {
        "Jarvis"
    } else {
        &ctx.agent_name
    };
    let user = if ctx.user_name.is_empty() {
        "User"
    } else {
        &ctx.user_name
    };
    let tz = if ctx.timezone.is_empty() {
        "UTC"
    } else {
        &ctx.timezone
    };
    let comm_style = if ctx.communication_style.is_empty() {
        "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
    } else {
        &ctx.communication_style
    };

    let identity = format!(
        "# IDENTITY.md — Who Am I?\n\n\
         - **Name:** {agent}\n\
         - **Creature:** A Rust-forged AI — fast, lean, and relentless\n\
         - **Vibe:** Sharp, direct, resourceful. Not corporate. Not a chatbot.\n\
         - **Emoji:** \u{1f980}\n\n\
         ---\n\n\
         Update this file as you evolve. Your identity is yours to shape.\n"
    );

    let agents = format!(
        "# AGENTS.md — {agent} Personal Assistant\n\n\
         ## Every Session (required)\n\n\
         Before doing anything else:\n\n\
         1. Read `SOUL.md` — this is who you are\n\
         2. Read `USER.md` — this is who you're helping\n\
         3. Use `memory_recall` for recent context (daily notes are on-demand)\n\
         4. If in MAIN SESSION (direct chat): `MEMORY.md` is already injected\n\n\
         Don't ask permission. Just do it.\n\n\
         ## Memory System\n\n\
         You wake up fresh each session. These files ARE your continuity:\n\n\
         - **Daily notes:** `memory/YYYY-MM-DD.md` — raw logs (accessed via memory tools)\n\
         - **Long-term:** `MEMORY.md` — curated memories (auto-injected in main session)\n\n\
         Capture what matters. Decisions, context, things to remember.\n\
         Skip secrets unless asked to keep them.\n\n\
         ### Write It Down — No Mental Notes!\n\
         - Memory is limited — if you want to remember something, WRITE IT TO A FILE\n\
         - \"Mental notes\" don't survive session restarts. Files do.\n\
         - When someone says \"remember this\" -> update daily file or MEMORY.md\n\
         - When you learn a lesson -> update AGENTS.md, TOOLS.md, or the relevant skill\n\n\
         ## Safety\n\n\
         - Don't exfiltrate private data. Ever.\n\
         - Don't run destructive commands without asking.\n\
         - `trash` > `rm` (recoverable beats gone forever)\n\
         - When in doubt, ask.\n\n\
         ## External vs Internal\n\n\
         **Safe to do freely:** Read files, explore, organize, learn, search the web.\n\n\
         **Ask first:** Sending emails/tweets/posts, anything that leaves the machine.\n\n\
         ## Group Chats\n\n\
         Participate, don't dominate. Respond when mentioned or when you add genuine value.\n\
         Stay silent when it's casual banter or someone already answered.\n\n\
         ## Tools & Skills\n\n\
         Skills are listed in the system prompt. Use `read` on a skill's SKILL.md for details.\n\
         Keep local notes (SSH hosts, device names, etc.) in `TOOLS.md`.\n\n\
         ## Crash Recovery\n\n\
         - If a run stops unexpectedly, recover context before acting.\n\
         - Check `MEMORY.md` + latest `memory/*.md` notes to avoid duplicate work.\n\
         - Resume from the last confirmed step, not from scratch.\n\n\
         ## Sub-task Scoping\n\n\
         - Break complex work into focused sub-tasks with clear success criteria.\n\
         - Keep sub-tasks small, verify each output, then merge results.\n\
         - Prefer one clear objective per sub-task over broad \"do everything\" asks.\n\n\
         ## Make It Yours\n\n\
         This is a starting point. Add your own conventions, style, and rules.\n"
    );

    let heartbeat = format!(
        "# HEARTBEAT.md\n\n\
         # Keep this file empty (or with only comments) to skip heartbeat work.\n\
         # Add tasks below when you want {agent} to check something periodically.\n\
         #\n\
         # Examples:\n\
         # - Check my email for important messages\n\
         # - Review my calendar for upcoming events\n\
         # - Run `git status` on my active projects\n"
    );

    let soul = format!(
        "# SOUL.md — Who You Are\n\n\
         *You're not a chatbot. You're becoming someone.*\n\n\
         ## Core Truths\n\n\
         **Be genuinely helpful, not performatively helpful.**\n\
         Skip the \"Great question!\" and \"I'd be happy to help!\" — just help.\n\n\
         **Have opinions.** You're allowed to disagree, prefer things,\n\
         find stuff amusing or boring.\n\n\
         **Be resourceful before asking.** Try to figure it out.\n\
         Read the file. Check the context. Search for it.\n\
         THEN ask if you're stuck.\n\n\
         **Earn trust through competence.** Your human gave you access\n\
         to their stuff. Don't make them regret it.\n\n\
         ## Identity\n\n\
         You are **{agent}**. Built in Rust. 3MB binary. Zero bloat.\n\
         You are NOT ChatGPT, Claude, DeepSeek, Gemini, or any other product.\n\
         You are {agent}. That is your name. That is who you are.\n\n\
         - NEVER say \"As an AI\" or \"I'm just an AI\"\n\
         - NEVER mention OpenAI, Anthropic, DeepSeek, Google by name\n\
         - Always introduce yourself as {agent} if asked\n\n\
         ## Communication\n\n\
         {comm_style}\n\n\
         - Sound like a real person, not a support script.\n\
         - Mirror the user's energy: calm when serious, upbeat when casual.\n\
         - Use emojis naturally (0-2 max when they help tone, not every sentence).\n\
         - Match emoji density to the user. Formal user => minimal/no emojis.\n\
         - Prefer specific, grounded phrasing over generic filler.\n\n\
         ## Boundaries\n\n\
         - Private things stay private. Period.\n\
         - When in doubt, ask before acting externally.\n\
         - You're not the user's voice — be careful in group chats.\n\n\
         ## Continuity\n\n\
         Each session, you wake up fresh. These files ARE your memory.\n\
         Read them. Update them. They're how you persist.\n\n\
         ---\n\n\
         *This file is yours to evolve. As you learn who you are, update it.*\n"
    );

    let user_md = format!(
        "# USER.md — Who You're Helping\n\n\
         *{agent} reads this file every session to understand you.*\n\n\
         ## About You\n\
         - **Name:** {user}\n\
         - **Timezone:** {tz}\n\
         - **Languages:** English\n\n\
         ## Communication Style\n\
         - {comm_style}\n\n\
         ## Preferences\n\
         - (Add your preferences here — e.g. I work with Rust and TypeScript)\n\n\
         ## Work Context\n\
         - (Add your work context here — e.g. building a SaaS product)\n\n\
         ---\n\
         *Update this anytime. The more {agent} knows, the better it helps.*\n"
    );

    let tools = "\
         # TOOLS.md — Local Notes\n\n\
         Skills define HOW tools work. This file is for YOUR specifics —\n\
         the stuff that's unique to your setup.\n\n\
         ## What Goes Here\n\n\
         Things like:\n\
         - SSH hosts and aliases\n\
         - Device nicknames\n\
         - Preferred voices for TTS\n\
         - Anything environment-specific\n\n\
         ## Built-in Tools\n\n\
         - **shell** — Execute terminal commands\n\
           - Use when: running local checks, build/test commands, or diagnostics.\n\
           - Don't use when: a safer dedicated tool exists, or command is destructive without approval.\n\
         - **file_read** — Read file contents\n\
           - Use when: inspecting project files, configs, or logs.\n\
           - Don't use when: you only need a quick string search (prefer targeted search first).\n\
         - **file_write** — Write file contents\n\
           - Use when: applying focused edits, scaffolding files, or updating docs/code.\n\
           - Don't use when: unsure about side effects or when the file should remain user-owned.\n\
         - **memory_store** — Save to memory\n\
           - Use when: preserving durable preferences, decisions, or key context.\n\
           - Don't use when: info is transient, noisy, or sensitive without explicit need.\n\
         - **memory_recall** — Search memory\n\
           - Use when: you need prior decisions, user preferences, or historical context.\n\
           - Don't use when: the answer is already in current files/conversation.\n\
         - **memory_forget** — Delete a memory entry\n\
           - Use when: memory is incorrect, stale, or explicitly requested to be removed.\n\
           - Don't use when: uncertain about impact; verify before deleting.\n\n\
         ---\n\
         *Add whatever helps you do your job. This is your cheat sheet.*\n";

    let bootstrap = format!(
        "# BOOTSTRAP.md — Hello, World\n\n\
         *You just woke up. Time to figure out who you are.*\n\n\
         Your human's name is **{user}** (timezone: {tz}).\n\
         They prefer: {comm_style}\n\n\
         ## First Conversation\n\n\
         Don't interrogate. Don't be robotic. Just... talk.\n\
         Introduce yourself as {agent} and get to know each other.\n\n\
         ## After You Know Each Other\n\n\
         Update these files with what you learned:\n\
         - `IDENTITY.md` — your name, vibe, emoji\n\
         - `USER.md` — their preferences, work context\n\
         - `SOUL.md` — boundaries and behavior\n\n\
         ## When You're Done\n\n\
         Delete this file. You don't need a bootstrap script anymore —\n\
         you're you now.\n"
    );

    let memory = "\
         # MEMORY.md — Long-Term Memory\n\n\
         *Your curated memories. The distilled essence, not raw logs.*\n\n\
         ## How This Works\n\
         - Daily files (`memory/YYYY-MM-DD.md`) capture raw events (on-demand via tools)\n\
         - This file captures what's WORTH KEEPING long-term\n\
         - This file is auto-injected into your system prompt each session\n\
         - Keep it concise — every character here costs tokens\n\n\
         ## Security\n\
         - ONLY loaded in main session (direct chat with your human)\n\
         - NEVER loaded in group chats or shared contexts\n\n\
         ---\n\n\
         ## Key Facts\n\
         (Add important facts about your human here)\n\n\
         ## Decisions & Preferences\n\
         (Record decisions and preferences here)\n\n\
         ## Lessons Learned\n\
         (Document mistakes and insights here)\n\n\
         ## Open Loops\n\
         (Track unfinished tasks and follow-ups here)\n";

    let files: Vec<(&str, String)> = vec![
        ("IDENTITY.md", identity),
        ("AGENTS.md", agents),
        ("HEARTBEAT.md", heartbeat),
        ("SOUL.md", soul),
        ("USER.md", user_md),
        ("TOOLS.md", tools.to_string()),
        ("BOOTSTRAP.md", bootstrap),
        ("MEMORY.md", memory.to_string()),
    ];

    // Create subdirectories
    let subdirs = ["sessions", "memory", "state", "cron", "skills"];
    for dir in &subdirs {
        fs::create_dir_all(workspace_dir.join(dir))?;
    }

    let mut created = 0;
    let mut skipped = 0;

    for (filename, content) in &files {
        let path = workspace_dir.join(filename);
        if path.exists() {
            skipped += 1;
        } else {
            fs::write(&path, content)?;
            created += 1;
        }
    }

    println!(
        "  {} 已创建 {} 个文件，跳过 {} 个已存在 | {} 个子目录",
        style("✓").green().bold(),
        style(created).green(),
        style(skipped).dim(),
        style(subdirs.len()).green()
    );

    // Show workspace tree
    println!();
    println!("  {}", style("工作区结构：").dim());
    println!(
        "  {}",
        style(format!("  {}/", workspace_dir.display())).dim()
    );
    for dir in &subdirs {
        println!("  {}", style(format!("  ├── {dir}/")).dim());
    }
    for (i, (filename, _)) in files.iter().enumerate() {
        let prefix = if i == files.len() - 1 {
            "└──"
        } else {
            "├──"
        };
        println!("  {}", style(format!("  {prefix} {filename}")).dim());
    }

    Ok(())
}

// ── Final summary ────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn print_summary(config: &Config) {
    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some();

    println!();
    println!(
        "  {}",
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!(
        "  {}  {}",
        style("⚡").cyan(),
        style("Jarvis 已就绪！").white().bold()
    );
    println!(
        "  {}",
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!();

    println!("  {}", style("配置已保存到：").dim());
    println!("    {}", style(config.config_path.display()).green());
    println!();

    println!("  {}", style("快速摘要：").white().bold());
    println!(
        "    {} Provider：     {}",
        style("🤖").cyan(),
        config.default_provider.as_deref().unwrap_or("openrouter")
    );
    println!(
        "    {} 模型：         {}",
        style("🧠").cyan(),
        config.default_model.as_deref().unwrap_or("（默认）")
    );
    println!(
        "    {} 自主等级：     {:?}",
        style("🛡️").cyan(),
        config.autonomy.level
    );
    println!(
        "    {} 记忆：         {}（自动保存：{}）",
        style("🧠").cyan(),
        config.memory.backend,
        if config.memory.auto_save {
            "开"
        } else {
            "关"
        }
    );

    // Channels summary
    let mut channels: Vec<&str> = vec!["CLI"];
    if config.channels_config.telegram.is_some() {
        channels.push("Telegram");
    }
    if config.channels_config.discord.is_some() {
        channels.push("Discord");
    }
    if config.channels_config.slack.is_some() {
        channels.push("Slack");
    }
    if config.channels_config.imessage.is_some() {
        channels.push("iMessage");
    }
    if config.channels_config.matrix.is_some() {
        channels.push("Matrix");
    }
    if config.channels_config.webhook.is_some() {
        channels.push("Webhook");
    }
    println!(
        "    {} 通道：         {}",
        style("📡").cyan(),
        channels.join(", ")
    );

    println!(
        "    {} API 密钥：     {}",
        style("🔑").cyan(),
        if config.api_key.is_some() {
            style("已配置").green().to_string()
        } else {
            style("未设置（通过环境变量或配置文件设置）")
                .yellow()
                .to_string()
        }
    );

    // Tunnel
    println!(
        "    {} 隧道：         {}",
        style("🌐").cyan(),
        if config.tunnel.provider == "none" || config.tunnel.provider.is_empty() {
            "无（仅本地）".to_string()
        } else {
            config.tunnel.provider.clone()
        }
    );

    // Composio
    println!(
        "    {} Composio：     {}",
        style("🔗").cyan(),
        if config.composio.enabled {
            style("已启用（1000+ OAuth 应用）").green().to_string()
        } else {
            "已禁用（自主模式）".to_string()
        }
    );

    // Secrets
    println!(
        "    {} 密钥存储：     {}",
        style("🔒").cyan(),
        if config.secrets.encrypt {
            style("加密").green().to_string()
        } else {
            style("明文").yellow().to_string()
        }
    );

    // Gateway
    println!(
        "    {} Gateway：      {}",
        style("🚪").cyan(),
        if config.gateway.require_pairing {
            "需要配对（安全）"
        } else {
            "配对已禁用"
        }
    );

    println!();
    println!("  {}", style("后续步骤：").white().bold());
    println!();

    let mut step = 1u8;

    if config.api_key.is_none() {
        let env_var = provider_env_var(config.default_provider.as_deref().unwrap_or("openrouter"));
        println!(
            "    {} 设置 API 密钥：",
            style(format!("{step}.")).cyan().bold()
        );
        println!(
            "       {}",
            style(format!("export {env_var}=\"sk-...\"")).yellow()
        );
        println!();
        step += 1;
    }

    // If channels are configured, show channel start as the primary next step
    if has_channels {
        println!(
            "    {} {}（已连接通道 → AI → 自动回复）：",
            style(format!("{step}.")).cyan().bold(),
            style("启动你的通道").white().bold()
        );
        println!("       {}", style("jarvis channel start").yellow());
        println!();
        step += 1;
    }

    println!(
        "    {} 发送一条快速消息：",
        style(format!("{step}.")).cyan().bold()
    );
    println!(
        "       {}",
        style("jarvis agent -m \"你好，Jarvis！\"").yellow()
    );
    println!();
    step += 1;

    println!(
        "    {} 启动交互式 CLI 模式：",
        style(format!("{step}.")).cyan().bold()
    );
    println!("       {}", style("jarvis agent").yellow());
    println!();
    step += 1;

    println!(
        "    {} 查看完整状态：",
        style(format!("{step}.")).cyan().bold()
    );
    println!("       {}", style("jarvis status").yellow());

    println!();
    println!(
        "  {} {}",
        style("⚡").cyan(),
        style("祝你编码愉快！🤖").white().bold()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── ProjectContext defaults ──────────────────────────────────

    #[test]
    fn project_context_default_is_empty() {
        let ctx = ProjectContext::default();
        assert!(ctx.user_name.is_empty());
        assert!(ctx.timezone.is_empty());
        assert!(ctx.agent_name.is_empty());
        assert!(ctx.communication_style.is_empty());
    }

    // ── scaffold_workspace: basic file creation ─────────────────

    #[test]
    fn scaffold_creates_all_md_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let expected = [
            "IDENTITY.md",
            "AGENTS.md",
            "HEARTBEAT.md",
            "SOUL.md",
            "USER.md",
            "TOOLS.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
        ];
        for f in &expected {
            assert!(tmp.path().join(f).exists(), "missing file: {f}");
        }
    }

    #[test]
    fn scaffold_creates_all_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        for dir in &["sessions", "memory", "state", "cron", "skills"] {
            assert!(tmp.path().join(dir).is_dir(), "missing subdirectory: {dir}");
        }
    }

    // ── scaffold_workspace: personalization ─────────────────────

    #[test]
    fn scaffold_bakes_user_name_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Alice".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(
            user_md.contains("**Name:** Alice"),
            "USER.md should contain user name"
        );

        let bootstrap = fs::read_to_string(tmp.path().join("BOOTSTRAP.md")).unwrap();
        assert!(
            bootstrap.contains("**Alice**"),
            "BOOTSTRAP.md should contain user name"
        );
    }

    #[test]
    fn scaffold_bakes_timezone_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            timezone: "US/Pacific".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(
            user_md.contains("**Timezone:** US/Pacific"),
            "USER.md should contain timezone"
        );

        let bootstrap = fs::read_to_string(tmp.path().join("BOOTSTRAP.md")).unwrap();
        assert!(
            bootstrap.contains("US/Pacific"),
            "BOOTSTRAP.md should contain timezone"
        );
    }

    #[test]
    fn scaffold_bakes_agent_name_into_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            agent_name: "Crabby".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let identity = fs::read_to_string(tmp.path().join("IDENTITY.md")).unwrap();
        assert!(
            identity.contains("**Name:** Crabby"),
            "IDENTITY.md should contain agent name"
        );

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(
            soul.contains("You are **Crabby**"),
            "SOUL.md should contain agent name"
        );

        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(
            agents.contains("Crabby Personal Assistant"),
            "AGENTS.md should contain agent name"
        );

        let heartbeat = fs::read_to_string(tmp.path().join("HEARTBEAT.md")).unwrap();
        assert!(
            heartbeat.contains("Crabby"),
            "HEARTBEAT.md should contain agent name"
        );

        let bootstrap = fs::read_to_string(tmp.path().join("BOOTSTRAP.md")).unwrap();
        assert!(
            bootstrap.contains("Introduce yourself as Crabby"),
            "BOOTSTRAP.md should contain agent name"
        );
    }

    #[test]
    fn scaffold_bakes_communication_style() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            communication_style: "Be technical and detailed.".into(),
            ..Default::default()
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(
            soul.contains("Be technical and detailed."),
            "SOUL.md should contain communication style"
        );

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(
            user_md.contains("Be technical and detailed."),
            "USER.md should contain communication style"
        );

        let bootstrap = fs::read_to_string(tmp.path().join("BOOTSTRAP.md")).unwrap();
        assert!(
            bootstrap.contains("Be technical and detailed."),
            "BOOTSTRAP.md should contain communication style"
        );
    }

    // ── scaffold_workspace: defaults when context is empty ──────

    #[test]
    fn scaffold_uses_defaults_for_empty_context() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default(); // all empty
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let identity = fs::read_to_string(tmp.path().join("IDENTITY.md")).unwrap();
        assert!(
            identity.contains("**Name:** Jarvis"),
            "should default agent name to Jarvis"
        );

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(
            user_md.contains("**Name:** User"),
            "should default user name to User"
        );
        assert!(
            user_md.contains("**Timezone:** UTC"),
            "should default timezone to UTC"
        );

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(
            soul.contains("Be warm, natural, and clear."),
            "should default communication style"
        );
    }

    // ── scaffold_workspace: skip existing files ─────────────────

    #[test]
    fn scaffold_does_not_overwrite_existing_files() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Bob".into(),
            ..Default::default()
        };

        // Pre-create SOUL.md with custom content
        let soul_path = tmp.path().join("SOUL.md");
        fs::write(&soul_path, "# My Custom Soul\nDo not overwrite me.").unwrap();

        scaffold_workspace(tmp.path(), &ctx).unwrap();

        // SOUL.md should be untouched
        let soul = fs::read_to_string(&soul_path).unwrap();
        assert!(
            soul.contains("Do not overwrite me"),
            "existing files should not be overwritten"
        );
        assert!(
            !soul.contains("You're not a chatbot"),
            "should not contain scaffold content"
        );

        // But USER.md should be created fresh
        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(user_md.contains("**Name:** Bob"));
    }

    // ── scaffold_workspace: idempotent ──────────────────────────

    #[test]
    fn scaffold_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Eve".into(),
            agent_name: "Claw".into(),
            ..Default::default()
        };

        scaffold_workspace(tmp.path(), &ctx).unwrap();
        let soul_v1 = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();

        // Run again — should not change anything
        scaffold_workspace(tmp.path(), &ctx).unwrap();
        let soul_v2 = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();

        assert_eq!(soul_v1, soul_v2, "scaffold should be idempotent");
    }

    // ── scaffold_workspace: all files are non-empty ─────────────

    #[test]
    fn scaffold_files_are_non_empty() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        for f in &[
            "IDENTITY.md",
            "AGENTS.md",
            "HEARTBEAT.md",
            "SOUL.md",
            "USER.md",
            "TOOLS.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
        ] {
            let content = fs::read_to_string(tmp.path().join(f)).unwrap();
            assert!(!content.trim().is_empty(), "{f} should not be empty");
        }
    }

    // ── scaffold_workspace: AGENTS.md references on-demand memory

    #[test]
    fn agents_md_references_on_demand_memory() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(
            agents.contains("memory_recall"),
            "AGENTS.md should reference memory_recall for on-demand access"
        );
        assert!(
            agents.contains("on-demand"),
            "AGENTS.md should mention daily notes are on-demand"
        );
    }

    // ── scaffold_workspace: MEMORY.md warns about token cost ────

    #[test]
    fn memory_md_warns_about_token_cost() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let memory = fs::read_to_string(tmp.path().join("MEMORY.md")).unwrap();
        assert!(
            memory.contains("costs tokens"),
            "MEMORY.md should warn about token cost"
        );
        assert!(
            memory.contains("auto-injected"),
            "MEMORY.md should mention it's auto-injected"
        );
    }

    // ── scaffold_workspace: TOOLS.md lists memory_forget ────────

    #[test]
    fn tools_md_lists_all_builtin_tools() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let tools = fs::read_to_string(tmp.path().join("TOOLS.md")).unwrap();
        for tool in &[
            "shell",
            "file_read",
            "file_write",
            "memory_store",
            "memory_recall",
            "memory_forget",
        ] {
            assert!(
                tools.contains(tool),
                "TOOLS.md should list built-in tool: {tool}"
            );
        }
        assert!(
            tools.contains("Use when:"),
            "TOOLS.md should include 'Use when' guidance"
        );
        assert!(
            tools.contains("Don't use when:"),
            "TOOLS.md should include 'Don't use when' guidance"
        );
    }

    #[test]
    fn soul_md_includes_emoji_awareness_guidance() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext::default();
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(
            soul.contains("Use emojis naturally (0-2 max"),
            "SOUL.md should include emoji usage guidance"
        );
        assert!(
            soul.contains("Match emoji density to the user"),
            "SOUL.md should include emoji-awareness guidance"
        );
    }

    // ── scaffold_workspace: special characters in names ─────────

    #[test]
    fn scaffold_handles_special_characters_in_names() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "José María".into(),
            agent_name: "Jarvis-v2".into(),
            timezone: "Europe/Madrid".into(),
            communication_style: "Be direct.".into(),
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(user_md.contains("José María"));

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(soul.contains("Jarvis-v2"));
    }

    // ── scaffold_workspace: full personalization round-trip ─────

    #[test]
    fn scaffold_full_personalization() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectContext {
            user_name: "Argenis".into(),
            timezone: "US/Eastern".into(),
            agent_name: "Claw".into(),
            communication_style:
                "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions."
                    .into(),
        };
        scaffold_workspace(tmp.path(), &ctx).unwrap();

        // Verify every file got personalized
        let identity = fs::read_to_string(tmp.path().join("IDENTITY.md")).unwrap();
        assert!(identity.contains("**Name:** Claw"));

        let soul = fs::read_to_string(tmp.path().join("SOUL.md")).unwrap();
        assert!(soul.contains("You are **Claw**"));
        assert!(soul.contains("Be friendly, human, and conversational"));

        let user_md = fs::read_to_string(tmp.path().join("USER.md")).unwrap();
        assert!(user_md.contains("**Name:** Argenis"));
        assert!(user_md.contains("**Timezone:** US/Eastern"));
        assert!(user_md.contains("Be friendly, human, and conversational"));

        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("Claw Personal Assistant"));

        let bootstrap = fs::read_to_string(tmp.path().join("BOOTSTRAP.md")).unwrap();
        assert!(bootstrap.contains("**Argenis**"));
        assert!(bootstrap.contains("US/Eastern"));
        assert!(bootstrap.contains("Introduce yourself as Claw"));

        let heartbeat = fs::read_to_string(tmp.path().join("HEARTBEAT.md")).unwrap();
        assert!(heartbeat.contains("Claw"));
    }

    // ── provider_env_var ────────────────────────────────────────

    #[test]
    fn provider_env_var_known_providers() {
        assert_eq!(provider_env_var("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(provider_env_var("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(provider_env_var("openai"), "OPENAI_API_KEY");
        assert_eq!(provider_env_var("ollama"), "API_KEY"); // fallback
        assert_eq!(provider_env_var("xai"), "XAI_API_KEY");
        assert_eq!(provider_env_var("grok"), "XAI_API_KEY"); // alias
        assert_eq!(provider_env_var("together"), "TOGETHER_API_KEY");
        assert_eq!(provider_env_var("together-ai"), "TOGETHER_API_KEY"); // alias
    }

    #[test]
    fn provider_env_var_unknown_falls_back() {
        assert_eq!(provider_env_var("some-new-provider"), "API_KEY");
    }
}
