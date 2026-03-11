#!/usr/bin/env bash
set -euo pipefail

# 场景：研报晨间速递（模板脚本）
# 作用：
# 1. 在配置中暴露研报工具；
# 2. 提示你使用 `openpup spawn` 创建以下子 Agent：
#    - research_fetcher
#    - research_analyst
#    - research_integrator
#    - brief_writer
# 3. 在 workspace/tools 下写入一个组合工具 TOML：`research_morning_digest.toml`，
#    作为「晨间简报」场景模板（方便在对话中调用）。

CONFIG_PATH="${HOME}/.openpup/config.toml"
WORKSPACE_ROOT="${HOME}/.openpup/workspace"
TOOLS_DIR="${WORKSPACE_ROOT}/tools"

mkdir -p "${TOOLS_DIR}"

echo "== 1) 提示：请在 config.toml 中增加研报工具暴露条目 =="
cat <<'EOF'

在 ~/.openpup/config.toml 的 [tools] 段中添加（或通过 openpup CLI 交互添加）：

[[tools]]
id = "research_reports_search"
name = "research_reports_search"
description = "按关键词和时间窗口搜索最新的研报元数据（示例实现，当前为 mock）。"
level = "L1"
args = "{\"keywords\": string[], \"since_hours\": optional number, \"limit\": optional number}"

[[tools]]
id = "research_reports_fetch"
name = "research_reports_fetch"
description = "根据 URL 抓取单份研报的核心内容（示例实现，当前为 mock）。"
level = "L1"
args = "{\"url\": string}"

EOF

echo "== 2) 提示：创建子 Agent（手动执行以下命令一次） =="
cat <<'EOF'

示例（可按需修改 persona 文本）：

openpup spawn research_fetcher
openpup spawn research_analyst
openpup spawn research_integrator
openpup spawn brief_writer

然后在 ~/.openpup/workspace/persona/ 下为以上 Agent 编写/调整 persona：
- research_fetcher：负责调用 research_reports_* 工具，输出结构化 JSON 列表。
- research_analyst：负责对单份/多份研报提炼核心结论与风险提示。
- research_integrator：负责跨主题去重、筛选、排序，并汇总全局观点/风险。
- brief_writer：负责根据结构化结果生成 Markdown 晨报。

EOF

echo "== 3) 写入组合工具模板：research_morning_digest.toml =="
cat > "${TOOLS_DIR}/research_morning_digest.toml" <<'EOF'
id = "research_morning_digest"
name = "research_morning_digest"
description = "研报晨间速递：按关键词搜索最新研报，并为后续 Agent 编排提供统一入口。"
level = "L1"
args = """{
  \"keywords\": string[],
  \"since_hours\": optional number,
  \"limit\": optional number
}"""

[[steps]]
tool = "research_reports_search"
args = { keywords = [], since_hours = 24, limit = 5 }

# 说明：
# - 这是一个「场景模板」，默认 keywords 为空，仅作为结构示例；
# - 实际使用时，建议在对话 / orchestrator 中直接调用底层工具
#   research_reports_search，并把用户输入的关键词作为参数传入；
# - 或者你可以复制本文件，改成自己的场景版本（例如专门为某几个长期关注主题设定默认关键词）。
EOF

echo
echo "完成：已在 ${TOOLS_DIR}/research_morning_digest.toml 写入场景模板。"
echo "你可以在对话中让 Agent 调用组合工具：research_morning_digest，或用 orchestrator 结合子 Agent 跑完整晨报工作流。"

