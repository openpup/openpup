import React, { useEffect, useMemo, useState } from 'react';
import { useLang } from '../i18n';
import type {
  ActivityStep,
  ChatMessage,
  StreamingPupState,
  FinanceArtifactPayload,
  FinanceIntentPayload,
} from '../stores/chatStore';
import { FINANCE_CONNECTOR_CAPABILITY_KEYS, type FinanceCapabilityKey, type FinanceScenarioConfig } from '../stores/scenarioStore';

const panelStyle: React.CSSProperties = {
  width: 'clamp(400px, 28vw, 460px)',
  flexShrink: 0,
  borderLeft: '0.5px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  overflowY: 'auto',
};

const cardStyle: React.CSSProperties = {
  borderRadius: 14,
  border: '0.5px solid var(--color-border-tertiary)',
  background: 'var(--color-background-secondary)',
  padding: '14px',
};

const segmentButtonStyle: React.CSSProperties = {
  border: 'none',
  borderRadius: 10,
  padding: '7px 10px',
  fontSize: 11,
  fontWeight: 600,
  cursor: 'pointer',
};

const skillLabels: Record<string, { zh: string; en: string }> = {
  premarket_scan: { zh: '盘前扫描', en: 'Premarket Scan' },
  intraday_check: { zh: '盘中评估', en: 'Intraday Check' },
  postmarket_review: { zh: '收盘复盘', en: 'Postmarket Review' },
  watchlist_cleanup: { zh: '自选维护', en: 'Watchlist Cleanup' },
  emergency_stop: { zh: '紧急止损', en: 'Emergency Stop' },
};

const capabilityLabels: Record<FinanceCapabilityKey, { zh: string; en: string }> = {
  search_news: { zh: '资讯检索', en: 'News Search' },
  get_quote: { zh: '实时行情', en: 'Quote' },
  get_candles: { zh: 'K线 / Bar', en: 'Candles' },
  screen_symbols: { zh: '条件选股', en: 'Screen Symbols' },
  list_watchlist: { zh: '读取自选', en: 'List Watchlist' },
  add_watchlist: { zh: '加入自选', en: 'Add Watchlist' },
  remove_watchlist: { zh: '移除自选', en: 'Remove Watchlist' },
  review_trade_intent: { zh: '审批 Intent', en: 'Review Intent' },
  validate_order: { zh: '订单校验', en: 'Validate Order' },
  validate_positions: { zh: '持仓校验', en: 'Validate Positions' },
  validate_market_status: { zh: '市场状态校验', en: 'Validate Market Status' },
  validate_exposure: { zh: '敞口校验', en: 'Validate Exposure' },
  get_account: { zh: '账户总览', en: 'Account Summary' },
  get_positions: { zh: '持仓查询', en: 'Get Positions' },
  list_orders: { zh: '委托列表', en: 'List Orders' },
  get_order_status: { zh: '委托状态', en: 'Order Status' },
  place_order: { zh: '下单', en: 'Place Order' },
  cancel_order: { zh: '撤单', en: 'Cancel Order' },
};

interface ParsedTradeIntent {
  symbol?: string;
  market?: string;
  direction?: string;
  thesis?: string;
  confidence?: string;
  entryRule?: string;
  exitRule?: string;
  maxPositionPct?: string;
  timeHorizon?: string;
  validUntil?: string;
  riskNotes?: string;
  approvalStatus?: string;
  adjustedPositionPct?: string;
  rawLines: string[];
}

interface FinanceArtifact {
  id: string;
  message: ChatMessage;
  parsed: ParsedTradeIntent | FinanceIntentPayload | null;
  rawLines: string[];
  bundle: FinanceArtifactPayload | null;
}

function extractTradeIntentPreview(content: string): string[] {
  return content
    .split('\n')
    .map((line) => line.trim())
    .filter((line) =>
      /tradeintent|symbol|direction|entry_rule|exit_rule|approval_status|max_position_pct/i.test(line),
    )
    .slice(0, 8);
}

