## openPup - 你的个人分身 OS

> Opened by you, loyal forever.  
> 上网之处，皆有你的小分身。

License: **MIT OR Apache-2.0**.

openPup 是一个面向个人/小团队的 **Rust 数字分身操作系统**：  
内建人格、三层记忆、**L1–L4 风险分级工具**和定时 Loop，让你的「第二个你」在安全边界内，长期经营你的工作、投资和生活。

和 OpenClaw 相比：**OpenClaw 更像多通道消息 + 工具的通用 Agent 网关；openPup 专注于「一个人的王朝」——人格化、记忆化、生活 + 投资 + 工作一体的长期分身。**

一句话：**openPup 是你亲手造出来的“第二个你”，而不是一个通用 ChatGPT wrapper。**

---

### 🚀 项目愿景

**openPup** 想要做的是：  
不是给所有人一个「Agent 云服务」，而是给你一个可以带走的、跑在你自己环境里的 **个人王朝中枢（Core + Workers）**。

它常驻在你的世界里，作为 **Core 大脑 + Worker 节点**：

- **记得你是谁、在意什么**（Persona + Memory）
- **按时间/事件主动跑 Loop**（Scheduler + Loops）
- **通过工具和节点操作外部世界**（Tools + SubAgents + Nodes）
- **所有高风险动作都分层治理**（L1–L4 风险等级 + 审批）

目标是：**让一个强人格、强记忆、强约束的「第二个你」帮你长期经营自己的王朝**。

---

### 🎯 为什么选择 openPup？

- **🧠 强人格 + 三层记忆**  
  - Persona 文档 + 短期 / 长期 / 程序性记忆，决策风格、沟通方式、风险偏好尽量「像你」。  
  - 所有 Loop、工具调用和重要对话，都会沉淀进本地记忆与审计日志。

- **🏛️ Core/Workers 王朝模型**  
  - Core 负责世界观、规则和调度（「你」）。  
  - Worker 节点跑在不同机器/环境里（浏览器自动化、家居控制、市场数据、服务器脚本等），作为你的「地方官」。

- **🛡️ 安全优先的工具分层（L1–L4）**  
  - 从只读查询到小额度自治，再到高风险动作（交易、家居控制、远程登录），按 L1–L4 逐层分级。  
  - 高风险工具默认不开放，必须显式配置或走审批工具（ApprovalToolExecutor + 本地 UI）。

- **⏰ 生活 + 投资 + 工作一体的 Loop**  
  - 内置 `work:*` / `invest:*` / `life:*` 这类 Loop：  
    - 工作：晨间规划、复盘、邮件/PR 扫描  
    - 投资：盘前/盘中/盘后扫描、研究队列管理  
    - 生活：睡前例行、公寓状态检查、日程/任务整理

- **📜 可审计、可降权的自治**  
  - 所有 CLI 和自治行为写入 `~/.openpup/*` 审计日志（含工具等级、来源、上下文）。  
  - 提供一键降权的安全入口（只读 / draft-only），随时把分身拉回「顾问模式」。

---

### 🏗️ 核心架构

从上到下，可以粗略理解为：

```text
┌─────────────────────────────────────────┐
│          你的王朝 / 生活 / 工作视角       │
│  (目标 / 领域：工作、投资、生活等)       │
├─────────────────────────────────────────┤
│        Core 分身 (Persona + Memory)    │
│   - 会话、记忆、计划、审计、决策风格     │
├─────────────────────────────────────────┤
│        Loop & Scheduler 调度层          │
│   - work_*/invest_*/life_* 等 Loop      │
│   - 时间/事件驱动执行                   │
├─────────────────────────────────────────┤
│     Tools & SubAgents & Nodes 工具层     │
│   - L1–L4 风险分级工具                  │
│   - 本地组合工具 (TOML)                 │
│   - SubAgents / Node Workers            │
├─────────────────────────────────────────┤
│        LLM 交互层 (OpenAI 等)           │
│   - 对话 / 规划 / 工具调用              │
└─────────────────────────────────────────┘
```

更细节的架构与扩展点见 `docs/architecture.md`、`docs/CLI.md` 与 `docs/tools-autonomy-safety.md`。

---

### 🔑 关键特性

