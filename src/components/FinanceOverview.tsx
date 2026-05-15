import React from 'react';
import { useFinanceStore } from '../stores/financeStore';

const sectionCard: React.CSSProperties = {
  borderRadius: 16,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  padding: '16px 18px',
};

const metricCard = (tone: 'green' | 'amber' | 'blue' | 'red'): React.CSSProperties => {
  const palette = {
    green: ['rgba(29,158,117,0.12)', '#0E6A4C'],
    amber: ['rgba(186,117,23,0.12)', '#8A5A10'],
    blue: ['rgba(55,138,221,0.12)', '#1A5EA0'],
    red: ['rgba(226,75,74,0.12)', '#B81C1C'],
  } as const;
  return {
    borderRadius: 14,
    background: palette[tone][0],
    color: palette[tone][1],
    padding: '16px 18px',
    minHeight: 110,
  };
};

const formatMoney = (value?: number | null) => {
  if (value == null || Number.isNaN(value)) return '--';
  return new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 }).format(value);
};

export const FinanceOverview: React.FC = () => {
  const { overview } = useFinanceStore();

  if (!overview) {
    return <div style={sectionCard}>正在加载 finance 总览…</div>;
  }

  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))', gap: 14 }}>
        <div style={metricCard('green')}>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.9 }}>总资产</div>
          <div style={{ marginTop: 10, fontSize: 28, fontWeight: 760 }}>¥ {formatMoney(overview.balance?.total_assets)}</div>
          <div style={{ marginTop: 8, fontSize: 12 }}>可用资金 ¥ {formatMoney(overview.balance?.available_balance)}</div>
        </div>
        <div style={metricCard('amber')}>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.9 }}>累计盈亏</div>
          <div style={{ marginTop: 10, fontSize: 28, fontWeight: 760 }}>¥ {formatMoney(overview.balance?.total_pnl)}</div>
          <div style={{ marginTop: 8, fontSize: 12 }}>收益率 {overview.balance?.total_pnl_pct?.toFixed?.(2) ?? '--'}%</div>
        </div>
        <div style={metricCard('blue')}>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.9 }}>活动委托</div>
          <div style={{ marginTop: 10, fontSize: 28, fontWeight: 760 }}>{overview.active_order_count}</div>
          <div style={{ marginTop: 8, fontSize: 12 }}>今日成交 {overview.today_trade_count} 笔</div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr', gap: 16 }}>
        <div style={sectionCard}>
          <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 12 }}>账户与风险提醒</div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 10 }}>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'var(--color-background-secondary)' }}>
              <div style={{ fontSize: 12, fontWeight: 700 }}>时间戳</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>{overview.market_status.server_time ?? overview.health.checked_at ?? '--'}</div>
            </div>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'var(--color-background-secondary)' }}>
              <div style={{ fontSize: 12, fontWeight: 700 }}>资金冻结</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>¥ {formatMoney(overview.balance?.frozen)}</div>
            </div>
            <div style={{ borderRadius: 12, padding: '12px 14px', background: 'var(--color-background-secondary)' }}>
              <div style={{ fontSize: 12, fontWeight: 700 }}>观察池规模</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>{overview.watchlist.length} 只自选，{overview.positions.length} 只持仓</div>
            </div>
          </div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
        <div style={sectionCard}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
            <div style={{ fontSize: 14, fontWeight: 700 }}>自选股摘要</div>
            <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>Top 5</span>
          </div>
          <div style={{ display: 'grid', gap: 8 }}>
            {overview.watchlist.slice(0, 5).map((item) => (
              <div key={item.code} style={{ display: 'grid', gridTemplateColumns: '90px 1fr 90px', gap: 10, alignItems: 'center', borderRadius: 12, padding: '10px 12px', background: 'var(--color-background-secondary)' }}>
                <div>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>{item.code}</div>
                  <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{item.name}</div>
                </div>
                <div style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>PE {item.pe ?? '--'} / 市值 {item.market_cap ? `${(item.market_cap / 100000000).toFixed(1)}亿` : '--'}</div>
                <div style={{ textAlign: 'right' }}>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>{item.price ?? '--'}</div>
                  <div style={{ fontSize: 11, color: (item.change_pct ?? 0) >= 0 ? 'var(--color-text-success)' : 'var(--color-text-danger)' }}>{item.change_pct ?? '--'}%</div>
                </div>
              </div>
            ))}
          </div>
        </div>

        <div style={sectionCard}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
            <div style={{ fontSize: 14, fontWeight: 700 }}>持仓摘要</div>
            <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>Top 5</span>
          </div>
          <div style={{ display: 'grid', gap: 8 }}>
            {overview.positions.slice(0, 5).map((item) => (
              <div key={item.symbol} style={{ display: 'grid', gridTemplateColumns: '90px 1fr 110px', gap: 10, alignItems: 'center', borderRadius: 12, padding: '10px 12px', background: 'var(--color-background-secondary)' }}>
                <div>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>{item.symbol}</div>
                  <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{item.name}</div>
                </div>
                <div style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>数量 {item.quantity} / 成本 {item.cost_price}</div>
                <div style={{ textAlign: 'right' }}>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>¥ {formatMoney(item.market_value)}</div>
                  <div style={{ fontSize: 11, color: item.pnl >= 0 ? 'var(--color-text-success)' : 'var(--color-text-danger)' }}>{item.pnl_pct.toFixed(2)}%</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
