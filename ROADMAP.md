# OpenPup ROADMAP

> OpenPup：你的个人分身 OS（Core + Workers 王朝中枢）。  
> 这里只追踪「王朝中枢」和分身体验这条主线，而不是「做一个通用 Agent 平台」。

---

## 1. 当前状态（当前版本）

### 1.1 已落地能力

- **Core / 分身大脑**
  - Persona + 三层记忆（短期 / 长期 / 程序性）的基础实现。  
  - `openpup agent` / `openpup agent -m`：带记忆的单轮与多轮对话。  
  - 统一 LLM 配置（`~/.openpup/config.toml` + env），通过 `llm::load_openai_from_config` 等接口使用。

- **Loops / Scheduler**
  - 文件驱动的 Loop（`work:*` / `invest:*` / `life:*` 等），`openpup run <loop_id>` 单次执行。  
  - `openpup up`：长驻调度器，按时间规则触发 Loop。  
  - Loop 运行过程和结果写入 runtime 审计与记忆。

- **Tools / 组合工具**
  - L1 只读工具（如 Home Assistant 状态、Market 行情、RSS、IMAP、CalDAV 等）。  
  - 组合工具：`~/.openpup/workspace/tools/*.toml` 声明多步调用，执行时自动递归展开。  
  - 工具调用统一走审计日志，带上工具等级信息。

- **SubAgents / Nodes**
  - 本地 SubAgent registry（`~/.openpup/agents.toml`），支持 `openpup spawn` 与 `invoke_sub_agent`。  
  - Node registry（`~/.openpup/nodes.toml`），支持 `openpup node spawn/list` 与 `invoke_node_tool`。  
  - 内置 `node_worker` 示例，演示最小 Core + Worker 联调路径。

- **审计与 Dashboard**
  - CLI 调用审计（`audit.log`）与 runtime 活动审计（`runtime-audit.log`）。  
  - `openpup dashboard`：按 Loops / Tool calls / All activity 维度统计。

### 1.2 安全集与风险分层

- 工具等级 L1–L3 的基本约束与执行路径已落地，配置方式见 `docs/tools-autonomy-safety.md`。  
- L3 小权限自治工具（如本地 TODO/进度/决策日志）已具备基础读写与审计能力。  
- 基础紧急降权能力：通过配置手动将 execution mode 降为只读 / draft-only。

---

## 2. 短期规划（下一阶段优先级）

> 目标版本：**v0.2.x**  
> 方向：让「一个人的王朝」在安全前提下更好用，而不是堆更多通用 Agent 功能或接更多外部集成。

### 2.1 分身体验

- **更顺手的 Loop 模板**
  - 提供更完整的 `work:*` / `invest:*` / `life:*` Loop 示例与注释。  
  - 支持通过简单命令生成/克隆 Loop（便于个人定制）。

- **记忆与审计的可视化入口（最小版）**
  - 在 `openpup dashboard` 中加入更清晰的「时间线视图」和「按领域过滤」（工作/投资/生活）。  
  - 提供简单的过滤参数（如最近 N 条、按工具等级过滤）。

### 2.2 安全与自治

- **execution_mode 与工具等级的硬约束**
  - 在工具执行路径中，彻底打通 `readonly` / `draft-only` / `full` 与 L1–L4 的矩阵限制。  
  - 触发被拒绝时，写入清晰的审计事件（含期望等级与当前 mode）。

- **紧急降权的一键操作**
  - `openpup safety readonly` / `draft-only` 等命令稳定化，并在审计中标记为 `safety_emergency`。  
  - 提供最小的「当前安全状态」查询接口（CLI 子命令或 dashboard 视图）。

### 2.3 Core + Worker 通路打磨

- **NodeTransport 抽象收敛**
  - 把 HTTP Node 调用链整理成清晰的 trait + 默认实现。  
  - 保证未来可以在不动上层逻辑的情况下换成 WS/队列等实现。

- **Worker 模板与脚手架**
  - 提供可 `cargo generate` 的 Worker 模板（最小 HTTP /tool 实现 + 示例工具）。  

---

## 3. 中期规划（稳定之后再做）

### 3.1 小权限自治（L3）完善

- 更丰富的 L3 工具族（如进度、决策树、长期项目状态等），全部限定在本机与低风险范围内。  
- 在语义和配置上把「可自治的范围」描述清楚，避免误用。

### 3.2 审批与外部入口

- 审批工具链（ApprovalToolExecutor）打通到简单的 UI 或 IM 渠道（如飞书卡片）。  
- 对高风险工具（L4）强制走审批流，将决策与结果写入专门的审计轨迹。

### 3.3 更强的 Dashboard

- 提供 HTTP 只读接口，允许挂一个简单的 Web/TUI 仪表盘。  
- 在时间、领域、工具等级、节点等维度上做更灵活的过滤/聚合。

---

## 4. Breaking changes（只记录对使用者有感的）

> 仅保留和「如何用/怎么升级」强相关的条目，内部重构不在此列；不再绑定具体日期/年份。

- **RuntimeEvent 改为 enum**  
  - 从 struct 变为 enum（如 `Loop { loop_id, trigger, source }` 等）。  
  - 若你直接构造 `RuntimeEvent`，需要按新枚举形式调整；CLI 使用无需修改。

- **LLM 客户端接口统一**  
  - 移除旧的 `LlmConfig` / `complete()` 等占位接口。  
  - 统一使用 `OpenpupLlmConfig` 与 `load_openai_from_config` / `chat_once` / `chat_with_memory` / `tool_planner`。  
  - 若你在自定义代码中依赖旧接口，请对照源码替换为上述新 API。

- **execution_mode 与工具等级硬约束**  
  - 工具执行路径中强制执行 execution_mode × L1–L4 矩阵：`readonly` 仅允许 L1，`draft-only` 允许 L1/L2，`full` 允许 L1–L3（L4 仍走审批）。  
  - 此前若在 `readonly` 下曾依赖 L2 执行，升级后会被拒绝；请改用 `openpup safety draft-only` 或将 config.toml 中 `execution_mode` 设为 `draft-only` / `full`。  
  - 被拒绝时会写入 `safety_denied` 审计事件，并可在 `openpup dashboard` 中查看近期安全事件。