1. **个人分身，而不是通用 Agent SaaS**  
   - 开箱就是围绕「你这个人」构建的人格 + 记忆 + Loop 体系。  
   - 没有「多租户超级平台」这套设计，默认假设这是你私人的王朝操作系统。

2. **Persona + Memory：三层记忆体系**  
   - 短期：当前会话与最近的任务上下文。  
   - 长期：跨会话的关键决策、偏好、重要事实。  
   - 程序性：一类「怎么做」的经验/Playbook（偏向工作流与技能）。

3. **L1–L4 风险分级工具 + 审批**  
   - L1：只读查询（行情、RSS、邮件头、日历、Home Assistant 状态等）。  
   - L2：低风险写操作（本地文件、备忘、低影响配置）。  
   - L3：小权限自治（内部 TODO 进度、仅本机状态、不会对外部世界造成直接危险）。  
   - L4：高风险外部动作（真实交易、设备控制、远程登录等，默认关 + 需要显式审批）。

4. **Core + Worker 多节点模型**  
   - Core：你的大脑与调度中枢，管理 Persona、Memory、Loop、工具与审计。  
   - Worker Node：通过 HTTP 或其他协议挂接在 Core 之下的执行节点，你可以在不同机器上跑 `node_worker` 或自定义 Worker。

5. **生活 / 投资 / 工作一体化 Loop**  
   - 用 `openpup run work:morning`、`invest:morning`、`life:evening` 等命令触发不同领域的日常流程。  
   - 用 `openpup up` 启动长驻 scheduler，让分身按时间自动行动。

6. **审计优先 + 紧急降权**  
   - 所有行为都有审计记录：来源、Loop、工具、风险等级。  
   - 提供 `openpup safety readonly` / `draft-only` 等命令立刻把自治降到安全等级。

---

### 🛠️ 快速开始

#### 安装（开发者模式）

```bash
# 克隆仓库
git clone https://github.com/yourname/openpup.git
cd openpup

# 构建 CLI
cargo build --release

# （可选）把 openpup 放到 PATH
cp target/release/openpup ~/.local/bin/  # 或你自己的 bin 目录
```

#### 初始化你的分身

```bash
# 1. 初始化分身配置（模型、通道、基础目录）
openpup init

# 2. 初始化 Persona 工作区（写下「你是谁、你在意什么」）
openpup persona init

# 3. 启动分身 OS（Scheduler + Loops）
openpup up

# 4. 看看你的分身在干嘛
openpup status
```

更多 CLI 细节见 `docs/CLI.md`。

---

### 💬 第一个「分身对话」

和参考 README 的「第一个 Agent」类似，在 openPup 里你主要通过 CLI 和本地配置来「养成」分身。

```bash
# 进入交互式分身会话
openpup agent

# 单轮问答（适合脚本或快捷键）
openpup agent -m "帮我做一个今天的工作计划"
```

此时：

- 分身会带着你的 Persona 与记忆来回答。  
- 可以在允许的工具等级内，主动调用工具（如读取日历、扫描邮件主题、查看市场头条等），并把结果写入记忆与审计。

---

### 🔁 Loop 示例：工作 / 投资 / 生活

```bash
# 工作相关 Loop
openpup run work:morning      # 早晨规划、待办整理、重要邮件/PR 扫描
openpup run work:evening      # 下班前复盘，总结与明日计划

# 投资相关 Loop
openpup run invest:morning    # 盘前行情/新闻扫描，更新关注列表
openpup run invest:evening    # 盘后复盘与记忆更新

# 生活相关 Loop
openpup run life:evening      # 睡前例行：日程检查、任务收尾、家居状态只读检查

# 启动长驻调度
openpup up                    # 让这些 Loop 按时间自动跑
```

Loop 的结构与配置见 `docs/CLI.md` 与 `docs/architecture.md`。

---

### 🤝 多 Agent / 多 Node 协作（Core + Workers）

- **多 Agent（SubAgents）**  
  - 通过 `openpup spawn` 或在对话中调用 `register_sub_agent` 工具，为你的王朝定义不同角色：  
    - `research`：只做研报与资讯摘要  
    - `writer`：负责写作与润色  
    - `ops`：负责日常运维脚本等  
  - 主分身通过 `invoke_sub_agent` 工具点将，形成一个「你+小团队」的协作模式。

