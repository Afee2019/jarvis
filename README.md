<p align="center">
  <img src="jarvis.png" alt="Jarvis" width="200" />
</p>

<h1 align="center">Jarvis 🤖</h1>

<p align="center">
  <strong>Your AI, your rules.</strong><br>
  ⚡️ <strong>在 $10 硬件上运行，内存 <5MB：比 OpenClaw 节省 99% 内存，比 Mac mini 便宜 98%！</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
</p>

快速、轻量、完全自主的 AI 助手基础设施 —— 随处部署，万物可换。

```
~3.4MB 二进制 · <10ms 启动 · 1,017 个测试 · 22+ 提供商 · 8 个 trait · 一切皆可插拔
```

### ✨ 特性

- 🏎️ **超轻量：** 内存占用 <5MB —— 比 OpenClaw 核心小 99%。
- 💰 **极低成本：** 可在 $10 硬件上高效运行 —— 比 Mac mini 便宜 98%。
- ⚡ **闪电启动：** 启动速度快 400 倍，<10ms 启动（0.6GHz 核心上也不到 1 秒）。
- 🌍 **真正可移植：** 单一自包含二进制，支持 ARM、x86 和 RISC-V。

### 为什么团队选择 Jarvis

- **默认精简：** 小巧的 Rust 二进制，快速启动，低内存占用。
- **安全为先：** 配对认证、严格沙箱、显式白名单、工作区作用域。
- **完全可换：** 核心系统均为 trait（提供商、通道、工具、记忆、隧道）。
- **无锁定：** 支持 OpenAI 兼容提供商 + 可插拔自定义端点。

## 基准测试快照（Jarvis vs OpenClaw）

本地快速基准测试（macOS arm64，2026 年 2 月），已归一化至 0.8GHz 边缘硬件。

| | OpenClaw | NanoBot | PicoClaw | Jarvis 🤖 |
|---|---|---|---|---|
| **语言** | TypeScript | Python | Go | **Rust** |
| **内存** | > 1GB | > 100MB | < 10MB | **< 5MB** |
| **启动时间（0.8GHz 核心）** | > 500s | > 30s | < 1s | **< 10ms** |
| **二进制大小** | ~28MB (dist) | N/A (脚本) | ~8MB | **3.4 MB** |
| **成本** | Mac Mini $599 | Linux SBC ~$50 | Linux 开发板 $10 | **任意硬件 $10** |

> 注：Jarvis 数据使用 `/usr/bin/time -l` 在 release 构建上测量。OpenClaw 需要 Node.js 运行时（约 390MB 开销）。PicoClaw 和 Jarvis 是静态二进制。

<p align="center">
  <img src="jarvis.jpeg" alt="Jarvis vs OpenClaw 对比" width="800" />
</p>

本地复现 Jarvis 测试数据：

```bash
cargo build --release
ls -lh target/release/jarvis

/usr/bin/time -l target/release/jarvis --help
/usr/bin/time -l target/release/jarvis status
```

## 快速开始

```bash
git clone https://github.com/Afee2019/jarvis.git
cd jarvis
cargo build --release
cargo install --path . --force

# 快速配置（无交互提示）
jarvis onboard --api-key sk-... --provider openrouter

# 或使用交互式向导
jarvis onboard --interactive

# 或仅快速修复通道/白名单
jarvis onboard --channels-only

# 聊天
jarvis agent -m "你好，Jarvis！"

# 交互模式
jarvis agent

# 启动网关（webhook 服务器）
jarvis gateway                # 默认：127.0.0.1:8080
jarvis gateway --port 0       # 随机端口（安全加固）

# 启动完整自主运行时
jarvis daemon

# 检查状态
jarvis status

# 运行系统诊断
jarvis doctor

# 检查通道健康状态
jarvis channel doctor

# 获取集成配置详情
jarvis integrations info Telegram

# 管理后台服务
jarvis service install
jarvis service status

# 从 OpenClaw 迁移记忆（先安全预览）
jarvis migrate openclaw --dry-run
jarvis migrate openclaw
```

> **开发替代（无需全局安装）：** 在命令前加 `cargo run --release --`（例如：`cargo run --release -- status`）。

## 架构

每个子系统都是一个 **trait** —— 修改配置即可切换实现，无需改动代码。

<p align="center">
  <img src="docs/architecture.svg" alt="Jarvis 架构" width="900" />
</p>

