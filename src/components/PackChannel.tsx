import React from 'react';

// ─── Pack Channel placeholder ─────────────────────────────────────────────────
//
// Pack Channel 是 openpup 规划中的多 Pup 异步协作系统。
// 当前版本（并行 fan-out）的逻辑已合并到普通 Pup 路由中：
// Alpha 判断为多专业任务时，各 Pup 并行执行、结果由 Alpha 汇总后直接回到 Chat。
//
// 真正的 Pack Channel 需要：
//   1. Tokio broadcast 消息总线（channel/manager.rs）
//   2. Pup 间 @mention 通信（writer → @research 提问，等待回答再继续）
//   3. 顺序/依赖感知调度（而非无差别并行）
//   4. artifact 文件落盘（~/workspace/channels/{id}/）
//   5. 用户旁观界面（实时回放 Pup 群聊过程）
//
// 该功能计划在后续版本实现。

const features = [
  {
    icon: '💬',
    title: 'Pup 间实时通信',
    desc: 'Research Pup 完成调研后，直接 @Writer Pup 传递数据；Writer 可以反向提问请求补充。',
  },
  {
    icon: '🔀',
    title: '依赖感知调度',
    desc: 'Alpha 分析任务依赖关系，按需串行或并行，而非无差别同时启动所有 Pup。',
  },
  {
    icon: '📁',
    title: 'Artifact 文件落盘',
    desc: 'Pup 的中间产物（调研笔记、草稿、代码片段）写入本地文件，路径记录在数据库供回溯。',
  },
  {
    icon: '👁️',
    title: '用户旁观模式',
    desc: '在此页面实时查看 Pup 之间的对话全过程，像阅读一场会议记录一样透明可审计。',
  },
  {
    icon: '⏱️',
    title: '超时与阻塞检测',
    desc: '单个 Pup 超过 60 秒无响应时，Alpha 标记 blocked 并通知用户，避免静默卡死。',
  },
];

export const PackChannel: React.FC = () => {
  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-4 py-3 border-b border-stone-800 shrink-0">
        <h2 className="text-sm font-semibold text-stone-200">Pack Channel</h2>
        <p className="text-[11px] text-stone-500 mt-0.5">多 Pup 异步协作系统 · 即将推出</p>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-auto px-5 py-6 flex flex-col items-center gap-6">
        {/* Hero */}
        <div className="text-center max-w-xs">
          <div className="text-4xl mb-3">🐾</div>
          <p className="text-stone-300 text-sm font-medium leading-relaxed">
            让 Pup 们像真正的团队一样协作
          </p>
          <p className="text-stone-500 text-[11px] mt-1.5 leading-relaxed">
            当前版本的多 Pup 任务会并行执行并由 Alpha 汇总结果直接返回到 Chat。
            Pack Channel 将支持 Pup 之间互相传递信息、提问和等待，实现真正的异步协作。
          </p>
        </div>

        {/* Feature cards */}
        <div className="w-full max-w-sm space-y-2.5">
          {features.map((f) => (
            <div
              key={f.title}
              className="bg-stone-800/50 border border-stone-700/40 rounded-xl px-4 py-3 flex gap-3"
            >
              <span className="text-xl shrink-0 mt-0.5">{f.icon}</span>
              <div>
                <div className="text-stone-200 text-[12px] font-medium">{f.title}</div>
                <div className="text-stone-500 text-[11px] mt-0.5 leading-relaxed">{f.desc}</div>
              </div>
            </div>
          ))}
        </div>

        <p className="text-stone-700 text-[10px] text-center pb-2">
          设计文档：docs/architecture.md § Pack Channel
        </p>
      </div>
    </div>
  );
};
