use clap::{Parser, Subcommand};
use serde::Serialize;

/// openpup – Everywhere your second self works.
#[derive(Debug, Parser, Serialize)]
#[command(name = "openpup")]
#[command(about = "openpup CLI – front door for your second self", long_about = None)]
pub struct OpenpupCli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Command {
    /// 交互式初始化 openpup 的最小安全配置
    Init,
    /// 启动 openpup 守护进程（runtime + scheduler），与 down 对应
    #[command(name = "up")]
    Up,
    /// 优雅关闭由 openpup 管理的前台守护进程（提示使用 Ctrl+C）
    Down,
    /// 查看当前 openpup 的整体状态
    Status,

    /// Persona 文档：初始化模板、健康检查
    Persona(PersonaCmd),

    /// 记忆相关操作：如手动触发一次压缩/整理
    Memory(MemoryCmd),

    /// 安全模式：一键切换为只读或仅草稿模式（不执行外部写操作）
    Safety(SafetyCmd),

    /// 触发一次只读/草稿 Loop（如工作早晨计划、投资简报）
    Run(RunCmd),

    /// 启动主 Agent 会话（占位：未来多 Agent 协调入口）
    Agent(AgentCmd),

    /// 启动或声明一个子 Agent / 子分身（占位：未来 spawn 语义）
    Spawn(SpawnCmd),

    /// 多节点相关操作（占位：multi-node Agent OS）
    Node(NodeCmd),

    /// Cron 集成：单次执行某个 Loop，用于系统 crontab
    #[command(name = "cron")]
    Cron(CronCmd),

    /// 添加通道（写入 openpup 配置）；安全默认仅允许本人
    #[command(name = "add-channel")]
    AddChannel(AddChannelCmd),

    /// 添加工具/集成（写入 openpup 配置，不保存明文 token）
    #[command(name = "add-tool")]
    AddTool(AddToolCmd),

    /// 直接调用某个只读工具（用于验证集成可用性）
    #[command(name = "tool")]
    Tool(ToolCmd),

    /// 配置或更新 LLM / Agent 相关参数（写入 ~/.openpup/config.toml 的 llm 段）
    #[command(name = "add-agent")]
    AddAgent(AddAgentCmd),

    /// 显示最近的 Agent/Loop 活动概览（CLI Dashboard，占位实现）
    Dashboard,

    /// 规划型调用：让 LLM 产出一串工具调用计划并执行（实验性）。
    #[command(name = "plan")]
    Plan(PlanCmd),

    /// 列出已注册的子 Agent（来自 ~/.openpup/agents.toml）
    #[command(name = "agents")]
    AgentsList,

    /// 列出当前根据配置暴露给内核的工具
    #[command(name = "tools")]
    ToolsList,
}