| 子系统 | Trait | 内置实现 | 扩展 |
|--------|-------|----------|------|
| **AI 模型** | `Provider` | 22+ 提供商（OpenRouter、Anthropic、OpenAI、Ollama、Venice、Groq、Mistral、xAI、DeepSeek、Together、Fireworks、Perplexity、Cohere、Bedrock 等） | `custom:https://your-api.com` —— 任意 OpenAI 兼容 API |
| **通道** | `Channel` | CLI、Telegram、Discord、Slack、iMessage、Matrix、WhatsApp、Webhook | 任意消息 API |
| **记忆** | `Memory` | SQLite 混合搜索（FTS5 + 向量余弦相似度）、Markdown | 任意持久化后端 |
| **工具** | `Tool` | shell、file_read、file_write、memory_store、memory_recall、memory_forget、browser_open（Brave + 白名单）、composio（可选） | 任意能力 |
| **可观测性** | `Observer` | Noop、Log、Multi | Prometheus、OTel |
| **运行时** | `RuntimeAdapter` | Native（Mac/Linux/Pi） | Docker、WASM（计划中；不支持的类型会立即报错退出） |
| **安全** | `SecurityPolicy` | 网关配对、沙箱、白名单、速率限制、文件系统作用域、加密密钥 | — |
| **身份** | `IdentityConfig` | OpenClaw（markdown）、AIEOS v1.1（JSON） | 任意身份格式 |
| **隧道** | `Tunnel` | None、Cloudflare、Tailscale、ngrok、Custom | 任意隧道二进制 |
| **心跳** | Engine | HEARTBEAT.md 定时任务 | — |
| **技能** | Loader | TOML 清单 + SKILL.md 说明 | 社区技能包 |
| **集成** | Registry | 9 个类别共 50+ 集成 | 插件系统 |

### 运行时支持（当前）

- ✅ 目前支持：`runtime.kind = "native"`
- 🚧 计划中，尚未实现：Docker / WASM / 边缘运行时

当配置了不支持的 `runtime.kind` 时，Jarvis 会以明确的错误退出，而不是静默回退到 native。

### 记忆系统（全栈搜索引擎）

全部自研，零外部依赖 —— 无 Pinecone、无 Elasticsearch、无 LangChain：

| 层 | 实现 |
|----|------|
| **向量数据库** | Embeddings 以 BLOB 形式存储在 SQLite 中，余弦相似度搜索 |
| **关键词搜索** | FTS5 虚拟表 + BM25 评分 |
| **混合合并** | 自定义加权合并函数（`vector.rs`） |
| **Embeddings** | `EmbeddingProvider` trait —— OpenAI、自定义 URL 或 noop |
| **分块** | 基于行的 markdown 分块器，保留标题 |
| **缓存** | SQLite `embedding_cache` 表 + LRU 淘汰 |
| **安全重建索引** | 原子化重建 FTS5 + 补嵌缺失向量 |

Agent 通过工具自动召回、保存和管理记忆。

```toml
[memory]
backend = "sqlite"          # "sqlite"、"markdown"、"none"
auto_save = true
embedding_provider = "openai"
vector_weight = 0.7
keyword_weight = 0.3
```

## 安全

Jarvis 在**每一层**都强制执行安全策略 —— 不仅仅是沙箱。它通过了社区安全检查清单的所有项目。

### 安全检查清单

| # | 项目 | 状态 | 实现方式 |
|---|------|------|----------|
| 1 | **网关不公开暴露** | ✅ | 默认绑定 `127.0.0.1`。没有隧道或未显式设置 `allow_public_bind = true` 时拒绝绑定 `0.0.0.0`。 |
| 2 | **要求配对认证** | ✅ | 启动时生成 6 位一次性配对码。通过 `POST /pair` 交换 Bearer 令牌。所有 `/webhook` 请求需要 `Authorization: Bearer <token>`。 |
| 3 | **文件系统受限（非根目录）** | ✅ | 默认 `workspace_only = true`。14 个系统目录 + 4 个敏感点文件被禁止访问。阻止 Null 字节注入。通过路径规范化 + 解析路径工作区检查检测符号链接逃逸。 |
| 4 | **仅通过隧道访问** | ✅ | 没有活动隧道时网关拒绝公开绑定。支持 Tailscale、Cloudflare、ngrok 或任意自定义隧道。 |

> **自行运行 nmap：** `nmap -p 1-65535 <your-host>` —— Jarvis 仅绑定 localhost，除非你显式配置隧道，否则不会暴露任何端口。

### 通道白名单（Telegram / Discord / Slack）

入站发送者策略现在保持一致：

- 空白名单 = **拒绝所有入站消息**
- `"*"` = **允许所有**（需显式选择）
- 其他 = 精确匹配白名单

这使得默认情况下意外暴露风险最低。

推荐的低摩擦配置方式（安全 + 快速）：

