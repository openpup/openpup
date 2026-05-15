import React, { useMemo, useState } from 'react';
import { useFinanceStore } from '../stores/financeStore';

const panel: React.CSSProperties = {
  borderRadius: 16,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  padding: '16px 18px',
};

const seedIntent = `[
  {
    "symbol": "600519",
    "market": "SSE",
    "direction": "buy",
    "thesis": "白酒龙头回调后成交额恢复，预期资金回流",
    "confidence": 0.76,
    "entry_rule": "回踩 1660 附近企稳分批买入",
    "exit_rule": "跌破 1600 止损，上看 1725",
    "max_position_pct": 0.12,
    "time_horizon": "3d",
    "valid_until": "2026-05-15T15:00:00+08:00",
    "risk_notes": "板块高位震荡，注意成交量衰减",
    "tool_evidence": ["近5日成交额回升", "机构研报继续维持买入"],
    "approval_status": "pending"
  }
]`;

const statusTone = (status?: string) => {
  if (status === 'approved') return { bg: 'rgba(29,158,117,0.12)', color: '#0E6A4C', label: 'approved' };
  if (status === 'reduced') return { bg: 'rgba(186,117,23,0.12)', color: '#8A5A10', label: 'reduced' };
  if (status === 'rejected') return { bg: 'rgba(226,75,74,0.12)', color: '#B81C1C', label: 'rejected' };
  return { bg: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', label: 'pending' };
};

const fieldLabelStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 700,
  color: 'var(--color-text-secondary)',
};

const fieldHintStyle: React.CSSProperties = {
  marginTop: 4,
  fontSize: 11,
  color: 'var(--color-text-tertiary)',
  lineHeight: 1.5,
};