function normalizeFieldKey(rawKey: string): string {
  return rawKey
    .trim()
    .toLowerCase()
    .replace(/[（）()]/g, '')
    .replace(/\s+/g, '_')
    .replace(/-/g, '_');
}

function parseTradeIntent(content: string): ParsedTradeIntent | null {
  const rawLines = extractTradeIntentPreview(content);
  if (rawLines.length === 0) return null;
  const parsed: ParsedTradeIntent = { rawLines };
  for (const line of rawLines) {
    const match = line.match(/^[-*]?\s*([A-Za-z_][A-Za-z0-9_\s-]*)\s*[:：]\s*(.+)$/);
    if (!match) continue;
    const key = normalizeFieldKey(match[1]);
    const value = match[2].trim();
    if (!value) continue;
    if (key === 'symbol') parsed.symbol = value;
    else if (key === 'market') parsed.market = value;
    else if (key === 'direction') parsed.direction = value;
    else if (key === 'thesis') parsed.thesis = value;
    else if (key === 'confidence') parsed.confidence = value;
    else if (key === 'entry_rule') parsed.entryRule = value;
    else if (key === 'exit_rule') parsed.exitRule = value;
    else if (key === 'max_position_pct') parsed.maxPositionPct = value;
    else if (key === 'time_horizon') parsed.timeHorizon = value;
    else if (key === 'valid_until') parsed.validUntil = value;
    else if (key === 'risk_notes') parsed.riskNotes = value;
    else if (key === 'approval_status') parsed.approvalStatus = value;
    else if (key === 'adjusted_position_pct') parsed.adjustedPositionPct = value;
  }
  return parsed;
}

function stepTone(kind: string): { dot: string; line: string; badge: string } {
  if (kind === 'routing') return { dot: '#0E6A4C', line: 'rgba(14,106,76,0.22)', badge: 'rgba(14,106,76,0.10)' };
  if (kind === 'mcp' || kind === 'tool_call') return { dot: '#BA7517', line: 'rgba(186,117,23,0.22)', badge: 'rgba(186,117,23,0.10)' };
  if (kind === 'skill') return { dot: '#2257C1', line: 'rgba(34,87,193,0.22)', badge: 'rgba(34,87,193,0.10)' };
  return { dot: 'var(--color-text-tertiary)', line: 'var(--color-border-tertiary)', badge: 'var(--color-background-primary)' };
}