- **Telegram：** 将你的 `@username`（不含 `@`）和/或 Telegram 数字用户 ID 加入白名单。
- **Discord：** 将你的 Discord 用户 ID 加入白名单。
- **Slack：** 将你的 Slack 成员 ID（通常以 `U` 开头）加入白名单。
- 仅在临时开放测试时使用 `"*"`。

如果不确定使用哪个身份标识：

1. 启动通道并给你的机器人发送一条消息。
2. 查看警告日志以获取确切的发送者身份。
3. 将该值添加到白名单并重新运行仅通道配置。

如果在日志中看到授权警告（例如：`ignoring message from unauthorized user`），
仅重新运行通道配置：

```bash
jarvis onboard --channels-only
```

### WhatsApp Business Cloud API 配置

WhatsApp 使用 Meta 的 Cloud API 和 webhook（推送模式，非轮询）：

1. **创建 Meta Business 应用：**
   - 访问 [developers.facebook.com](https://developers.facebook.com)
   - 创建新应用 → 选择 "Business" 类型
   - 添加 "WhatsApp" 产品

2. **获取凭据：**
   - **Access Token：** 从 WhatsApp → API Setup → 生成令牌（或创建 System User 以获取永久令牌）
   - **Phone Number ID：** 从 WhatsApp → API Setup → Phone number ID
   - **Verify Token：** 由你自定义（任意随机字符串）—— Meta 会在 webhook 验证时回传此值

3. **配置 Jarvis：**
   ```toml
   [channels_config.whatsapp]
   access_token = "EAABx..."
   phone_number_id = "123456789012345"
   verify_token = "my-secret-verify-token"
   allowed_numbers = ["+1234567890"]  # E.164 格式，或 ["*"] 允许所有
   ```

4. **启动带隧道的网关：**
   ```bash
   jarvis gateway --port 8080
   ```
   WhatsApp 要求 HTTPS，因此需要使用隧道（ngrok、Cloudflare、Tailscale Funnel）。

5. **配置 Meta webhook：**
   - 在 Meta 开发者控制台 → WhatsApp → Configuration → Webhook
   - **Callback URL：** `https://your-tunnel-url/whatsapp`
   - **Verify Token：** 与配置中的 `verify_token` 相同
   - 订阅 `messages` 字段

6. **测试：** 向你的 WhatsApp Business 号码发送消息 —— Jarvis 将通过 LLM 回复。

## 配置

配置文件：`~/.jarvis/config.toml`（由 `onboard` 创建）

```toml
api_key = "sk-..."
default_provider = "openrouter"
default_model = "anthropic/claude-sonnet-4-20250514"
default_temperature = 0.7

[memory]
backend = "sqlite"              # "sqlite"、"markdown"、"none"
auto_save = true
embedding_provider = "openai"   # "openai"、"noop"
vector_weight = 0.7
keyword_weight = 0.3

[gateway]
require_pairing = true          # 首次连接时要求配对码
allow_public_bind = false       # 没有隧道时拒绝绑定 0.0.0.0

[autonomy]
level = "supervised"            # "readonly"、"supervised"、"full"（默认：supervised）
workspace_only = true           # 默认：true —— 限定在工作区内
allowed_commands = ["git", "npm", "cargo", "ls", "cat", "grep"]
forbidden_paths = ["/etc", "/root", "/proc", "/sys", "~/.ssh", "~/.gnupg", "~/.aws"]

[runtime]
kind = "native"                # 目前唯一支持的值；不支持的类型会立即报错退出

[heartbeat]
enabled = false
interval_minutes = 30

[tunnel]
provider = "none"               # "none"、"cloudflare"、"tailscale"、"ngrok"、"custom"

[secrets]
encrypt = true                  # 使用本地密钥文件加密 API 密钥

[browser]
enabled = false                 # 需显式启用的 browser_open 工具
allowed_domains = ["docs.rs"]  # 启用浏览器时必须设置

[composio]
enabled = false                 # 需显式启用：通过 composio.dev 接入 1000+ OAuth 应用

[identity]
format = "openclaw"             # "openclaw"（默认，markdown 文件）或 "aieos"（JSON）
# aieos_path = "identity.json"  # AIEOS JSON 文件路径（相对于工作区或绝对路径）
# aieos_inline = '{"identity":{"names":{"first":"Nova"}}}'  # 内联 AIEOS JSON
```

## 身份系统（AIEOS 支持）

Jarvis 支持**身份无关**的 AI 人格，提供两种格式：

### OpenClaw（默认）

工作区中的传统 markdown 文件：
- `IDENTITY.md` —— Agent 是谁
- `SOUL.md` —— 核心人格与价值观
- `USER.md` —— Agent 服务的用户是谁
- `AGENTS.md` —— 行为准则

### AIEOS（AI 实体对象规范）

[AIEOS](https://aieos.org) 是一个可移植 AI 身份的标准化框架。Jarvis 支持 AIEOS v1.1 JSON 载荷，允许你：

- 从 AIEOS 生态系统**导入身份**
- 向其他 AIEOS 兼容系统**导出身份**
- 在不同 AI 模型间**保持行为一致性**

#### 启用 AIEOS

```toml
[identity]
format = "aieos"
aieos_path = "identity.json"  # 相对于工作区或绝对路径
```

或内联 JSON：

```toml
[identity]
format = "aieos"
aieos_inline = '''
{
  "identity": {
    "names": { "first": "Nova", "nickname": "N" }
  },
  "psychology": {
    "neural_matrix": { "creativity": 0.9, "logic": 0.8 },
    "traits": { "mbti": "ENTP" },
    "moral_compass": { "alignment": "Chaotic Good" }
  },
  "linguistics": {
    "text_style": { "formality_level": 0.2, "slang_usage": true }
  },
  "motivations": {
    "core_drive": "突破边界，探索可能性"
  }
}
'''
```

#### AIEOS Schema 各部分

| 部分 | 描述 |
|------|------|
| `identity` | 姓名、简介、出生地、居住地 |
| `psychology` | 神经矩阵（认知权重）、MBTI、OCEAN、道德指南针 |
| `linguistics` | 文本风格、正式程度、口头禅、禁用词 |
| `motivations` | 核心驱动力、短期/长期目标、恐惧 |
| `capabilities` | Agent 可使用的技能和工具 |
| `physicality` | 用于图像生成的视觉描述 |
| `history` | 起源故事、教育背景、职业 |
| `interests` | 爱好、偏好、生活方式 |

完整 schema 和在线示例请参阅 [aieos.org](https://aieos.org)。

## 网关 API

| 端点 | 方法 | 认证 | 描述 |
|------|------|------|------|
| `/health` | GET | 无 | 健康检查（始终公开，不泄露密钥） |
| `/pair` | POST | `X-Pairing-Code` 请求头 | 交换一次性配对码以获取 Bearer 令牌 |
| `/webhook` | POST | `Authorization: Bearer <token>` | 发送消息：`{"message": "your prompt"}` |
| `/whatsapp` | GET | 查询参数 | Meta webhook 验证（hub.mode、hub.verify_token、hub.challenge） |
| `/whatsapp` | POST | 无（Meta 签名） | WhatsApp 入站消息 webhook |

## 命令

| 命令 | 描述 |
|------|------|
| `onboard` | 快速配置（默认） |
| `onboard --interactive` | 完整交互式 7 步向导 |
| `onboard --channels-only` | 仅重新配置通道/白名单（快速修复流程） |
| `agent -m "..."` | 单条消息模式 |
| `agent` | 交互式聊天模式 |
| `gateway` | 启动 webhook 服务器（默认：`127.0.0.1:8080`） |
| `gateway --port 0` | 随机端口模式 |
| `daemon` | 启动长时间运行的自主运行时 |
| `service install/start/stop/status/uninstall` | 管理用户级后台服务 |
| `doctor` | 诊断守护进程/调度器/通道状态 |
| `status` | 显示完整系统状态 |
| `channel doctor` | 运行通道健康检查 |
| `integrations info <name>` | 显示指定集成的配置/状态详情 |

## 开发

```bash
cargo build              # 开发构建
cargo build --release    # 发布构建（~3.4MB）
cargo test               # 1,017 个测试
cargo clippy             # Lint（0 warnings）
cargo fmt                # 格式化

# 运行 SQLite vs Markdown 基准测试
cargo test --test memory_comparison -- --nocapture
```

### Pre-push 钩子

一个 git 钩子会在每次 push 前运行 `cargo fmt --check`、`cargo clippy -- -D warnings` 和 `cargo test`。启用一次即可：

```bash
git config core.hooksPath .githooks
```

在开发过程中需要快速 push 时跳过钩子：

```bash
git push --no-verify
```

## 许可证

MIT —— 参阅 [LICENSE](LICENSE)

## 贡献

参阅 [CONTRIBUTING.md](CONTRIBUTING.md)。实现一个 trait，提交 PR：
- 新 `Provider` → `src/providers/`
- 新 `Channel` → `src/channels/`
- 新 `Observer` → `src/observability/`
- 新 `Tool` → `src/tools/`
- 新 `Memory` → `src/memory/`
- 新 `Tunnel` → `src/tunnel/`
- 新 `Skill` → `~/.jarvis/workspace/skills/<name>/`

---

**Jarvis** —— Your AI, your rules. 🤖