export const FinancePipeline: React.FC = () => {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const {
    pipelineInput,
    setPipelineInput,
    intents,
    pipelineError,
    loading,
    batchCheckIntents,
    removeIntent,
    setIntents,
    updateIntent,
    applyPipelineInput,
    checkIntentAt,
    prepareOrderAt,
    placeOrderAt,
    clearOrderPreview,
    orderPreview,
    orderExecutionResult,
  } = useFinanceStore();

  const selectedIntent = intents[selectedIndex] ?? null;

  const summary = useMemo(() => ({
    total: intents.length,
    pending: intents.filter((item) => (item.approval_status ?? 'pending') === 'pending').length,
    approved: intents.filter((item) => item.approval_status === 'approved').length,
    reduced: intents.filter((item) => item.approval_status === 'reduced').length,
    rejected: intents.filter((item) => item.approval_status === 'rejected').length,
  }), [intents]);

  const stage = useMemo(() => {
    if (!selectedIntent) return 'draft';
    if (selectedIntent.approval_status === 'approved' || selectedIntent.approval_status === 'reduced') return 'risk-cleared';
    if (selectedIntent.approval_status === 'rejected') return 'risk-blocked';
    if (selectedIntent.entry_rule || selectedIntent.exit_rule || selectedIntent.max_position_pct) return 'ready-for-risk';
    return 'draft';
  }, [selectedIntent]);

  const stageTone = stage === 'risk-cleared'
    ? { label: '可进入执行准备', background: 'rgba(29,158,117,0.12)', color: '#0E6A4C' }
    : stage === 'risk-blocked'
      ? { label: '风控阻塞', background: 'rgba(226,75,74,0.12)', color: '#B81C1C' }
      : stage === 'ready-for-risk'
        ? { label: '待风控审批', background: 'rgba(186,117,23,0.12)', color: '#8A5A10' }
        : { label: '草稿编辑中', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)' };

  const nextAction = summary.pending > 0
    ? `优先处理 ${summary.pending} 条待审意图`
    : summary.rejected > 0
      ? `复核 ${summary.rejected} 条被拒意图`
      : summary.reduced > 0
        ? `确认 ${summary.reduced} 条降仓建议`
        : summary.approved > 0
          ? `已通过 ${summary.approved} 条，可进入执行准备`
          : '先导入或生成一条交易意图';

  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <section style={{ ...panel, display: 'grid', gap: 14, background: 'linear-gradient(180deg, rgba(255,255,255,0.96), rgba(247,248,245,0.98))' }}>
        <div style={{ display: 'grid', gap: 14 }}>
          <div style={{ display: 'flex', alignItems: 'end', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap' }}>
            <div>
              <div style={{ fontSize: 14, fontWeight: 700 }}>Pipeline</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                在这里整理交易意图、补齐规则并完成风控审批。
              </div>
            </div>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              {[
                ['总数', summary.total, '#0E6A4C', 'rgba(16,59,47,0.08)'],
                ['待审', summary.pending, '#1A5EA0', 'rgba(55,138,221,0.08)'],
                ['通过', summary.approved, '#0E6A4C', 'rgba(29,158,117,0.10)'],
                ['降仓', summary.reduced, '#8A5A10', 'rgba(186,117,23,0.10)'],
                ['拒绝', summary.rejected, '#B81C1C', 'rgba(226,75,74,0.10)'],
              ].map(([label, value, color, bg]) => (
                <div key={String(label)} style={{ minWidth: 78, borderRadius: 999, padding: '8px 10px', background: String(bg), color: String(color), display: 'flex', alignItems: 'center', gap: 8, justifyContent: 'space-between' }}>
                  <span style={{ fontSize: 11, fontWeight: 700 }}>{label}</span>
                  <span style={{ fontSize: 13, fontWeight: 760 }}>{value}</span>
                </div>
              ))}
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap', paddingTop: 2 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <span style={{ fontSize: 11, fontWeight: 700, color: 'var(--color-text-secondary)' }}>当前处理</span>
              <span style={{ borderRadius: 999, padding: '4px 8px', fontSize: 11, fontWeight: 700, background: stageTone.background, color: stageTone.color }}>
                {stageTone.label}
              </span>
              <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>{nextAction}</span>
            </div>

            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button
                onClick={() => void batchCheckIntents()}
                style={{ borderRadius: 10, border: 'none', background: '#103B2F', color: '#D7F4E8', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
              >
                {loading.pipeline ? '风控校验中…' : '批量风控校验'}
              </button>
              <button
                onClick={() => setPipelineInput(seedIntent)}
                style={{ borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
              >
                填充示例
              </button>
              <button
                onClick={() => setIntents([])}
                style={{ borderRadius: 10, border: '1px solid rgba(226,75,74,0.25)', background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
              >
                清空列表
              </button>
            </div>
          </div>
        </div>
      </section>

      <section style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1.3fr) minmax(340px, 0.92fr)', gap: 16 }}>
        <div style={{ ...panel, display: 'grid', gap: 12, alignContent: 'start' }}>
          <div style={{ display: 'flex', alignItems: 'end', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
            <div>
              <div style={{ fontSize: 14, fontWeight: 700 }}>意图列表</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                选择需要处理的标的，在右侧补齐规则或查看审批结果。
              </div>
            </div>
            <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
              当前选中: <strong style={{ color: 'var(--color-text-primary)' }}>{selectedIntent?.symbol ?? '--'}</strong>
            </div>
          </div>

          <div style={{ borderRadius: 12, border: '1px solid var(--color-border-tertiary)', background: 'var(--color-background-secondary)', padding: '10px 12px', display: 'grid', gap: 10 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
              <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-secondary)' }}>批量导入</div>
              <button
                onClick={() => applyPipelineInput()}
                style={{ borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', color: 'var(--color-text-secondary)', fontSize: 12, padding: '7px 12px', cursor: 'pointer' }}
              >
                应用导入内容
              </button>
            </div>
            <textarea
              value={pipelineInput}
              onChange={(e) => setPipelineInput(e.target.value)}
              placeholder="粘贴 TradeIntent JSON 数组"
              style={{ minHeight: 110, resize: 'vertical', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', color: 'var(--color-text-primary)', padding: '10px 12px', fontSize: 12, lineHeight: 1.7, fontFamily: 'var(--font-mono)' }}
            />
            {pipelineError && (
              <div style={{ borderRadius: 10, padding: '8px 10px', background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', fontSize: 12 }}>
                {pipelineError}
              </div>
            )}
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr 1fr', gap: 10 }}>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(55,138,221,0.08)', border: '1px solid rgba(55,138,221,0.12)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#1A5EA0' }}>待处理</div>
              <div style={{ marginTop: 8, fontSize: 13, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                {summary.pending > 0
                  ? `${summary.pending} 条待审，优先补齐 entry / exit / 仓位。`
                  : '当前没有待审意图。'}
              </div>
            </div>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(226,75,74,0.08)', border: '1px solid rgba(226,75,74,0.12)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#B81C1C' }}>Rejected</div>
              <div style={{ marginTop: 8, fontSize: 24, fontWeight: 760, color: '#B81C1C' }}>{summary.rejected}</div>
              <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>查看拒绝原因</div>
            </div>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(186,117,23,0.08)', border: '1px solid rgba(186,117,23,0.12)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#8A5A10' }}>Reduced</div>
              <div style={{ marginTop: 8, fontSize: 24, fontWeight: 760, color: '#8A5A10' }}>{summary.reduced}</div>
              <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>评估降仓建议</div>
            </div>
          </div>

          <div style={{ overflowX: 'auto', borderRadius: 12, border: '1px solid var(--color-border-tertiary)', minHeight: 420 }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12, background: 'var(--color-background-primary)' }}>
              <thead>
                <tr>
                  {['标的', '方向', '理由', '信心度', '入场', '出场', '仓位', '审批', '操作'].map((column) => (
                    <th key={column} style={{ textAlign: 'left', padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-tertiary)', fontWeight: 600, background: 'var(--color-background-secondary)' }}>
                      {column}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {intents.map((intent, index) => {
                  const tone = statusTone(intent.approval_status);
                  return (
                    <tr
                      key={`${intent.symbol}-${index}`}
                      onClick={() => setSelectedIndex(index)}
                      style={{ background: selectedIndex === index ? 'rgba(16,59,47,0.05)' : 'transparent', cursor: 'pointer' }}
                    >
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', fontWeight: 700 }}>
                        {intent.symbol}
                        <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>{intent.market}</div>
                      </td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{intent.direction}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', minWidth: 220, color: 'var(--color-text-secondary)' }}>{intent.thesis || '--'}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{intent.confidence ?? '--'}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', minWidth: 160 }}>{intent.entry_rule || '--'}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', minWidth: 160 }}>{intent.exit_rule || '--'}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{intent.adjusted_position_pct ?? intent.max_position_pct ?? '--'}</td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)' }}>
                        <span style={{ borderRadius: 999, padding: '4px 8px', fontSize: 11, fontWeight: 700, background: tone.bg, color: tone.color }}>
                          {tone.label}
                        </span>
                      </td>
                      <td style={{ padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)' }}>
                        <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
                          <button
                            onClick={(event) => {
                              event.stopPropagation();
                              void checkIntentAt(index);
                            }}
                            style={{ border: 'none', background: 'transparent', color: '#0E6A4C', cursor: 'pointer', fontSize: 12, padding: 0 }}
                          >
                            校验
                          </button>
                          <button
                            onClick={(event) => {
                              event.stopPropagation();
                              removeIntent(intent.symbol, index);
                              setSelectedIndex(Math.max(0, index - 1));
                            }}
                            style={{ border: 'none', background: 'transparent', color: 'var(--color-text-danger)', cursor: 'pointer', fontSize: 12, padding: 0 }}
                          >
                            删除
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          {intents.length === 0 && (
            <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
              还没有候选意图。先去 Research 选择一只标的并生成草稿，或者使用底部导入区粘贴 JSON。
            </div>
          )}
        </div>

        <div style={{ display: 'grid', gap: 16, alignContent: 'start' }}>
          <div style={{ ...panel, display: 'grid', gap: 12 }}>
            <div>
              <div style={{ fontSize: 14, fontWeight: 700 }}>当前意图</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                编辑当前意图，并查看这条意图的审批结果。
              </div>
            </div>

            {selectedIntent ? (
              <>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, borderRadius: 12, padding: '10px 12px', background: statusTone(selectedIntent.approval_status).bg }}>
                  <div>
                    <div style={{ fontSize: 12, fontWeight: 700 }}>{selectedIntent.symbol} · {selectedIntent.direction}</div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>{selectedIntent.market}</div>
                  </div>
                  <span style={{ fontSize: 11, fontWeight: 700, color: statusTone(selectedIntent.approval_status).color }}>
                    {statusTone(selectedIntent.approval_status).label}
                  </span>
                </div>

                <div style={{ display: 'grid', gap: 10 }}>
                  <div>
                    <div style={fieldLabelStyle}>交易理由</div>
                    <div style={fieldHintStyle}>简要说明为什么关注这只标的，以及当前判断依据。</div>
                    <input
                      value={selectedIntent.thesis ?? ''}
                      onChange={(e) => updateIntent(selectedIndex, { thesis: e.target.value, approval_status: 'pending' })}
                      placeholder="例如：板块回调后资金回流，成交额明显放大"
                      style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
                    />
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                    <div>
                      <div style={fieldLabelStyle}>信心度</div>
                      <div style={fieldHintStyle}>0 到 1 之间，用来表示当前判断强度。</div>
                      <input
                        value={String(selectedIntent.confidence ?? '')}
                        onChange={(e) => updateIntent(selectedIndex, { confidence: Number(e.target.value || 0), approval_status: 'pending' })}
                        placeholder="例如：0.72"
                        style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
                      />
                    </div>
                    <div>
                      <div style={fieldLabelStyle}>仓位占比</div>
                      <div style={fieldHintStyle}>建议最大仓位，占总资产比例。</div>
                      <input
                        value={String(selectedIntent.max_position_pct ?? '')}
                        onChange={(e) => updateIntent(selectedIndex, { max_position_pct: Number(e.target.value || 0), approval_status: 'pending' })}
                        placeholder="例如：0.12"
                        style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
                      />
                    </div>
                  </div>
                  <div>
                    <div style={fieldLabelStyle}>入场规则</div>
                    <div style={fieldHintStyle}>写清价格、区间或触发条件，便于后续审批和执行。</div>
                    <input
                      value={selectedIntent.entry_rule ?? ''}
                      onChange={(e) => updateIntent(selectedIndex, { entry_rule: e.target.value, approval_status: 'pending' })}
                      placeholder="例如：回踩 1660 附近企稳后分批买入"
                      style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
                    />
                  </div>
                  <div>
                    <div style={fieldLabelStyle}>出场规则</div>
                    <div style={fieldHintStyle}>包含止盈、止损或失效条件。</div>
                    <input
                      value={selectedIntent.exit_rule ?? ''}
                      onChange={(e) => updateIntent(selectedIndex, { exit_rule: e.target.value, approval_status: 'pending' })}
                      placeholder="例如：跌破 1600 止损，上看 1725"
                      style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
                    />
                  </div>
                  <div>
                    <div style={fieldLabelStyle}>风险备注</div>
                    <div style={fieldHintStyle}>记录当前判断里最需要注意的风险点。</div>
                    <textarea
                      value={selectedIntent.risk_notes ?? ''}
                      onChange={(e) => updateIntent(selectedIndex, { risk_notes: e.target.value, approval_status: 'pending' })}
                      placeholder="例如：板块高位震荡，注意成交量衰减"
                      style={{ marginTop: 8, width: '100%', boxSizing: 'border-box', minHeight: 84, resize: 'vertical', borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12, lineHeight: 1.6 }}
                    />
                  </div>
                </div>

                <div style={{ display: 'grid', gap: 10 }}>
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                    <button
                      onClick={() => void checkIntentAt(selectedIndex)}
                      style={{ borderRadius: 10, border: 'none', background: '#103B2F', color: '#D7F4E8', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
                    >
                      {loading.pipeline ? '处理中…' : '仅校验当前意图'}
                    </button>
                    <button
                      onClick={() => void prepareOrderAt(selectedIndex)}
                      disabled={!selectedIntent || !['approved', 'reduced'].includes(selectedIntent.approval_status ?? '')}
                      style={{ borderRadius: 10, border: '1px solid rgba(186,117,23,0.18)', background: ['approved', 'reduced'].includes(selectedIntent.approval_status ?? '') ? 'rgba(186,117,23,0.10)' : 'var(--color-background-secondary)', color: ['approved', 'reduced'].includes(selectedIntent.approval_status ?? '') ? '#8A5A10' : 'var(--color-text-tertiary)', fontSize: 12, padding: '9px 12px', cursor: ['approved', 'reduced'].includes(selectedIntent.approval_status ?? '') ? 'pointer' : 'not-allowed' }}
                    >
                      下单预览
                    </button>
                  </div>

                  <div style={{ borderRadius: 12, padding: '12px 14px', background: 'var(--color-background-secondary)', display: 'grid', gap: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                    <div>风险标志: {(selectedIntent.risk_flags ?? []).join(', ') || '--'}</div>
                    <div>证据: {(selectedIntent.tool_evidence ?? []).slice(0, 3).join(' | ') || '--'}</div>
                    <div>检查时间: {selectedIntent.checked_at ?? '待校验'}</div>
                    {selectedIntent.rejection_reason && <div style={{ color: 'var(--color-text-danger)' }}>拒绝原因: {selectedIntent.rejection_reason}</div>}
                  </div>

                  {orderPreview && selectedIntent.symbol === orderPreview.symbol && (
                    <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(186,117,23,0.08)', border: '1px solid rgba(186,117,23,0.16)', display: 'grid', gap: 8 }}>
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 700, color: '#8A5A10' }}>订单预览</div>
                        <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{orderPreview.order_type}</span>
                      </div>
                      <div style={{ display: 'grid', gap: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                        <div>方向: {orderPreview.order_direction}</div>
                        <div>价格: {orderPreview.price}</div>
                        <div>数量: {orderPreview.quantity}</div>
                        <div>金额: {orderPreview.amount.toFixed(2)}</div>
                        <div>仓位: {(orderPreview.position_pct * 100).toFixed(2)}%</div>
                        {orderPreview.thesis && <div>理由: {orderPreview.thesis}</div>}
                      </div>
                      <div style={{ display: 'grid', gap: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                        {orderPreview.notes.map((note) => (
                          <div key={note}>{note}</div>
                        ))}
                      </div>
                      <div style={{ display: 'flex', gap: 10 }}>
                        <button
                          onClick={() => void placeOrderAt(selectedIndex)}
                          style={{ borderRadius: 10, border: 'none', background: '#8A5A10', color: '#FFF7EB', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
                        >
                          确认下单
                        </button>
                        <button
                          onClick={() => clearOrderPreview()}
                          style={{ borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', color: 'var(--color-text-secondary)', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
                        >
                          取消
                        </button>
                      </div>
                    </div>
                  )}

                  {orderExecutionResult && (
                    <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(29,158,117,0.08)', border: '1px solid rgba(29,158,117,0.16)', display: 'grid', gap: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                      <div style={{ fontSize: 12, fontWeight: 700, color: '#0E6A4C' }}>下单结果</div>
                      <div>状态: {String(orderExecutionResult.status ?? '--')}</div>
                      <div>委托号: {String(orderExecutionResult.order_id ?? '--')}</div>
                      <div>价格: {String(orderExecutionResult.price ?? '--')}</div>
                      <div>数量: {String(orderExecutionResult.quantity ?? '--')}</div>
                      <div>{String(orderExecutionResult.message ?? '已提交委托')}</div>
                    </div>
                  )}
                </div>
              </>
            ) : (
              <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                先从左侧列表选择一条意图，右侧才会出现编辑字段和风险结果。
              </div>
            )}
          </div>

          <div style={{ ...panel, display: 'grid', gap: 12 }}>
            <div>
              <div style={{ fontSize: 14, fontWeight: 700 }}>最近结果</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                最近几条意图的审批痕迹。
              </div>
            </div>

            <div style={{ display: 'grid', gap: 12 }}>
              {intents.slice(0, 3).map((intent, index) => (
                <div key={`${intent.symbol}-detail-${index}`} style={{ borderRadius: 14, border: '1px solid var(--color-border-tertiary)', background: 'var(--color-background-secondary)', padding: '12px 14px' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                    <div style={{ fontSize: 13, fontWeight: 700 }}>{intent.symbol} · {intent.direction}</div>
                    <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{intent.checked_at ?? '待校验'}</span>
                  </div>
                  <div style={{ marginTop: 10, display: 'grid', gap: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                    <div>风险标志: {(intent.risk_flags ?? []).join(', ') || '--'}</div>
                    <div>证据: {(intent.tool_evidence ?? []).slice(0, 2).join(' | ') || '--'}</div>
                    {intent.rejection_reason && <div style={{ color: 'var(--color-text-danger)' }}>原因: {intent.rejection_reason}</div>}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
};