export const FinancePreviewSidebar: React.FC<{
  finance: FinanceScenarioConfig;
  messages: ChatMessage[];
  streamingSteps: ActivityStep[];
  streamingPup: StreamingPupState | null;
}> = ({ finance, messages, streamingSteps, streamingPup }) => {
  const { lang } = useLang();
  const [view, setView] = useState<'overview' | 'runtime' | 'artifact'>('overview');
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(null);

  const financeArtifacts = useMemo<FinanceArtifact[]>(() => (
    [...messages]
      .reverse()
      .filter((message) => message.role === 'assistant')
      .map((message) => {
        const parsed = message.finance_artifact ?? parseTradeIntent(message.content);
        const rawLines = message.finance_artifact?.rawLines ?? parseTradeIntent(message.content)?.rawLines ?? extractTradeIntentPreview(message.content);
        return {
          id: message.id,
          message,
          parsed: message.finance_artifact?.intents?.[0] ?? parsed,
          rawLines,
          bundle: message.finance_artifact ?? null,
        };
      })
      .filter((artifact) => artifact.rawLines.length > 0)
      .slice(0, 5)
  ), [messages]);
  const latestAssistant = useMemo(
    () => financeArtifacts[0]?.message ?? [...messages].reverse().find((message) => message.role === 'assistant') ?? null,
    [financeArtifacts, messages],
  );
  const financeRouting = useMemo(() => {
    const route = [...streamingSteps].reverse().find((step) => step.kind === 'routing' && step.label.startsWith('finance:'));
    if (!route) return null;
    const match = route.label.match(/^finance:([^:]+):(.+)$/);
    if (!match) return null;
    return { preset: match[1], stage: match[2] };
  }, [streamingSteps]);
  const recentSteps = useMemo(() => streamingSteps.slice(-6), [streamingSteps]);
  const aliasSummary = useMemo(() => (
    (['intel', 'risk', 'exec'] as const).map((connector) => {
      const capabilities = FINANCE_CONNECTOR_CAPABILITY_KEYS[connector].map((capability) => ({
        capability,
        toolName: finance.connectorBindings[connector].capabilityBindings[capability]?.toolName ?? null,
      }));
      return {
        connector,
        target: finance.connectorBindings[connector].serverName ?? 'unbound',
        boundCount: capabilities.filter((item) => !!item.toolName).length,
        totalCount: capabilities.length,
        capabilities,
      };
    })
  ), [finance]);
  const roleSummary = useMemo(() => (
    (['researcher', 'strategist', 'risk_officer', 'executor', 'reviewer'] as const).map((role) => ({
      role,
      pup: finance.roleBindings[role].pupKey ?? 'unbound',
    }))
  ), [finance]);
  const riskSummary = useMemo(() => ([
    [lang === 'zh' ? '单票上限' : 'Single name', `${finance.riskPreset.singlePositionLimitPct}%`],
    [lang === 'zh' ? '行业上限' : 'Sector cap', `${finance.riskPreset.singleSectorLimitPct}%`],
    [lang === 'zh' ? '日亏熔断' : 'Daily stop', `${finance.riskPreset.dailyLossCircuitBreakerPct}%`],
    [lang === 'zh' ? '整手限制' : 'Lot size', `${finance.riskPreset.boardLotSize}`],
  ]), [finance, lang]);
  const riskGuards = useMemo(() => ([
    finance.riskPreset.forceLeashed ? (lang === 'zh' ? '强制 leashed' : 'Leashed enforced') : null,
    finance.riskPreset.requireManualApproval ? (lang === 'zh' ? '人工确认下单' : 'Manual confirm') : null,
    finance.riskPreset.blockStSuspendedDelisting ? (lang === 'zh' ? '禁 ST / 停牌 / 退市' : 'Block ST / suspended / delisting') : null,
    finance.riskPreset.enforceTradingWindow ? (lang === 'zh' ? '限制交易时段' : 'Trading window enforced') : null,
    finance.riskPreset.enforceT1 ? (lang === 'zh' ? '强制 T+1' : 'T+1 enforced') : null,
  ].filter(Boolean)), [finance, lang]);
  useEffect(() => {
    if (financeArtifacts.length === 0) {
      if (selectedArtifactId !== null) setSelectedArtifactId(null);
      return;
    }
    if (!selectedArtifactId || !financeArtifacts.some((artifact) => artifact.id === selectedArtifactId)) {
      setSelectedArtifactId(financeArtifacts[0].id);
    }
  }, [financeArtifacts, selectedArtifactId]);
  const selectedArtifact = useMemo(
    () => financeArtifacts.find((artifact) => artifact.id === selectedArtifactId) ?? financeArtifacts[0] ?? null,
    [financeArtifacts, selectedArtifactId],
  );
  const [selectedIntentIndex, setSelectedIntentIndex] = useState(0);
  useEffect(() => {
    setSelectedIntentIndex(0);
  }, [selectedArtifact?.id]);
  const selectedIntent = selectedArtifact?.bundle?.intents?.[selectedIntentIndex] ?? null;
  const tradeIntentLines = selectedArtifact?.rawLines ?? [];
  const parsedTradeIntent = selectedIntent ?? selectedArtifact?.parsed ?? null;
  const hasLiveRun = !!financeRouting || recentSteps.length > 0 || !!streamingPup;

  return (
    <aside style={panelStyle}>
      <div style={{ padding: '18px 16px 22px', display: 'grid', gap: 14 }}>
        <div style={{
          display: 'grid',
          gap: 10,
          padding: '16px',
          borderRadius: 18,
          background: 'linear-gradient(180deg, rgba(16,59,47,0.10), rgba(16,59,47,0.03))',
          border: '0.5px solid rgba(16,59,47,0.12)',
        }}>
          <span style={{
            width: 'fit-content',
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            padding: '4px 9px',
            borderRadius: 999,
            fontSize: 11,
            fontWeight: 700,
            color: '#0E6A4C',
            background: 'rgba(29,158,117,0.12)',
            border: '0.5px solid rgba(29,158,117,0.20)',
          }}>
            {lang === 'zh' ? 'Finance Preview' : 'Finance Preview'}
          </span>
          <div style={{ fontSize: 20, fontWeight: 760, lineHeight: 1.15, color: 'var(--color-text-primary)' }}>
            {lang === 'zh' ? '运行预览与上下文' : 'Runtime Preview & Context'}
          </div>
          <div style={{ fontSize: 12, lineHeight: 1.7, color: 'var(--color-text-secondary)' }}>
            {lang === 'zh'
              ? '这里展示当前金融场景会话的运行态、链路和最近产物摘要。配置已迁移到左下角齿轮里。'
              : 'This panel now focuses on runtime state, pipeline context, and recent artifacts. Configuration lives behind the gear button.'}
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
            <div style={{ ...cardStyle, padding: '12px 13px', background: 'rgba(255,255,255,0.48)' }}>
              <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {lang === 'zh' ? '场景状态' : 'Scenario'}
              </div>
              <div style={{ marginTop: 6, fontSize: 15, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                {hasLiveRun ? (lang === 'zh' ? '运行中' : 'Live') : (lang === 'zh' ? '待命' : 'Idle')}
              </div>
            </div>
            <div style={{ ...cardStyle, padding: '12px 13px', background: 'rgba(255,255,255,0.48)' }}>
              <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {lang === 'zh' ? '最近产物' : 'Artifact'}
              </div>
              <div style={{ marginTop: 6, fontSize: 15, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                {tradeIntentLines.length > 0 ? 'TradeIntent' : (lang === 'zh' ? '暂无' : 'None')}
              </div>
            </div>
          </div>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
            gap: 6,
            padding: 4,
            borderRadius: 14,
            background: 'rgba(16,59,47,0.06)',
          }}>
            {([
              ['overview', lang === 'zh' ? '总览' : 'Overview'],
              ['runtime', lang === 'zh' ? '运行态' : 'Runtime'],
              ['artifact', lang === 'zh' ? '产物' : 'Artifact'],
            ] as const).map(([key, label]) => {
              const active = view === key;
              return (
                <button
                  key={key}
                  onClick={() => setView(key)}
                  style={{
                    ...segmentButtonStyle,
                    background: active ? 'var(--color-background-primary)' : 'transparent',
                    color: active ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                    boxShadow: active ? '0 6px 16px rgba(0,0,0,0.06)' : 'none',
                  }}
                >
                  {label}
                </button>
              );
            })}
          </div>
        </div>

        {view === 'overview' && (
          <>
        <section style={cardStyle}>
          <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
            {lang === 'zh' ? '当前运行态' : 'Current Runtime'}
          </div>
          <div style={{ marginTop: 10, display: 'grid', gap: 8 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, fontSize: 12 }}>
              <span style={{ color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '命中预设' : 'Preset'}</span>
              <strong style={{ color: 'var(--color-text-primary)' }}>
                {financeRouting
                  ? (lang === 'zh' ? (skillLabels[financeRouting.preset]?.zh ?? financeRouting.preset) : (skillLabels[financeRouting.preset]?.en ?? financeRouting.preset))
                  : (lang === 'zh' ? '未触发' : 'Idle')}
              </strong>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, fontSize: 12 }}>
              <span style={{ color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '当前阶段' : 'Stage'}</span>
              <strong style={{ color: 'var(--color-text-primary)', textAlign: 'right' }}>
                {financeRouting?.stage ?? streamingPup?.name ?? (lang === 'zh' ? '等待中' : 'Waiting')}
              </strong>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, fontSize: 12 }}>
              <span style={{ color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '人工确认' : 'Manual confirm'}</span>
              <strong style={{ color: finance.riskPreset.requireManualApproval ? '#0E6A4C' : 'var(--color-text-secondary)' }}>
                {finance.riskPreset.requireManualApproval ? (lang === 'zh' ? '必需' : 'Required') : (lang === 'zh' ? '可选' : 'Optional')}
              </strong>
            </div>
          </div>
        </section>

        <section style={cardStyle}>
          <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
            {lang === 'zh' ? '生效中的风控' : 'Active Risk Preset'}
          </div>
          <div style={{ marginTop: 10, display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 8 }}>
            {riskSummary.map(([label, value]) => (
              <div key={label} style={{
                borderRadius: 12,
                padding: '10px 11px',
                background: 'var(--color-background-primary)',
                border: '0.5px solid var(--color-border-tertiary)',
              }}>
                <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>{label}</div>
                <div style={{ marginTop: 5, fontSize: 14, fontWeight: 700, color: 'var(--color-text-primary)' }}>{value}</div>
              </div>
            ))}
          </div>
          {riskGuards.length > 0 && (
            <div style={{ marginTop: 10, display: 'flex', flexWrap: 'wrap', gap: 6 }}>
              {riskGuards.map((item) => (
                <span key={item} style={{
                  padding: '4px 8px',
                  borderRadius: 999,
                  fontSize: 10,
                  color: '#0E6A4C',
                  background: 'rgba(16,59,47,0.08)',
                  border: '0.5px solid rgba(16,59,47,0.14)',
                }}>
                  {item}
                </span>
              ))}
            </div>
          )}
        </section>

        <section style={cardStyle}>
          <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
            {lang === 'zh' ? 'Alias 路由' : 'Alias Routing'}
          </div>
          <div style={{ marginTop: 10, display: 'grid', gap: 8 }}>
            {aliasSummary.map(({ connector, target, boundCount, totalCount, capabilities }) => (
              <div key={connector} style={{
                display: 'grid',
                gap: 8,
                padding: '10px 11px',
                borderRadius: 12,
                background: 'var(--color-background-primary)',
                border: '0.5px solid var(--color-border-tertiary)',
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, fontSize: 12 }}>
                  <span style={{ color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>{connector}</span>
                  <span style={{ color: 'var(--color-text-primary)' }}>{target}</span>
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, fontSize: 11 }}>
                  <span style={{ color: 'var(--color-text-secondary)' }}>{lang === 'zh' ? '能力覆盖' : 'Coverage'}</span>
                  <strong style={{ color: boundCount === totalCount ? '#0E6A4C' : 'var(--color-text-primary)' }}>{boundCount}/{totalCount}</strong>
                </div>
                <div style={{ display: 'grid', gap: 6 }}>
                  {capabilities.map(({ capability, toolName }) => (
                    <div key={capability} style={{ display: 'flex', justifyContent: 'space-between', gap: 8, fontSize: 10 }}>
                      <div style={{ display: 'grid', gap: 2 }}>
                        <span style={{ color: 'var(--color-text-primary)' }}>
                          {lang === 'zh' ? capabilityLabels[capability].zh : capabilityLabels[capability].en}
                        </span>
                        <span style={{ color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>{`mcp__${connector}__${capability}`}</span>
                      </div>
                      <span style={{ color: toolName ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)' }}>
                        {toolName ?? (lang === 'zh' ? '未绑定' : 'Unbound')}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </section>

        <section style={cardStyle}>
          <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
            {lang === 'zh' ? '专家绑定快照' : 'Expert Snapshot'}
          </div>
          <div style={{ marginTop: 10, display: 'grid', gap: 8 }}>
            {roleSummary.map(({ role, pup }) => (
              <div key={role} style={{ display: 'flex', justifyContent: 'space-between', gap: 8, fontSize: 12 }}>
                <span style={{ color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>{role}</span>
                <span style={{ color: 'var(--color-text-primary)' }}>{pup}</span>
              </div>
            ))}
          </div>
        </section>
          </>
        )}

        {(view === 'overview' || view === 'runtime') && (
          <section style={cardStyle}>
            <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
              {lang === 'zh' ? '最近活动' : 'Recent Activity'}
            </div>
            <div style={{ marginTop: 12, display: 'grid', gap: 10 }}>
              {recentSteps.length > 0 ? recentSteps.map((step, index) => {
                const tone = stepTone(step.kind);
                const isLast = index === recentSteps.length - 1;
                return (
                  <div key={`${step.kind}-${index}`} style={{ display: 'grid', gridTemplateColumns: '16px 1fr', gap: 10 }}>
                    <div style={{ display: 'grid', justifyItems: 'center' }}>
                      <span style={{
                        width: 10,
                        height: 10,
                        borderRadius: 999,
                        background: tone.dot,
                        marginTop: 4,
                        boxShadow: `0 0 0 3px ${tone.badge}`,
                      }} />
                      {!isLast && <span style={{ width: 1.5, flex: 1, minHeight: 18, background: tone.line, marginTop: 4 }} />}
                    </div>
                    <div style={{
                      borderRadius: 12,
                      padding: '9px 10px',
                      background: tone.badge,
                      border: '0.5px solid var(--color-border-tertiary)',
                    }}>
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                        <strong style={{ color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)', fontSize: 10 }}>{step.kind}</strong>
                        <span style={{ fontSize: 10, color: 'var(--color-text-tertiary)' }}>{index + 1}</span>
                      </div>
                      <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                        {step.label}
                      </div>
                    </div>
                  </div>
                );
              }) : (
                <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                  {lang === 'zh' ? '等待第一条运行活动。' : 'Waiting for the first runtime activity.'}
                </div>
              )}
            </div>
          </section>
        )}

        {(view === 'overview' || view === 'artifact') && (
          <section style={cardStyle}>
            <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
              {lang === 'zh' ? 'TradeIntent / 产物预览' : 'TradeIntent / Artifact Preview'}
            </div>
            <div style={{ marginTop: 10, display: 'grid', gap: 10 }}>
              {financeArtifacts.length > 1 && (
                <div style={{
                  display: 'grid',
                  gap: 8,
                  padding: '10px 11px',
                  borderRadius: 12,
                  background: 'var(--color-background-primary)',
                  border: '0.5px solid var(--color-border-tertiary)',
                }}>
                  <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                    {lang === 'zh' ? '最近制品栈' : 'Recent Artifact Stack'}
                  </div>
                  <div style={{ display: 'grid', gap: 6 }}>
                    {financeArtifacts.map((artifact, index) => {
                      const active = artifact.id === selectedArtifact?.id;
                      const primaryIntent = artifact.bundle?.intents?.[0] ?? artifact.parsed;
                      const heading = primaryIntent?.symbol
                        ?? primaryIntent?.market
                        ?? artifact.message.pup_name
                        ?? `Artifact ${index + 1}`;
                      const subline = [primaryIntent?.direction, primaryIntent?.approvalStatus].filter(Boolean).join(' · ')
                        || artifact.rawLines[0]
                        || (lang === 'zh' ? '未提取到字段' : 'No extracted fields');
                      return (
                        <button
                          key={artifact.id}
                          onClick={() => setSelectedArtifactId(artifact.id)}
                          style={{
                            textAlign: 'left',
                            border: 'none',
                            cursor: 'pointer',
                            borderRadius: 10,
                            padding: '9px 10px',
                            background: active ? 'rgba(16,59,47,0.08)' : 'var(--color-background-secondary)',
                            color: 'var(--color-text-primary)',
                          }}
                        >
                          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                            <strong style={{ fontSize: 11, color: 'var(--color-text-primary)' }}>{heading}</strong>
                            <span style={{ fontSize: 10, color: 'var(--color-text-tertiary)' }}>#{index + 1}</span>
                          </div>
                          <div style={{ marginTop: 4, fontSize: 10, color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>
                            {subline}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
              {parsedTradeIntent ? (
                <>
                  {selectedArtifact?.bundle && (
                    <div style={{
                      display: 'grid',
                      gap: 8,
                      padding: '10px 11px',
                      borderRadius: 12,
                      background: 'var(--color-background-primary)',
                      border: '0.5px solid var(--color-border-tertiary)',
                    }}>
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
                        {selectedArtifact.bundle.scenarioPreset && (
                          <span style={{
                            padding: '4px 8px',
                            borderRadius: 999,
                            fontSize: 10,
                            fontWeight: 700,
                            color: '#0E6A4C',
                            background: 'rgba(29,158,117,0.12)',
                            border: '0.5px solid rgba(29,158,117,0.20)',
                          }}>
                            {selectedArtifact.bundle.scenarioPreset}
                          </span>
                        )}
                        {selectedArtifact.bundle.sourcePupName && (
                          <span style={{ fontSize: 11, color: 'var(--color-text-secondary)' }}>
                            {lang === 'zh' ? '来源：' : 'Source: '}
                            {selectedArtifact.bundle.sourcePupName}
                          </span>
                        )}
                        <span style={{ fontSize: 11, color: 'var(--color-text-secondary)' }}>
                          {lang === 'zh' ? '意图数：' : 'Intents: '}
                          {selectedArtifact.bundle.intents.length}
                        </span>
                      </div>
                      {selectedArtifact.bundle.stageLabels.length > 0 && (
                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                          {selectedArtifact.bundle.stageLabels.map((label) => (
                            <span key={label} style={{
                              padding: '3px 7px',
                              borderRadius: 999,
                              fontSize: 10,
                              color: 'var(--color-text-secondary)',
                              background: 'var(--color-background-secondary)',
                              border: '0.5px solid var(--color-border-tertiary)',
                            }}>
                              {label}
                            </span>
                          ))}
                        </div>
                      )}
                      {selectedArtifact.bundle.intents.length > 1 && (
                        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 6 }}>
                          {selectedArtifact.bundle.intents.map((intent, index) => {
                            const active = index === selectedIntentIndex;
                            return (
                              <button
                                key={`${selectedArtifact.id}-intent-${index}`}
                                onClick={() => setSelectedIntentIndex(index)}
                                style={{
                                  border: 'none',
                                  cursor: 'pointer',
                                  borderRadius: 10,
                                  padding: '8px 9px',
                                  textAlign: 'left',
                                  background: active ? 'rgba(16,59,47,0.08)' : 'var(--color-background-secondary)',
                                  color: 'var(--color-text-primary)',
                                }}
                              >
                                <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)' }}>
                                  {lang === 'zh' ? `意图 ${index + 1}` : `Intent ${index + 1}`}
                                </div>
                                <div style={{ marginTop: 3, fontSize: 11, fontWeight: 700 }}>
                                  {intent.symbol ?? intent.market ?? '—'}
                                </div>
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}
                  <div style={{
                    borderRadius: 14,
                    padding: '12px 13px',
                    background: 'linear-gradient(180deg, rgba(16,59,47,0.08), rgba(16,59,47,0.03))',
                    border: '0.5px solid rgba(16,59,47,0.14)',
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10 }}>
                      <div>
                        <div style={{ fontSize: 16, fontWeight: 760, color: 'var(--color-text-primary)' }}>
                          {parsedTradeIntent.symbol ?? (lang === 'zh' ? '未识别标的' : 'Unknown symbol')}
                        </div>
                        <div style={{ marginTop: 3, fontSize: 11, color: 'var(--color-text-secondary)' }}>
                          {[parsedTradeIntent.market, parsedTradeIntent.direction].filter(Boolean).join(' · ') || (lang === 'zh' ? '等待结构化字段' : 'Waiting for structured fields')}
                        </div>
                      </div>
                      {parsedTradeIntent.approvalStatus && (
                        <span style={{
                          padding: '4px 8px',
                          borderRadius: 999,
                          fontSize: 10,
                          fontWeight: 700,
                          color: '#0E6A4C',
                          background: 'rgba(29,158,117,0.12)',
                          border: '0.5px solid rgba(29,158,117,0.20)',
                          textTransform: 'uppercase',
                        }}>
                          {parsedTradeIntent.approvalStatus}
                        </span>
                      )}
                    </div>
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
                    {[
                      [lang === 'zh' ? '仓位上限' : 'Max position', parsedTradeIntent.maxPositionPct],
                      [lang === 'zh' ? '置信度' : 'Confidence', parsedTradeIntent.confidence],
                      [lang === 'zh' ? '时效' : 'Horizon', parsedTradeIntent.timeHorizon],
                      [lang === 'zh' ? '有效期' : 'Valid until', parsedTradeIntent.validUntil],
                    ].map(([label, value]) => (
                      <div key={String(label)} style={{
                        borderRadius: 12,
                        padding: '10px 11px',
                        background: 'var(--color-background-primary)',
                        border: '0.5px solid var(--color-border-tertiary)',
                      }}>
                        <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                          {label}
                        </div>
                        <div style={{ marginTop: 5, fontSize: 12, color: 'var(--color-text-primary)', lineHeight: 1.55 }}>
                          {value || '—'}
                        </div>
                      </div>
                    ))}
                  </div>
                  {[
                    [lang === 'zh' ? '入场规则' : 'Entry rule', parsedTradeIntent.entryRule],
                    [lang === 'zh' ? '出场规则' : 'Exit rule', parsedTradeIntent.exitRule],
                    [lang === 'zh' ? '交易理由' : 'Thesis', parsedTradeIntent.thesis],
                    [lang === 'zh' ? '风险备注' : 'Risk notes', parsedTradeIntent.riskNotes],
                  ].filter(([, value]) => !!value).map(([label, value]) => (
                    <div key={String(label)} style={{
                      borderRadius: 12,
                      padding: '11px 12px',
                      background: 'var(--color-background-primary)',
                      border: '0.5px solid var(--color-border-tertiary)',
                    }}>
                      <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                        {label}
                      </div>
                      <div style={{ marginTop: 6, fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.65 }}>
                        {value}
                      </div>
                    </div>
                  ))}
                  <div style={{
                    borderRadius: 12,
                    padding: '11px 12px',
                    background: 'var(--color-background-primary)',
                    border: '0.5px solid var(--color-border-tertiary)',
                  }}>
                    <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                      {lang === 'zh' ? '原始片段' : 'Raw excerpt'}
                    </div>
                    <div style={{ marginTop: 6, display: 'grid', gap: 6 }}>
                      {parsedTradeIntent.rawLines.map((line) => (
                        <div key={line} style={{ fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.55, fontFamily: 'var(--font-mono)' }}>
                          {line}
                        </div>
                      ))}
                    </div>
                  </div>
                </>
              ) : tradeIntentLines.length > 0 ? tradeIntentLines.map((line) => (
                <div key={line} style={{ fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.55, fontFamily: 'var(--font-mono)' }}>
                  {line}
                </div>
              )) : (
                <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                  {lang === 'zh'
                    ? '最近还没有可预览的 TradeIntent 片段。等场景开始运行后，这里会显示结构化摘要。'
                    : 'No TradeIntent preview yet. Once the scenario runs, a structured summary will appear here.'}
                </div>
              )}
            </div>
          </section>
        )}
      </div>
    </aside>
  );
};