#[derive(Debug, Parser, Serialize)]
pub struct AddChannelCmd {
    #[command(subcommand)]
    pub sub: AddChannelSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AddChannelSub {
    /// Telegram：写入 channels.telegram，allowed_users = ["me"]
    Telegram,
}

#[derive(Debug, Parser, Serialize)]
pub struct AddToolCmd {
    #[command(subcommand)]
    pub sub: AddToolSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AddToolSub {
    /// Home Assistant：保存 base_url/token_env/allowed_entities 到 ~/.openpup/config.toml
    #[command(name = "home-assistant")]
    HomeAssistant,
    /// Market（Stooq，无需 key）：保存 provider/watchlist 到 ~/.openpup/config.toml
    #[command(name = "market")]
    Market,
    /// News RSS：保存 RSS feeds 列表到 ~/.openpup/config.toml
    #[command(name = "news-rss")]
    NewsRss,
    /// IMAP：保存 host/port/username_env/password_env/mailbox 白名单到 ~/.openpup/config.toml
    #[command(name = "imap")]
    Imap,
    /// CalDAV：保存 base_url/username_env/password_env/calendar_urls 到 ~/.openpup/config.toml
    #[command(name = "caldav")]
    Caldav,
}

#[derive(Debug, Parser, Serialize)]
pub struct ToolCmd {
    #[command(subcommand)]
    pub sub: ToolSub,
}

#[derive(Debug, Parser, Serialize)]
pub struct AddAgentCmd {
    /// OpenAI base_url，例如 https://api.openai.com/v1
    #[arg(long = "base-url")]
    pub base_url: Option<String>,
    /// 模型名或别名，例如 gpt-4.1-mini / my-claude
    #[arg(long = "model")]
    pub model: Option<String>,
    /// 采样温度（0.0-2.0，默认 0.7）
    #[arg(long = "temperature")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ToolSub {
    /// Home Assistant：读取 entity 状态（L1）
    #[command(name = "ha:get-state")]
    HaGetState { entity_id: String },
    /// 行情：获取单个 symbol 报价（Stooq，L1）
    #[command(name = "market:quote")]
    MarketQuote { symbol: String },
    /// 资讯：抓取 RSS headlines（L1）
    #[command(name = "news:rss-headlines")]
    NewsRssHeadlines { limit: Option<usize> },
    /// 邮件：列出未读邮件标题（L1）
    #[command(name = "email:unread-subjects")]
    EmailUnreadSubjects {
        mailbox: Option<String>,
        limit: Option<usize>,
    },
    /// 日历：抓取今日事件（L1，基础）
    #[command(name = "caldav:events-today")]
    CaldavEventsToday { limit: Option<usize> },
    /// 任务：抓取 VTODO（L1，基础）
    #[command(name = "caldav:tasks")]
    CaldavTasks { limit: Option<usize> },
}

#[derive(Debug, Parser, Serialize)]
pub struct PersonaCmd {
    #[command(subcommand)]
    pub sub: PersonaSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum PersonaSub {
    /// 在 workspace 下创建 IDENTITY/SOUL/USER/TOOLS 模板
    Init,
    /// 检查 Persona 是否完整、安全（红线与权限是否声明）
    Doctor,
}

#[derive(Debug, Parser, Serialize)]
pub struct MemoryCmd {
    #[command(subcommand)]
    pub sub: MemorySub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum MemorySub {
    /// 请求对会话与语义记忆做一次压缩/整理
    Compact,
    /// 列出最近的语义记忆条目（按 created_ts 倒序）
    #[command(name = "list")]
    List {
        /// 可选：仅查看某个 kind（例如 work_log / invest_log / life_log）
        #[arg(long)]
        kind: Option<String>,
        /// 最大条数（默认 10）
        #[arg(long)]
        limit: Option<usize>,
    },
    /// 按关键字检索语义记忆
    #[command(name = "search")]
    Search {
        /// 检索关键字
        query: String,
        /// 可选：仅在某个 kind 中检索
        #[arg(long)]
        kind: Option<String>,
        /// 最大条数（默认 10）
        #[arg(long)]
        limit: Option<usize>,
    },
    /// 按 id 删除一条语义记忆
    #[command(name = "forget")]
    Forget {
        /// 要删除的语义记忆 id（可通过 list/search 查到）
        id: i64,
    },
}

#[derive(Debug, Parser, Serialize)]
pub struct SafetyCmd {
    #[command(subcommand)]
    pub sub: SafetySub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum SafetySub {
    /// 紧急降级为只读模式（仅允许 L1 工具）
    Readonly,
    /// 降级为仅草稿模式（允许 L1/L2，禁止 L3/L4）
    #[command(name = "draft-only")]
    DraftOnly,
}

#[derive(Debug, Parser, Serialize)]
pub struct PlanCmd {
    #[command(subcommand)]
    pub sub: PlanSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum PlanSub {
    /// 根据目标（goal）规划并执行一串工具调用，例如工作早晨简报。
    #[command(name = "run")]
    Run {
        /// 自由文本目标描述，例如 "work_morning" / "invest_morning"
        goal: String,
    },
    /// 将一段符合 CompositeToolFile schema 的 TOML 保存为 workspace 组合工具。
    #[command(name = "save-tool")]
    SaveTool {
        /// 组合工具定义的 TOML 字符串，或 @path 形式从文件读取
        spec: String,
    },
}

#[derive(Debug, Parser, Serialize)]
pub struct RunCmd {
    #[command(subcommand)]
    pub sub: RunSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum RunSub {
    /// 工作 – 早晨计划（只读）：从 workspace 读取今日任务，输出结构化摘要
    #[command(name = "work:morning")]
    WorkMorning,
    /// 工作 – 今日计划草稿（Phase 2 草稿）：基于 today_tasks.md 生成「今日计划草稿」
    #[command(name = "work:plan-draft")]
    WorkPlanDraft,
    /// 投资 – 早盘简报（只读）：从 workspace/logs 或占位生成简报
    #[command(name = "invest:morning")]
    InvestMorning,
    /// 投资 – 收盘复盘（只读）：从 workspace/logs 或占位生成复盘
    #[command(name = "invest:close")]
    InvestClose,
    /// 生活 – 早晨摘要（只读）：从 workspace/life_notes.md 读取内容并输出摘要
    #[command(name = "life:morning")]
    LifeMorning,
    /// 生活 – 晚间摘要（只读）：从 workspace/life_notes.md 读取内容并输出摘要
    #[command(name = "life:evening")]
    LifeEvening,
}

#[derive(Debug, Parser, Serialize)]
pub struct AgentCmd {
    /// 初始消息，例如「你现在是一个项目经理，需要完成 XX 任务」
    #[arg(short = 'm', long = "message")]
    pub message: Option<String>,
    /// 指定 persona 文件路径，例如 benalex_omni.md
    #[arg(long = "persona-file")]
    pub persona_file: Option<String>,
    /// 逗号分隔的子 Agent 名称列表，例如 researcher,writer,coder
    #[arg(long = "subagents")]
    pub subagents: Option<String>,
}

#[derive(Debug, Parser, Serialize)]
pub struct SpawnCmd {
    /// 子 Agent 名称，例如 researcher / writer / coder
    pub name: String,
    /// 使用的模型 ID，例如 qwen2.5 / claude-3.5-sonnet / deepseek-coder
    #[arg(long)]
    pub model: Option<String>,
    /// 内联 persona 描述，例如「资深研究员，擅长文献检索和分析」
    #[arg(long)]
    pub persona: Option<String>,
}

#[derive(Debug, Parser, Serialize)]
pub struct NodeCmd {
    #[command(subcommand)]
    pub sub: NodeSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum NodeSub {
    /// 在目标主机上声明/占位一个 Worker 节点（占位实现）
    #[command(name = "spawn")]
    Spawn {
        /// 节点名称
        name: String,
        /// 可选：目标主机或标签，例如 host1 / mac-mini / hk-cluster
        #[arg(long)]
        host: Option<String>,
    },
    /// 列出已知节点（占位实现）
    #[command(name = "list")]
    List,
}

#[derive(Debug, Parser, Serialize)]
pub struct CronCmd {
    #[command(subcommand)]
    pub sub: CronSub,
}

#[derive(Debug, Subcommand, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum CronSub {
    /// 立即执行一次指定 Loop（适合由系统 cron 调用）
    #[command(name = "run")]
    Run {
        /// Loop ID，例如 work_morning / invest_morning / life_evening 等
        loop_id: String,
    },
}