- **多 Node（Workers）**  
  - 通过 `openpup node spawn` 和 `register_node` 工具，把不同机器/环境挂到你的 Core 下：  
    - 浏览器自动化 Worker（券商网页等）  
    - 家居 Worker（Home Assistant）  
    - 数据/回测 Worker（市场数据、回测引擎）
  - Core 通过 `invoke_node_tool` 把指令发到远端 Worker，Worker 只负责执行具体工具。

更完整示例依然保留在代码和文档中（如 `node_worker` 示例、`docs/architecture.md`）。

---

### 📋 使用场景

#### 1. 个人分身 / 生活+工作的「第二个你」

- 早晚例行：自动帮你做工作/投资/生活的晨间/晚间 checklist。  
- 信息过滤：邮件标题、RSS、市场头条等的只读扫描与摘要。  
- 长期记忆：重要决策和复盘沉淀成可检索的语义记忆。

#### 2. 安全优先的投资助手（只读起步）

- 从只读市场数据+新闻+研究队列开始，不直接下单。  
- 在审计和工具分层下，逐步走向模拟盘/小额度自动化。  
- 所有决策经过 L3 日志与审计，方便你之后复盘和回滚权限。

#### 3. 家居与个人数字生活中枢

- 通过 Home Assistant 等工具，只读你的家居状态。  
- 在白名单和时间窗口内，逐步尝试低风险控制（如睡前关灯提醒）。  
- 所有行为写入本地审计，确保未来扩权可控。

#### 4. 开发者的「个人运维官」

- 定时检查服务状态、日志摘要、磁盘/备份情况（先只读）。  
- 在 L2/L3 工具下，逐步尝试小规模自动运维动作。  
- 用 SubAgents/Nodes 把不同环境纳入你的个人王朝。

---

### 🧭 项目路线图（示意）

详细版本见 `ROADMAP.md`，这里只列一个简化视图：

- **Phase 1：个人分身核心（当前）**
  - ✅ 基础 CLI 与配置（`openpup init`/`up`/`agent`/`run` 等）  
  - ✅ Persona + Memory 三层结构  
  - ✅ L1–L3 工具分层与本地审计  
  - ✅ Loop 与 Scheduler（工作/投资/生活基本 Loop）

- **Phase 2：生态与 Worker 扩展**
  - ⬜ 更多官方工具与组合工具示例  
  - ⬜ Node Worker 模板与脚手架  
  - ⬜ 更丰富的审计 Dashboard / 可视化

- **Phase 3：小权限自治与进阶安全**
  - ⬜ 更细粒度的 L3 自动执行策略  
  - ⬜ 审批 UI 与飞书/IM 等外部入口  
  - ⬜ 更完善的紧急降权与回滚机制

---

### 🤝 参与贡献

openPup 目前仍然处在「自用为主，但欢迎旁观和共创」的阶段。

#### 如何参与

1. **报告问题 / 讨论设计**  
   - 使用 GitHub Issues 反馈 bug、提出功能需求或设计建议。  
   - 特别欢迎围绕 Persona/记忆/安全分层/Loop 的实战反馈。

2. **提交代码 / 文档**  
   - Fork 本仓库并创建特性分支。  
   - 遵循现有 Rust 代码风格与模块边界。  
   - 如有 Breaking change，请同步更新 `ROADMAP.md` 与相关文档。

3. **分享你的「王朝用例」**  
   - 分享你是如何用 openPup 经营自己的工作/投资/生活。  
   - 包括：Persona 结构、Loop 设计、工具分层、审计策略等。

---

### 📚 文档与资源

- **架构与设计（高层）**  
  - `ROADMAP.md`：阶段性目标、已落地能力与 Breaking changes。  
  - `docs/architecture.md`：整体架构与扩展点（Core + Workers + Loops + Tools）。

- **人格与记忆**  
  - `docs/persona-and-memory.md`：Persona 与三层记忆设计。

- **工具、自主性与安全**  
  - `docs/tools-autonomy-safety.md`：L1–L4 工具分层、自主性与安全模型。

---

### 📄 许可证

本项目采用 **MIT OR Apache-2.0** 双许可证协议，你可以在遵守协议的前提下自由使用、修改和分发。  
详细条款见仓库中的 `LICENSE` 文件。
