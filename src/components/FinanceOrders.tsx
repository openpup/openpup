import React from 'react';
import { useFinanceStore } from '../stores/financeStore';

const panel: React.CSSProperties = {
  borderRadius: 16,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  padding: '16px 18px',
};

const money = (value: unknown) =>
  typeof value === 'number'
    ? new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 }).format(value)
    : '--';

const renderCell = (value: unknown) => {
  if (value == null) return '--';
  if (typeof value === 'number') return money(value);
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  return String(value);
};

export const FinanceOrders: React.FC = () => {
  const { ordersSnapshot } = useFinanceStore();

  if (!ordersSnapshot) {
    return <div style={panel}>正在加载订单数据…</div>;
  }

  const orderColumns = Object.keys(ordersSnapshot.orders[0] ?? {}).slice(0, 6);
  const tradeColumns = Object.keys(ordersSnapshot.trades[0] ?? {}).slice(0, 6);

  return (
    <div style={{ display: 'grid', gap: 16 }}>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 14 }}>
        <div style={{ ...panel, background: 'rgba(16,59,47,0.06)' }}>
          <div style={{ fontSize: 12, fontWeight: 700, color: '#0E6A4C' }}>总资产</div>
          <div style={{ marginTop: 10, fontSize: 26, fontWeight: 760 }}>¥ {money(ordersSnapshot.balance?.total_assets)}</div>
        </div>
        <div style={{ ...panel, background: 'rgba(186,117,23,0.08)' }}>
          <div style={{ fontSize: 12, fontWeight: 700, color: '#8A5A10' }}>可用资金</div>
          <div style={{ marginTop: 10, fontSize: 26, fontWeight: 760 }}>¥ {money(ordersSnapshot.balance?.available_balance)}</div>
        </div>
        <div style={{ ...panel, background: 'rgba(55,138,221,0.08)' }}>
          <div style={{ fontSize: 12, fontWeight: 700, color: '#1A5EA0' }}>总盈亏</div>
          <div style={{ marginTop: 10, fontSize: 26, fontWeight: 760 }}>¥ {money(ordersSnapshot.balance?.total_pnl)}</div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1.15fr 1fr', gap: 16 }}>
        <div style={panel}>
          <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 10 }}>持仓</div>
          <div style={{ overflowX: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
              <thead>
                <tr>
                  {['代码', '名称', '数量', '可卖', '成本', '现价', '市值', '盈亏%'].map((column) => (
                    <th key={column} style={{ textAlign: 'left', padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-tertiary)', fontWeight: 600 }}>{column}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {ordersSnapshot.positions.map((item) => (
                  <tr key={item.symbol}>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.symbol}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.name}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.quantity}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.available_quantity}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.cost_price}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{item.current_price}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{money(item.market_value)}</td>
                    <td style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)', color: item.pnl_pct >= 0 ? 'var(--color-text-success)' : 'var(--color-text-danger)' }}>{item.pnl_pct.toFixed(2)}%</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div style={{ display: 'grid', gap: 16 }}>
          <div style={panel}>
            <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 10 }}>今日委托</div>
            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
                <thead>
                  <tr>
                    {orderColumns.map((column) => (
                      <th key={column} style={{ textAlign: 'left', padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-tertiary)', fontWeight: 600 }}>{column}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {ordersSnapshot.orders.slice(0, 8).map((item, index) => (
                    <tr key={index}>
                      {orderColumns.map((column) => (
                        <td key={`${index}-${column}`} style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{renderCell(item[column])}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          <div style={panel}>
            <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 10 }}>今日成交</div>
            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
                <thead>
                  <tr>
                    {tradeColumns.map((column) => (
                      <th key={column} style={{ textAlign: 'left', padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-tertiary)', fontWeight: 600 }}>{column}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {ordersSnapshot.trades.slice(0, 8).map((item, index) => (
                    <tr key={index}>
                      {tradeColumns.map((column) => (
                        <td key={`${index}-${column}`} style={{ padding: '8px', borderBottom: '1px solid var(--color-border-tertiary)' }}>{renderCell(item[column])}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
