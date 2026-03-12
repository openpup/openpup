# Worker 快速入门

在另一台机器（或本机）上运行一个 OpenPup Worker 节点，让 Core 通过 `invoke_node_tool` 调用该节点上的工具。

## 1. 构建与运行 Worker

本仓库内置 `node_worker` 二进制，与 CLI 一起构建：

```bash
cd openpup
cargo build --release
# Worker 二进制
./target/release/node_worker
```

默认监听 `127.0.0.1:8080`，可通过环境变量覆盖：

```bash
OPENPUP_NODE_WORKER_ADDR=0.0.0.0:8080 ./target/release/node_worker
```

Worker 暴露 HTTP 接口：

- **POST /tool**：请求体 `{ "tool": "<name>", "args": {} }`，响应 `{ "ok": bool, "value": ..., "error": ... }`。

内置示例工具：

- **health_check**（L1）：返回版本与系统信息，供 `openpup node test` 使用。
- **disk_usage_readonly**（L1）：只读磁盘/工作目录信息。
- **echo** / **time_now**：演示用。

## 2. 在 Core 侧注册并测试节点

在运行 OpenPup CLI 的机器上：

```bash
# 注册节点（host 为 Worker 的 base URL，不要带 /tool 后缀）
openpup node spawn my-worker --host http://192.168.1.100:8080

# 列出节点
openpup node list

# 测试连通性（调用远端 health_check）
openpup node test my-worker
```

若 `node test` 返回 OK，即可在 Agent 或 Plan 中通过 `invoke_node_tool` 工具使用该节点。

## 3. 测试节点协作

`invoke_node_tool` 为 L2 工具，需 `execution_mode` 为 `draft-only` 或 `full`（默认 `readonly` 仅允许 L1）。若当前为只读，先执行：

```bash
openpup safety draft-only
```

然后启动交互式 Agent，在对话中让分身调用节点上的工具：

```bash
openpup agent
```

在提示符 `you>` 后输入例如：

- **中文**：`调用 dev-node-1 节点上的 disk_usage_readonly 工具`
- **或**：`用 invoke_node_tool 在节点 dev-node-1 上执行 time_now`

分身会返回类似：`pup (tool request): ... -> ToolResult { ok: true, value: Some(...) }`，以及自然语言回复。也可让分身执行 `echo`、`health_check` 等 Worker 已暴露的工具。

单轮示例（不进入交互）：

```bash
openpup agent -m "在节点 dev-node-1 上执行 disk_usage_readonly，把结果简要告诉我"
```

## 4. 多机部署示意

1. **机器 A（Core）**：运行 `openpup up` 或使用 `openpup agent` / `openpup plan run`。
2. **机器 B（Worker）**：运行 `node_worker`，并确保 A 能访问 B 的 host:port。
3. 在 A 上执行 `openpup node spawn <name> --host http://B:8080`，然后 `openpup node test <name>` 验证。

Worker 与 Core 之间当前为 HTTP JSON，无内置鉴权；若部署在不可信网络，请自行加反向代理或 VPN。
