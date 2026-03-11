## Architecture – openpup

本架构文档描述如何以 **openpup** 自身作为调度与大脑层，构建一个「像我一样思考和行动」的个人 Operator 系统，并支持分布式调度。未来如需接入外部 Agent OS，可在此架构基础上扩展。

### 1. 总体拓扑与「王朝模型」

- **Core Node（核心节点，主宰/皇帝）**
  - 跑 openpup runtime 与 scheduler，承载：
    - 全局 Persona / Memory（代表「王朝的灵魂」）
    - 会话入口（Telegram / 飞书 / 本地 CLI）
    - 调度与决策（任务拆解、分配、审批）
  - 仅挂必要的工具（低/中风险）：
    - 会话管理、记忆读写
    - Home Assistant 只读与部分写操作（非关键场景）
    - Worker 节点调用工具（远程执行）

- **Worker Nodes（执行节点，官员/军团）**
  - 多台机器，每台跑一个轻量 Agent 实例或其他 Agent OS：
    - **Trading Worker**：浏览器自动化，专职券商网页操作（东方财富等）
    - **Home Worker**：与 Home Assistant 深度集成，执行家居自动化
    - **Data Worker**：数据抓取、回测、日志分析
  - 对外暴露受保护的 HTTP / WebSocket 接口，供 Core Node 调用：
    - 统一为「工具」形态，如 `tools.trading_execute`, `tools.home_run_scene`

- **Realms（领域/国土，可选抽象）**
  - 通过「Realm」抽象，把不同场景组织起来，例如：
    - 一个 Realm 代表一家公司、一个部门、一个策略组合、甚至一个「王朝」阵营
    - 每个 Realm 内部维护自己的角色树（CEO/Trader/Ops/家臣等）和任务树
  - Core Node 作为最高统筹者，可以在多个 Realm 之间调度资源与角色。

- **External Systems**
  - 飞书 / Telegram：作为主要对话与通知入口
  - Home Assistant：统一的家居控制平台
  - 券商网页：通过浏览器 Sidecar 工具进行自动化操作（无直接交易 API）

### 2. Persona 与 Memory 在架构中的位置

- **Persona（人格）**
  - 存放于 Core Node Workspace 的文档中（`IDENTITY.md`, `SOUL.md`, `USER.md`, `TOOLS.md`）
  - Core Node 在每次发起任务或调用 Worker 之前，将相关 Persona 片段注入到提示词中
  - Worker 节点可以只持有 Persona 的「子集」或引用，由 Core Node 统一控制版本

- **Memory（三层记忆）**
  - **短期会话记忆（Session Memory）**
    - 存在 Core Node 的会话上下文中
    - Worker 节点只接收与本次任务相关的精简上下文
  - **长期语义记忆（Semantic Memory）**
    - 建议在 Core Node 使用本地 SQLite + FTS + 向量检索
    - Worker 节点通过工具请求「检索结果」，而不是直接访问底层存储
  - **程序性记忆（Procedural Memory）**
    - 以 Playbook / 技能形式存在（技能配置、tool prompt、Cron 任务描述）
    - 有些 Playbook 仅在 Core Node 执行，有些则会「编排」多个 Worker 节点

### 3. 工具与分层能力

系统中所有「能动手」的能力，都抽象为 openpup 提供的工具（或未来接入的外部引擎工具），并按风险等级分层：

- **L1 – Read Only 工具**
  - 如：行情查询、新闻/RSS 抓取、邮件/日历读取、Home Assistant 状态查看
  - 可在 Core Node 与 Worker 上广泛分布，无需特殊审批

- **L2 – Draft 工具**
  - 产出草稿（邮件/文案/交易计划/自动化方案），不直接执行外部动作
  - 执行结果总是“文本/结构化建议”，再由其他流程或人工确认

- **L3 – Auto Safe 工具（低风险自动执行）**
  - 如：睡前关灯、离家关空调、生成日报文件
  - 一般部署在 Home Worker，Core Node 通过明确的 Playbook 调用
  - 需要满足：时间窗口 + 白名单 + 幂等性

- **L4 – High-Risk 工具（高风险动作）**
  - 如：真实资金交易指令、重要系统配置修改
  - 原则：默认不开放；即使开放，也只能挂在特定 Worker（Trading Worker），并由 Core Node 严格控制
  - 所有调用都必须满足：
    - 来源会话和用户身份校验
    - 通过风控规则和限额检查
    - 留下完整的审计日志（输入、决策理由、执行结果）

### 4. 自主性与调度

整体采用「Core Node 驱动、Worker 节点执行」的自主模型：

- **事件源（Triggers）**
  - 时间型：openpup 自带 scheduler 或系统 Cron / Heartbeat，定时触发工作/投资/生活 Loop
  - 事件型：
    - 飞书 / Telegram 收到消息或特定命令
    - Home Assistant 的事件（在家/离家、设备状态变化）
    - 市场行情/监控信号（来自 Data Worker）

- **调度流程**
  1. Core Node 接收事件，结合 Persona 与 Memory 进行「状态评估」
  2. Core Node 决定是否创建或更新任务（Goal/Task），并选择合适的 Worker
  3. Core Node 通过工具调用 Worker 的接口，传入：
     - 上下文（压缩后的记忆 + Persona 片段）
     - 任务说明与约束（金额上限、时间限制、安全等级）
  4. Worker 执行具体动作，返回结果与日志
  5. Core Node 汇总结果，更新 Memory，并按需通知用户

- **自治层级**
  - 从 Roadmap 的 Phase 1 到 Phase 3，逐步提高：
    - Phase 1：所有动作只读 / 草稿，不做写操作
    - Phase 2：允许少量 Draft → Auto Safe 的迁移，需在 Persona/Playbook 中明确标注
    - Phase 3：在严格边界内允许部分自动执行（特别是模拟盘、家居场景）

### 5. 安全与信任在架构层面的落点

- **网络层与暴露面**
  - Core Node 对外只暴露必要的安全接口（如本地端口 + 受保护 Webhook）
  - Worker 节点通常只接受来自 Core Node 的请求，部署在受控网络中
  - 浏览器 Sidecar 与 Trading Worker 同机或同网段，减少攻击面

- **权限与配置集中管理**
  - 所有工具的风险等级和白名单配置集中维护于 Core Node
  - Worker 节点在本地再做一次白名单校验（双重防线）

- **审计与可追溯性**
  - Core Node 负责汇总：
    - 工具调用日志（时间、调用方、参数、结果）
    - 关键决策的「思考摘要」（为什么要这么做）
  - 这些信息都可以写入本地日志文件或数据库，用于事后审计和策略迭代

---

后续实现时，可以以本架构为蓝本，将具体的 openpup 配置、工具定义和 Worker 部署脚本逐步补充到本仓库中。

