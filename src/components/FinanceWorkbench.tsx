import React, { useEffect, useMemo, useState } from 'react';
import { FinanceOverview } from './FinanceOverview';
import { FinanceResearch } from './FinanceResearch';
import { FinanceOrders } from './FinanceOrders';
import { FinancePipeline } from './FinancePipeline';
import { useFinanceStore, type FinanceTab } from '../stores/financeStore';

const shellCard: React.CSSProperties = {
  borderRadius: 20,
  border: '1px solid var(--color-border-tertiary)',
  background: 'linear-gradient(180deg, rgba(255,255,255,0.88), rgba(255,255,255,0.98))',
  boxShadow: '0 22px 60px rgba(15, 15, 20, 0.06)',
  backdropFilter: 'blur(18px)',
};

const toneCard = (accent: string, wash: string): React.CSSProperties => ({
  borderRadius: 18,
  padding: '14px 16px',
  border: `1px solid ${wash}`,
  background: `linear-gradient(180deg, ${wash}, rgba(255,255,255,0.88))`,
  boxShadow: `inset 0 1px 0 rgba(255,255,255,0.55), 0 14px 28px ${wash}`,
  color: accent,
});

const tabButton = (active: boolean): React.CSSProperties => ({
  border: '1px solid',
  borderColor: active ? 'rgba(16,59,47,0.22)' : 'transparent',
  borderRadius: 999,
  padding: '8px 14px',
  fontSize: 12,
  fontWeight: active ? 700 : 500,
  cursor: 'pointer',
  background: active ? 'linear-gradient(180deg, rgba(16,59,47,0.96), rgba(16,59,47,0.88))' : 'transparent',
  color: active ? '#E5F7F0' : 'var(--color-text-secondary)',
  boxShadow: active ? '0 10px 24px rgba(16,59,47,0.18)' : 'none',
});

const formatMoney = (value?: number | null) => {
  if (value == null || Number.isNaN(value)) return '--';
  if (Math.abs(value) >= 100000000) return `${(value / 100000000).toFixed(2)}亿`;
  if (Math.abs(value) >= 10000) return `${(value / 10000).toFixed(1)}万`;
  return new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 0 }).format(value);
};

const serviceTone = (status?: string) => {
  if (status === 'up') return { label: '在线', color: '#0E6A4C', wash: 'rgba(29,158,117,0.12)' };
  if (status === 'unconfigured') return { label: '未配置', color: '#8A5A10', wash: 'rgba(186,117,23,0.12)' };
  return { label: '异常', color: '#B81C1C', wash: 'rgba(226,75,74,0.12)' };
};

const microPill = (tone: string): React.CSSProperties => ({
  borderRadius: 999,
  padding: '4px 8px',
  fontSize: 11,
  fontWeight: 700,
  background: tone,
});

