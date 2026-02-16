#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::return_self_not_must_use,
    dead_code
)]

use clap::Subcommand;
use serde::{Deserialize, Serialize};

pub mod agent;
pub mod channels;
pub mod config;
pub mod cron;
pub mod daemon;
pub mod doctor;
pub mod gateway;
pub mod health;
pub mod heartbeat;
pub mod integrations;
pub mod memory;
pub mod migration;
pub mod observability;
pub mod onboard;
pub mod providers;
pub mod runtime;
pub mod security;
pub mod service;
pub mod skills;
pub mod tools;
pub mod tui;
pub mod tunnel;
pub mod util;

pub use config::Config;

/// 服务管理子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceCommands {
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

/// 通道管理子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCommands {
    /// 列出所有已配置的通道
    List,
    /// 启动所有已配置的通道（在 main.rs 中异步处理）
    Start,
    /// 运行已配置通道的健康检查（在 main.rs 中异步处理）
    Doctor,
    /// 添加新的通道配置
    Add {
        /// 通道类型（telegram、discord、slack、whatsapp、matrix、imessage、email）
        channel_type: String,
        /// 可选的 JSON 配置
        config: String,
    },
    /// 移除通道配置
    Remove {
        /// 要移除的通道名称
        name: String,
    },
}

/// 技能管理子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCommands {
    /// 列出所有已安装的技能
    List,
    /// 从 URL 或本地路径安装新技能
    Install {
        /// 来源 URL 或本地路径
        source: String,
    },
    /// 移除已安装的技能
    Remove {
        /// 要移除的技能名称
        name: String,
    },
}

/// 迁移子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MigrateCommands {
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

/// 定时任务子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CronCommands {
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

/// 集成子命令
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationCommands {
    /// 显示指定集成的详细信息
    Info {
        /// 集成名称
        name: String,
    },
}
