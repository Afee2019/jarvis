# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Jarvis 是一个用 100% Rust 编写的高性能、零开销 AI 助手基础设施。目标是 <5MB 二进制、<10ms 启动、<5MB 内存占用。核心架构基于 trait 的可插拔设计——每个子系统都是一个 trait，实现可以通过配置切换而无需修改代码。

## 构建与开发命令

```bash
cargo build                    # 开发构建
cargo build --release          # 发布构建（~3.4MB，体积优化）
cargo test                     # 运行全部 1,017 个测试
cargo test -- <filter>         # 按名称过滤运行测试（如 cargo test -- security）
cargo test -- --nocapture      # 显示测试中的 println 输出
cargo fmt                      # 格式化代码
cargo clippy -- -D warnings    # Lint 检查（warnings 视为错误）
```

**Pre-push 钩子**（启用方式：`git config core.hooksPath .githooks`）会在每次 push 前自动运行 fmt、clippy、test。

## 核心架构

### Trait 体系

所有主要子系统均为 trait，新增集成只需实现对应 trait 并在工厂函数中注册：

| 子系统 | Trait | 目录 | 关键实现 |
|--------|-------|------|----------|
| LLM 提供商 | `Provider` | `src/providers/` | OpenRouter, Anthropic, OpenAI, Ollama, Gemini + `compatible.rs`（通用 OpenAI 兼容） |
| 消息通道 | `Channel` | `src/channels/` | CLI, Telegram, Discord, Slack, iMessage, Matrix, WhatsApp, IRC, Email |
| 工具能力 | `Tool` | `src/tools/` | shell, file_read, file_write, memory_store/recall/forget, browser_open, composio |
| 持久化 | `Memory` | `src/memory/` | SQLite（混合 FTS5 + 向量余弦相似度搜索）, Markdown, None |
| 可观测性 | `Observer` | `src/observability/` | Noop, Log, Multi |
| 安全策略 | `SecurityPolicy` | `src/security/` | 配对认证、沙箱、路径白名单、速率限制、文件系统作用域 |
| 运行时 | `RuntimeAdapter` | `src/runtime/` | Native（目前唯一支持） |
| 隧道 | `Tunnel` | `src/tunnel/` | None, Cloudflare, Tailscale, ngrok, Custom |

### 关键模块

- **`src/main.rs`**（12,000+ 行）：CLI 定义及所有子命令路由
- **`src/lib.rs`**：模块声明和公共类型导出
- **`src/agent/loop_.rs`**：核心 agent 循环（Provider 交互 → 工具执行 → 记忆召回 → 反馈）
- **`src/providers/reliable.rs`**：`ReliableProvider` 自动降级和指数退避重试
- **`src/memory/sqlite.rs`**（~1,400 行）：混合搜索引擎实现
- **`src/security/policy.rs`**：自主等级和路径验证
- **`src/config/schema.rs`**：TOML 配置 schema
- **`src/daemon/`**：长时间运行的自主运行时，包含 supervisor 组件管理

### 添加新集成的模式

1. 在对应目录创建 `your_impl.rs`，实现该子系统的 trait
2. 在 `mod.rs` 中注册到工厂函数（match 分支）
3. 在同文件底部添加 `#[cfg(test)] mod tests {}` 内联测试

## 代码规范

- **错误处理**：全部使用 `anyhow::Result<T>`，通过 `?` 传播错误，用 `.context()` 添加上下文。生产代码禁止 `unwrap()` / `expect()` / `panic!()`
- **日志**：使用 `tracing` 宏（`info!`, `warn!`, `error!`），附带结构化字段
- **异步**：所有 I/O 使用 tokio async/await，trait 使用 `#[async_trait]`
- **序列化**：配置使用 `serde` + TOML，API 数据使用 `serde_json`
- **Clippy 配置**：启用 `clippy::all` + `clippy::pedantic`，并通过 `#![allow(...)]` 在 `lib.rs` 中统一豁免特定规则
- **测试**：内联 `#[cfg(test)] mod tests {}` 放在每个文件底部，异步测试用 `#[tokio::test]`，临时目录用 `tempfile` crate
- **依赖哲学**：严格控制新依赖以保持小体积二进制（发布构建使用 `opt-level = "z"` + LTO + strip）

## 提交约定

使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```
feat: add new feature
fix: fix bug
docs: documentation
test: add tests
refactor: refactor code
chore: dependency update
```

## 配置

配置文件位于 `~/.jarvis/config.toml`（由 `jarvis onboard` 生成）。主要配置段：`[memory]`、`[gateway]`、`[autonomy]`、`[runtime]`、`[tunnel]`、`[secrets]`、`[browser]`、`[composio]`、`[identity]`。

## 安全注意事项

- 所有文件操作必须验证路径在工作区范围内（路径遍历防护 + 符号链接逃逸检测）
- Shell 工具仅允许白名单命令，安全环境变量过滤
- 禁止硬编码密钥，错误信息中自动清洗 API 密钥（`sk-`、`xoxb-`、`xoxp-` 前缀）
- 通道入站消息默认拒绝（空白名单 = 拒绝所有）