export const FinanceWorkbench: React.FC = () => {
  const [screenQuery, setScreenQuery] = useState('市盈率小于20，非ST，成交额居前');
  const {
    activeTab,
    setActiveTab,
    overview,
    watchlist,
    ordersSnapshot,
    intents,
    error,
    loading,
    loadOverview,
    loadOrders,
    loadWatchlist,
    runScreenStocks,
  } = useFinanceStore();

  useEffect(() => {
    void loadOverview();
  }, []);

  useEffect(() => {
    if (activeTab === 'orders' && !ordersSnapshot && !loading.orders) {
      void loadOrders();
      return;
    }
    if (activeTab === 'research' && watchlist.length === 0 && !loading.research) {
      void loadWatchlist();
    }
  }, [activeTab, loadOrders, loadWatchlist, loading.orders, loading.research, ordersSnapshot, watchlist.length]);

  const tabs: Array<{ key: FinanceTab; label: string; hint: string }> = [
    { key: 'overview', label: 'Overview', hint: '总资产、风控位与运行状态' },
    { key: 'research', label: 'Research', hint: '观察池、新闻和结构化数据' },
    { key: 'orders', label: 'Orders', hint: '持仓、委托、成交与盈亏' },
    { key: 'pipeline', label: 'Pipeline', hint: 'TradeIntent 风控审批入口' },
  ];

  const activeTabHint = useMemo(
    () => tabs.find((tab) => tab.key === activeTab)?.hint ?? '',
    [activeTab],
  );

  const marketLabel = overview?.market_status.is_open
    ? '盘中联动'
    : overview?.market_status.is_trading_day
      ? '交易日非连续竞价'
      : '休市观察模式';

  const watchlistHighlights = (overview?.watchlist ?? []).slice(0, 3);
  const positionHighlights = [...(overview?.positions ?? [])]
    .sort((a, b) => Math.abs(b.pnl_pct) - Math.abs(a.pnl_pct))
    .slice(0, 3);
  const pipelineSummary = useMemo(() => {
    return {
      total: intents.length,
      pending: intents.filter((item) => (item.approval_status ?? 'pending') === 'pending').length,
      approved: intents.filter((item) => item.approval_status === 'approved').length,
      reduced: intents.filter((item) => item.approval_status === 'reduced').length,
      rejected: intents.filter((item) => item.approval_status === 'rejected').length,
    };
  }, [intents]);

  const renderFocusPanel = () => {
  const workflowHint = activeTab === 'research'
      ? '筛选标的、查看资讯和数据，再决定是否生成交易意图。'
      : activeTab === 'orders'
        ? '查看持仓、委托和成交，确认账户当前执行状态。'
        : activeTab === 'pipeline'
          ? '处理待审批意图，重点关注 rejected 和 reduced。'
          : '查看资金、交易时段和待处理信号。';

    if (activeTab === 'research') {
      return (
        <>
          <div style={{ display: 'grid', gap: 10 }}>
            <input
              value={screenQuery}
              onChange={(e) => setScreenQuery(e.target.value)}
              placeholder="输入一个选股条件"
              style={{ width: '100%', boxSizing: 'border-box', borderRadius: 12, border: '1px solid rgba(16,59,47,0.10)', background: 'rgba(255,255,255,0.88)', color: 'var(--color-text-primary)', fontSize: 12, padding: '10px 12px' }}
            />
            <button
              onClick={() => void runScreenStocks(screenQuery, true)}
              style={{ borderRadius: 12, border: 'none', background: 'linear-gradient(180deg, #BA7517, #9B6210)', color: '#FFF7EB', fontSize: 12, padding: '10px 12px', cursor: 'pointer', boxShadow: '0 10px 22px rgba(186,117,23,0.18)' }}
            >
              运行筛选
            </button>
          </div>
              <div style={{ display: 'grid', gap: 8 }}>
                <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(16,59,47,0.06)', border: '1px solid rgba(16,59,47,0.08)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>操作建议</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                从观察池或筛选结果中选择标的，继续查看新闻、数据和走势。
              </div>
            </div>
          </div>
        </>
      );
    }

    if (activeTab === 'orders') {
      return (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 8 }}>
            <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(55,138,221,0.08)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#1A5EA0' }}>持仓</div>
              <div style={{ marginTop: 8, fontSize: 18, fontWeight: 760 }}>{overview?.positions.length ?? '--'}</div>
            </div>
            <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(186,117,23,0.08)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#8A5A10' }}>委托</div>
              <div style={{ marginTop: 8, fontSize: 18, fontWeight: 760 }}>{overview?.active_order_count ?? '--'}</div>
            </div>
            <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(29,158,117,0.08)' }}>
              <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>成交</div>
              <div style={{ marginTop: 8, fontSize: 18, fontWeight: 760 }}>{overview?.today_trade_count ?? '--'}</div>
            </div>
          </div>
          <button
            onClick={() => void loadOrders(true)}
            style={{ borderRadius: 12, border: '1px solid rgba(16,59,47,0.12)', background: 'rgba(255,255,255,0.90)', color: 'var(--color-text-secondary)', fontSize: 12, padding: '9px 12px', cursor: 'pointer' }}
          >
            刷新订单快照
          </button>
        </>
      );
    }

    if (activeTab === 'pipeline') {
      return (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 8 }}>
            {[
              ['待审', pipelineSummary.pending, '#1A5EA0', 'rgba(55,138,221,0.08)'],
              ['通过', pipelineSummary.approved, '#0E6A4C', 'rgba(29,158,117,0.08)'],
              ['降仓', pipelineSummary.reduced, '#8A5A10', 'rgba(186,117,23,0.08)'],
              ['拒绝', pipelineSummary.rejected, '#B81C1C', 'rgba(226,75,74,0.08)'],
            ].map(([label, value, color, bg]) => (
              <div key={String(label)} style={{ borderRadius: 12, padding: '10px 12px', background: String(bg) }}>
                <div style={{ fontSize: 11, fontWeight: 700, color: String(color) }}>{label}</div>
                <div style={{ marginTop: 8, fontSize: 18, fontWeight: 760 }}>{value}</div>
              </div>
            ))}
          </div>
          <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(16,59,47,0.06)', border: '1px solid rgba(16,59,47,0.08)' }}>
            <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>审批提示</div>
            <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
              {workflowHint}
            </div>
          </div>
        </>
      );
    }

    return (
      <>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 8 }}>
          {([
            ['intel', overview?.health.intel.status],
            ['risk', overview?.health.risk.status],
            ['exec', overview?.health.exec.status],
          ] as const).map(([label, status]) => {
            const tone = serviceTone(status);
            return (
              <div key={label} style={{ borderRadius: 12, padding: '10px 12px', background: tone.wash, border: `1px solid ${tone.wash}`, minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                  <span style={{ fontSize: 11, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.06em' }}>{label}</span>
                  <span style={{ width: 7, height: 7, borderRadius: '50%', background: tone.color, flexShrink: 0 }} />
                </div>
                <div style={{ marginTop: 8, fontSize: 15, fontWeight: 760, color: tone.color }}>
                  {tone.label}
                </div>
              </div>
            );
          })}
        </div>
        <div style={{ borderRadius: 12, padding: '10px 12px', background: 'rgba(16,59,47,0.06)', border: '1px solid rgba(16,59,47,0.08)' }}>
          <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>工作流提示</div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
            {workflowHint}
          </div>
        </div>
        <button
          onClick={() => {
            void loadOverview(true);
          }}
          style={{ borderRadius: 12, border: '1px solid rgba(16,59,47,0.12)', background: 'rgba(255,255,255,0.90)', color: 'var(--color-text-secondary)', fontSize: 12, padding: '9px 12px', cursor: 'pointer', whiteSpace: 'nowrap' }}
        >
          强制刷新
        </button>
      </>
    );
  };

  return (
    <div
      className="flex-1 overflow-auto"
      style={{
        padding: '24px',
        background: 'radial-gradient(circle at top left, rgba(16,59,47,0.10), transparent 32%), radial-gradient(circle at 85% 0%, rgba(186,117,23,0.10), transparent 24%), linear-gradient(180deg, #F5F4EF 0%, var(--color-background-secondary) 44%, var(--color-background-primary) 100%)',
      }}
    >
      <div style={{ maxWidth: 1320, margin: '0 auto', display: 'grid', gap: 18 }}>
        <section style={{ ...shellCard, padding: '22px 22px 20px', overflow: 'hidden', position: 'relative' }}>
          <div
            style={{
              position: 'absolute',
              inset: 0,
              background: 'linear-gradient(125deg, rgba(16,59,47,0.10), transparent 38%, rgba(186,117,23,0.10) 74%, transparent)',
              pointerEvents: 'none',
            }}
          />

          <div style={{ position: 'relative', display: 'grid', gap: 18 }}>
            <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1.2fr) minmax(320px, 0.88fr)', gap: 18, alignItems: 'start' }}>
              <div style={{ display: 'grid', gap: 16 }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                  <span style={{ width: 'fit-content', fontSize: 11, fontWeight: 750, color: '#0E6A4C', background: 'rgba(29,158,117,0.12)', borderRadius: 999, padding: '4px 8px' }}>
                    Finance Workbench
                  </span>
                  <span style={{ width: 'fit-content', fontSize: 11, fontWeight: 700, color: '#8A5A10', background: 'rgba(186,117,23,0.10)', borderRadius: 999, padding: '4px 8px' }}>
                    {marketLabel}
                  </span>
                  {overview?.market_status.current_session && (
                    <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                      {overview.market_status.current_session}
                    </span>
                  )}
                </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ ...microPill('rgba(55,138,221,0.10)'), color: '#1A5EA0' }}>
                      观察池 {overview?.watchlist.length ?? '--'}
                    </span>
                    <span style={{ ...microPill('rgba(29,158,117,0.10)'), color: '#0E6A4C' }}>
                      持仓 {overview?.positions.length ?? '--'}
                    </span>
                    <span style={{ ...microPill('rgba(186,117,23,0.10)'), color: '#8A5A10' }}>
                      待审 {pipelineSummary.pending}
                    </span>
                  </div>
                </div>

                <div>
                  <h1 style={{ margin: 0, fontSize: 30, lineHeight: 1.05, fontWeight: 790, letterSpacing: '-0.025em', maxWidth: 720 }}>
                  Finance Workspace
                  </h1>
                  <p style={{ margin: '10px 0 0', fontSize: 13, color: 'var(--color-text-secondary)', maxWidth: 700, lineHeight: 1.7 }}>
                    在一个工作台里查看研究、自选、风控和执行状态。
                  </p>
                </div>

                <div style={{ display: 'grid', gridTemplateColumns: '1.15fr 1fr 1fr 1fr', gap: 12 }}>
                  <div style={toneCard('#0E6A4C', 'rgba(29,158,117,0.12)')}>
                    <div style={{ fontSize: 11, fontWeight: 700, opacity: 0.82 }}>总资产</div>
                    <div style={{ marginTop: 8, fontSize: 28, fontWeight: 790 }}>¥ {formatMoney(overview?.balance?.total_assets)}</div>
                    <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>可用 {formatMoney(overview?.balance?.available_balance)}</div>
                  </div>
                  <div style={toneCard('#1A5EA0', 'rgba(55,138,221,0.12)')}>
                    <div style={{ fontSize: 11, fontWeight: 700, opacity: 0.82 }}>观察池</div>
                    <div style={{ marginTop: 8, fontSize: 26, fontWeight: 780 }}>{overview?.watchlist.length ?? '--'}</div>
                    <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>持仓 {overview?.positions.length ?? '--'} 只</div>
                  </div>
                  <div style={toneCard('#8A5A10', 'rgba(186,117,23,0.12)')}>
                    <div style={{ fontSize: 11, fontWeight: 700, opacity: 0.82 }}>活动委托</div>
                    <div style={{ marginTop: 8, fontSize: 26, fontWeight: 780 }}>{overview?.active_order_count ?? '--'}</div>
                    <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>今日成交 {overview?.today_trade_count ?? '--'} 笔</div>
                  </div>
                  <div style={toneCard('#B81C1C', 'rgba(226,75,74,0.12)')}>
                    <div style={{ fontSize: 11, fontWeight: 700, opacity: 0.82 }}>当前节奏</div>
                    <div style={{ marginTop: 8, fontSize: 20, fontWeight: 760 }}>{marketLabel}</div>
                    <div style={{ marginTop: 8, fontSize: 12, color: 'var(--color-text-secondary)' }}>
                      {overview?.market_status.is_open ? '当前处于交易时段' : '当前适合研究与准备'}
                    </div>
                  </div>
                </div>

                <div style={{ borderRadius: 16, padding: '14px 16px', background: 'rgba(16,59,47,0.05)', border: '1px solid rgba(16,59,47,0.08)', display: 'grid', gap: 10 }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                    <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>今日关注</div>
                    <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>重点提醒</div>
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 10 }}>
                    <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {watchlistHighlights[0]
                        ? `${watchlistHighlights[0].code} ${watchlistHighlights[0].name} ${watchlistHighlights[0].change_pct ?? '--'}%`
                        : '暂无观察池提醒'}
                    </div>
                    <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {positionHighlights[0]
                        ? `${positionHighlights[0].symbol} 波动 ${positionHighlights[0].pnl_pct.toFixed(2)}%`
                        : '暂无持仓波动提醒'}
                    </div>
                    <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {pipelineSummary.pending > 0
                        ? `${pipelineSummary.pending} 条意图待审批`
                        : '暂无待审批意图'}
                    </div>
                  </div>
                </div>
              </div>

              <div style={{ ...shellCard, padding: '16px 16px 14px', display: 'grid', gap: 14, alignContent: 'start', background: 'linear-gradient(180deg, rgba(248,249,246,0.96), rgba(255,255,255,0.98))' }}>
                <div>
                  <div>
                    <div style={{ fontSize: 11, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.08em', color: 'var(--color-text-tertiary)' }}>
                      Workspace Focus
                    </div>
                    <div style={{ marginTop: 6, fontSize: 22, fontWeight: 760, color: 'var(--color-text-primary)' }}>
                      {tabs.find((tab) => tab.key === activeTab)?.label}
                    </div>
                    <div style={{ marginTop: 4, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {activeTabHint}
                    </div>
                  </div>
                </div>

                <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {tabs.map((tab) => (
                    <button key={tab.key} onClick={() => setActiveTab(tab.key)} style={tabButton(activeTab === tab.key)}>
                      {tab.label}
                    </button>
                  ))}
                </div>

                {renderFocusPanel()}
              </div>
            </div>
          </div>
        </section>

        {error && (
          <div style={{ ...shellCard, padding: '12px 16px', color: 'var(--color-text-danger)', background: 'linear-gradient(180deg, rgba(226,75,74,0.10), rgba(255,255,255,0.95))' }}>
            {error}
          </div>
        )}

        <section style={{ ...shellCard, padding: '14px 18px' }}>
          <div style={{ display: 'flex', gap: 18, flexWrap: 'wrap', alignItems: 'center' }}>
            <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
              交易日: <strong style={{ color: 'var(--color-text-primary)' }}>{overview?.market_status.is_trading_day ? '是' : '否'}</strong>
            </span>
            <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
              当前时段: <strong style={{ color: 'var(--color-text-primary)' }}>{overview?.market_status.current_session ?? '--'}</strong>
            </span>
            <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
              开市状态: <strong style={{ color: overview?.market_status.is_open ? 'var(--color-text-success)' : 'var(--color-text-warning)' }}>{overview?.market_status.is_open ? '交易中' : '未开市'}</strong>
            </span>
            <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
              缓存命中策略: <strong style={{ color: 'var(--color-text-primary)' }}>前端即时返回，过期后再拉取</strong>
            </span>
            <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
              最近检查: <strong style={{ color: 'var(--color-text-primary)' }}>{overview?.health.checked_at ?? '--'}</strong>
            </span>
          </div>
        </section>

        <section style={{ minHeight: 620 }}>
          {activeTab === 'overview' && <FinanceOverview />}
          {activeTab === 'research' && <FinanceResearch />}
          {activeTab === 'orders' && <FinanceOrders />}
          {activeTab === 'pipeline' && <FinancePipeline />}
        </section>

        {(loading.overview || loading.research || loading.orders || loading.pipeline) && (
          <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', textAlign: 'center', paddingBottom: 6 }}>
            正在同步 finance 数据…
          </div>
        )}
      </div>
    </div>
  );
};
