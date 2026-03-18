<div align="center">

<img src="openpup-icon.svg" width="80" alt="OpenPup">

# OpenPup

**一只记得你是谁的本地 AI 助手**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/tauri-2.0-blue.svg)](https://tauri.app)

[← 返回](README.md) · [English](README.en.md)

</div>

---

## 这是什么？

不是又一个 ChatGPT 套壳。

大多数 AI 工具在问：*「你想要一个什么样的 AI？」*
openpup 在问：**「你是一个什么样的人？」**

你的 `OWNER.md` 是核心——这是你写给自己的画像，每只小狗在每条消息里都会读它。对话、偏好和习惯会不断沉淀为**本地记忆**，是任何云端服务都无法复制或带走的。

它有自己的记忆文件，你可以直接编辑。它有自己的专属技能，跑在你的机器上。用久了，它比任何云端 AI 都懂你。

一组专业「小狗」负责处理你的请求：

| 小狗 | 职责 |
|------|------|
| Alpha | 总协调——了解你，调度其他小狗 |
| Dev | 软件工程、代码审查、调试 |
| Writer | 文章、邮件、报告、任何书面内容 |
| Ops | DevOps、服务器、部署、基础设施 |
| Research | 信息收集、摘要、分析 |
| Life Admin | 日程、规划、个人事务 |

所有数据保存在 `~/.openpup/`——纯文本文件，随时可读、可编辑、可备份。

---

## 与竞品对比

OpenPup、**OpenClaw** 和 **ZeroClaw** 处于同一生态位——三者都是本地优先、开源、有技能系统的 AI Agent。区别在于各自的核心取向：

### 功能对比

| | **OpenPup** | **OpenClaw** | **ZeroClaw** |
|---|:---:|:---:|:---:|
| **核心哲学** | 主人中心（OWNER.md） | 智能体中心（SOUL.md） | 运行时中心（可插拔） |
| **核心问题** | *你是什么样的人？* | *你想要什么样的 AI？* | *部署在哪里？* |
| 语言 / 运行时 | Rust + Tauri | TypeScript（Node.js） | Rust |
| 内存占用 | 低（原生） | >1 GB（Node.js 堆） | <5 MB |
| 启动时间 | 快（原生） | 数秒（Node.js 初始化） | <10 毫秒 |
| 界面 | 桌面应用 + CLI | Web UI + 27+ 消息平台网关 | CLI / 容器化 |
| 多智能体团队 | ✅ 5 只专家 + Alpha | ❌ 单一智能体 + AgentSkills | ❌ 单一运行时 |
| 记忆模型 | OWNER.md + SQLite 语义搜索 | SOUL.md + 每日 markdown + SQLite | 可插拔后端 |
| 技能格式 | TOML（ClaWHub） | AgentSkills YAML（ClaWHub） | 可插拔 |
| MCP 支持 | ✅ rmcp 流式 HTTP | ✅ | ❌ |
| 目标用户 | 个人日常使用 | 重度集成用户（20+ 平台） | 边缘 / 无服务器部署 |
| 开源协议 | MIT / Apache 2.0 | MIT（基金会维护） | Apache 2.0 |

### vs. OpenClaw

OpenPup 和 OpenClaw 共享同一个技能市场（[ClaWHub](https://clawhub.ai)）——技能文件可以互相兼容。核心差异是哲学层面的：

> OpenClaw 的 `SOUL.md`：定义的是 **AI 的人格**。
> OpenPup 的 `OWNER.md`：定义的是 **你的身份**。

OpenClaw 在问：「你想要一个什么样的 AI？」——你来塑造智能体的性格、声音和观点。
OpenPup 在问：「你是一个什么样的人？」——小狗们适应你，而不是反过来。

实际使用上：如果你需要一个高度自定义的单一智能体，同时打通 WhatsApp、Telegram、Discord 和桌面——OpenClaw 更合适。如果你想要一个**深度了解你、按专业分工协作的本地团队**，以及一个流畅的原生桌面应用——OpenPup 更合适。

值得注意的一个差异：OpenClaw 基于 Node.js，内存占用超过 1 GB（官方最低要求 1 GB，推荐 2 GB+）；作为消息网关长期驻留运行，资源消耗是持续的。OpenPup 是编译后的 Rust + Tauri，在同等硬件上更轻。

> OpenClaw 由 Peter Steinberger 于 2025 年底创建；2026 年 2 月 Steinberger 加入 OpenAI 后，项目移交开源基金会维护，持续活跃。

### vs. ZeroClaw

ZeroClaw 是一个面向**边缘计算和资源受限环境**的 Rust Agent 运行时——单一可执行文件，内存 <5 MB，冷启动 <10 毫秒，每个子系统（provider、channel、tool、memory）均可插拔替换。设计目标是跑在 10 美元的单板计算机上或 Docker 容器里。协议为 Apache 2.0。

OpenPup 是一个**桌面个人助手**——不适合无服务器或边缘部署，但提供了 ZeroClaw 没有的东西：图形界面、多智能体路由、丰富的记忆管理 UI，以及以主人为核心的设计哲学。

需要把 Agent 部署到服务器或嵌入式设备？ZeroClaw 是正确选择。需要一个随时间不断了解你的日常桌面伴侣？那就是 OpenPup。

---

## 功能特性

- **流式聊天** + 智能多智能体路由
- **长期记忆** — 每 5 条消息自动提取；语义去重防止噪音累积
- **技能系统** — 可安装的 TOML 格式提示链（兼容 [ClaWHub](https://clawhub.ai)）
- **MCP 支持** — 通过 Model Context Protocol（rmcp，流式 HTTP）接入外部工具
- **牵绳 / 自由运行模式** — 每次操作均确认，或让小狗自主执行
- **权限系统** — 三级风险（高/中/低），支持对常用操作的持久信任
- **任务追踪** — 创建并跟踪任务生命周期（待处理→进行中→完成/失败）
- **CLI** — 完整的终端 REPL（`openpup chat`），读写同一份本地数据库
- **Workspace 备份** — 将 `~/.openpup/` 导出/导入为 tar.gz
- **插件 API** — 将自定义小狗编译为原生动态库（`.dylib`/`.so`/`.dll`）
- **主题** — GitHub 风格深色与浅色

---

## 安装

### 下载发布版（推荐）

从 [Releases 页面](../../releases) 下载：

| 平台 | 桌面应用 | CLI 二进制 |
|------|----------|------------|
| macOS（Apple Silicon） | `openpup-*.dmg` | `openpup-cli-*-darwin-aarch64.tar.gz` |
| macOS（Intel） | `openpup-*.dmg` | `openpup-cli-*-darwin-x86_64.tar.gz` |
| Linux x86_64 | `openpup-*.AppImage` | `openpup-cli-*-linux-x86_64.tar.gz` |
| Windows | `openpup-*-setup.exe` | `openpup-cli-*-windows-x86_64.tar.gz` |

### 从源码构建

**前置条件：** Node.js 18+、Rust stable、[Tauri 系统依赖](https://tauri.app/start/prerequisites/)

```bash
git clone https://github.com/openpup/openpup
cd openpup

# 桌面应用
npm install
npm run tauri build

# 仅构建 CLI（更快，无需 Node.js）
cargo build --release -p openpup
```

---

## 配置

首次启动时，openpup 会创建 `~/.openpup/config.toml`：

```toml
[llm]
provider   = "openai"          # "openai"（OpenAI 兼容 API）或 "ollama"（本地）
model      = "gpt-4o"
mini_model = "gpt-4o-mini"     # 用于分类任务的轻量模型
api_key    = "sk-..."          # 或设置环境变量 OPENPUP_API_KEY
api_base   = ""                # 留空使用默认值
                               # 例：https://api.siliconflow.cn/v1

[app]
execution_mode = "leashed"     # "leashed"（操作前确认）或 "freerun"（自主执行）
theme          = "dark"        # "dark" 或 "light"
language       = "zh"          # "zh" 或 "en"

[pups]
enabled = ["alpha", "dev", "writer", "ops", "research", "life_admin"]
```

完整参考模板：[`workspace/config.toml`](workspace/config.toml)

**环境变量覆盖**（优先级高于配置文件）：

| 变量 | 用途 |
|------|------|
| `OPENPUP_API_KEY` | API 密钥 |
| `OPENAI_API_KEY` | 备用 API 密钥 |
| `OPENAI_BASE_URL` | Base URL 覆盖 |
| `OPENPUP_LLM_PROVIDER` | `openai` 或 `ollama` |

---

## CLI 用法

```bash
# 对话
openpup chat                          # 与 Alpha Pup 对话
openpup chat --pup dev                # 直接路由到 Dev Pup

# 记忆管理
openpup memory list                   # 浏览长期记忆
openpup memory search "python 偏好"   # 按内容搜索
openpup memory count                  # 总记忆数量

# 技能
openpup skill list                    # 已安装的技能
openpup skill run weekly_summary      # 运行技能
openpup skill run my_skill --input "上下文文本"

# 状态概览
openpup status
```

---

## 技能系统

技能是 TOML 格式的提示链定义，兼容 [ClaWHub](https://clawhub.ai) 技能格式：

```toml
name        = "daily_summary"
description = "从记忆生成每日总结"
version     = "1.0.0"

[[steps]]
type   = "search_memories"
query  = "今天的活动和任务"
limit  = 20

[[steps]]
type   = "generate_with_llm"
prompt = "基于以下记忆，写一份简洁的每日总结：\n{{memories}}"
```

通过 App 中的 **Skills → 从 Git 安装** 可安装任意 Git 仓库中的技能。[ClaWHub 技能库](https://clawhub.ai/PhenixStar/skill-hub) 会在首次运行时自动安装。

---

## MCP 集成

openpup 使用 [rmcp](https://github.com/modelcontextprotocol/rust-sdk)（官方 MCP Rust SDK）的流式 HTTP 传输。在 **Settings → MCP** 中添加服务器，或直接编辑 `~/.openpup/mcp_servers.json`。

---

## 插件开发

将自定义小狗编译为原生动态库：

```rust
#[no_mangle]
pub extern "C" fn create_pup() -> *mut dyn SpecialistPup {
    Box::into_raw(Box::new(MyPup::new()))
}
```

编译为 `cdylib`，放入 `~/.openpup/plugins/`。详见 [`plugins/example_pup/README.md`](plugins/example_pup/README.md)。

---

## Workspace 目录结构

```
~/.openpup/
├── config.toml          ← 全部配置
├── database.db          ← SQLite：对话、记忆、任务、技能运行记录
├── OWNER.md             ← 你的画像——姓名、偏好、背景
├── PUPS.md              ← 小狗定义
├── RULES.md             ← 小狗遵循的规则（可自进化）
├── memories/            ← 每日日志（YYYY-MM-DD.md）
├── skills_state/        ← 已安装技能清单及注册源状态
├── mcp_servers.json     ← MCP 服务器列表
└── plugins/             ← 原生小狗插件（.dylib / .so / .dll）
```

---

## 许可证

[MIT](LICENSE-MIT) OR [Apache 2.0](LICENSE-APACHE) — © 2026 OpenPup Contributors
