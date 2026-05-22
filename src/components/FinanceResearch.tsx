import React, { useEffect, useMemo, useState } from 'react';
import { useFinanceStore } from '../stores/financeStore';

const panel: React.CSSProperties = {
  borderRadius: 16,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  padding: '16px 18px',
  minHeight: 560,
};

export const FinanceResearch: React.FC = () => {
  const [symbolInput, setSymbolInput] = useState('');
  const [watchlistAction, setWatchlistAction] = useState<'add' | 'delete' | null>(null);
  const [feedback, setFeedback] = useState<{ tone: 'success' | 'info'; text: string } | null>(null);
  const {
    watchlist,
    screenerResults,
    symbolSnapshot,
    selectedSymbol,
    intents,
    loading,
    setSelectedSymbol,
    loadSymbolSnapshot,
    updateWatchlist,
    addDraftIntent,
  } = useFinanceStore();

  useEffect(() => {
    if (selectedSymbol) {
      setSymbolInput(selectedSymbol);
      void loadSymbolSnapshot(selectedSymbol);
    }
  }, [selectedSymbol]);

  useEffect(() => {
    if (!feedback) return;
    const timer = window.setTimeout(() => setFeedback(null), 1800);
    return () => window.clearTimeout(timer);
  }, [feedback]);

  const displayList = useMemo(() => screenerResults.length > 0 ? screenerResults : watchlist, [screenerResults, watchlist]);
  const activeInstrument = useMemo(
    () => displayList.find((item) => item.code === selectedSymbol) ?? watchlist.find((item) => item.code === selectedSymbol) ?? null,
    [displayList, selectedSymbol, watchlist],
  );
  const currentIntent = useMemo(
    () => intents.find((item) => item.symbol === selectedSymbol),
    [intents, selectedSymbol],
  );

  const createDraftIntent = () => {
    if (!selectedSymbol) return;
    const latestTable = symbolSnapshot?.tables?.[0];
    const latestRow = latestTable?.rows?.[0] ?? {};
    const evidence: string[] = [];
    if (typeof latestRow.close !== 'undefined') evidence.push(`close=${String(latestRow.close)}`);
    if (typeof latestRow.change_pct !== 'undefined') evidence.push(`change_pct=${String(latestRow.change_pct)}`);
    if (typeof latestRow.pe !== 'undefined') evidence.push(`pe=${String(latestRow.pe)}`);
    if ((symbolSnapshot?.news ?? []).length > 0) evidence.push(`news=${symbolSnapshot?.news[0]?.title ?? ''}`);

    const draft = {
      symbol: selectedSymbol,
      market: selectedSymbol.startsWith('6') ? 'SSE' : 'SZSE',
      direction: 'buy',
      thesis: activeInstrument
        ? `${activeInstrument.name} 进入研究观察池，等待进一步确认量价与消息面`
        : `${selectedSymbol} 进入研究观察池，等待进一步确认量价与消息面`,
      confidence: 0.62,
      entry_rule: '回踩关键均线企稳后小仓位试错',
      exit_rule: '跌破前低止损，放量上破后跟踪止盈',
      max_position_pct: 0.08,
      time_horizon: '3d',
      valid_until: '2026-05-15T15:00:00+08:00',
      risk_notes: '需结合盘中成交额、板块联动和最新公告确认',
      tool_evidence: evidence.length > 0 ? evidence : ['等待补充结构化数据与新闻证据'],
      approval_status: 'pending',
    } as const;

    addDraftIntent(draft);
    setFeedback({ tone: 'success', text: `已为 ${selectedSymbol} 生成草稿` });
  };

  const buttonVars = (
    bg: string,
    hover: string,
    fg: string,
    border: string,
  ): React.CSSProperties => ({
    ['--finance-btn-bg' as string]: bg,
    ['--finance-btn-bg-hover' as string]: hover,
    ['--finance-btn-fg' as string]: fg,
    ['--finance-btn-border' as string]: border,
  });

  const queryBusy = loading.research && symbolInput.trim() === (selectedSymbol ?? symbolInput).trim();
  const watchlistBusy = watchlistAction !== null;

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '320px minmax(0, 1fr) 260px', gap: 16 }}>
      <div style={{ ...panel, display: 'grid', gap: 12, alignContent: 'start' }}>
        <div>
          <div style={{ fontSize: 14, fontWeight: 700 }}>观察池 / 筛选结果</div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
            左侧优先展示筛选结果，没有筛选结果时展示自选股。点击任意标的会联动新闻和数据表。
          </div>
        </div>
        <div style={{ display: 'grid', gap: 8 }}>
          {displayList.map((item) => (
            <button
              key={item.code}
              onClick={() => {
                setSelectedSymbol(item.code);
                void loadSymbolSnapshot(item.code);
              }}
              style={{
                textAlign: 'left',
                border: selectedSymbol === item.code ? '1px solid rgba(16,59,47,0.35)' : '1px solid var(--color-border-tertiary)',
                borderRadius: 12,
                padding: '10px 12px',
                background: selectedSymbol === item.code ? 'rgba(16,59,47,0.06)' : 'var(--color-background-secondary)',
                cursor: 'pointer',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                <span style={{ fontSize: 12, fontWeight: 700 }}>{item.code}</span>
                <span style={{ fontSize: 11, color: (item.change_pct ?? 0) >= 0 ? 'var(--color-text-success)' : 'var(--color-text-danger)' }}>{item.change_pct ?? '--'}%</span>
              </div>
              <div style={{ marginTop: 4, fontSize: 12, color: 'var(--color-text-secondary)' }}>{item.name}</div>
            </button>
          ))}
        </div>
      </div>

      <div style={{ ...panel, display: 'grid', gap: 14, alignContent: 'start' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <input
            value={symbolInput}
            onChange={(e) => setSymbolInput(e.target.value)}
            placeholder="输入股票代码，例如 600519"
            style={{ flex: 1, borderRadius: 10, border: '1px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)', padding: '9px 12px', fontSize: 12 }}
          />
          <button
            onClick={async () => {
              const symbol = symbolInput.trim();
              setSelectedSymbol(symbol);
              await loadSymbolSnapshot(symbol);
              setFeedback({ tone: 'info', text: `已加载 ${symbol} 的研究数据` });
            }}
            disabled={!symbolInput.trim() || loading.research}
            data-busy={queryBusy}
            className="finance-button"
            style={buttonVars('linear-gradient(180deg, #103B2F, #0C2F25)', 'linear-gradient(180deg, #164A3A, #103B2F)', '#D7F4E8', 'rgba(16,59,47,0.26)')}
          >
            {queryBusy && <span className="finance-button__dot" />}
            {queryBusy ? '查询中…' : '查询'}
          </button>
        </div>

        <div>
          <div style={{ fontSize: 14, fontWeight: 700 }}>标的详情</div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)' }}>
            {selectedSymbol ? `当前标的 ${selectedSymbol}` : '选择一个标的后，这里会显示新闻和结构化数据。'}
          </div>
        </div>

        <div style={{ display: 'grid', gap: 12 }}>
          <div style={{ borderRadius: 14, padding: '12px 14px', background: 'var(--color-background-secondary)' }}>
            <div style={{ fontSize: 12, fontWeight: 700, marginBottom: 8 }}>相关新闻</div>
            <div style={{ display: 'grid', gap: 8 }}>
              {(symbolSnapshot?.news ?? []).slice(0, 4).map((item, idx) => (
                <div key={`${item.title}-${idx}`} style={{ borderRadius: 10, padding: '10px 12px', background: 'var(--color-background-primary)', border: '1px solid var(--color-border-tertiary)' }}>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>{item.title}</div>
                  <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>{item.source} · {item.date}</div>
                  <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>{item.content}</div>
                </div>
              ))}
              {(symbolSnapshot?.news ?? []).length === 0 && (
                <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{loading.research ? '正在加载新闻…' : '暂无新闻结果'}</div>
              )}
            </div>
          </div>

          <div style={{ borderRadius: 14, padding: '12px 14px', background: 'var(--color-background-secondary)' }}>
            <div style={{ fontSize: 12, fontWeight: 700, marginBottom: 8 }}>结构化数据</div>
            <div style={{ display: 'grid', gap: 10 }}>
              {(symbolSnapshot?.tables ?? []).slice(0, 3).map((table, idx) => (
                <div key={`${table.title}-${idx}`} style={{ borderRadius: 10, padding: '10px 12px', background: 'var(--color-background-primary)', border: '1px solid var(--color-border-tertiary)' }}>
                  <div style={{ fontSize: 12, fontWeight: 700 }}>{table.title}</div>
                  <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>{table.entity}</div>
                  <div style={{ marginTop: 8, overflowX: 'auto' }}>
                    <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11 }}>
                      <thead>
                        <tr>
                          {table.columns.slice(0, 5).map((column) => (
                            <th key={column} style={{ textAlign: 'left', padding: '6px 8px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-tertiary)', fontWeight: 600 }}>{column}</th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {table.rows.slice(0, 4).map((row, rowIndex) => (
                          <tr key={rowIndex}>
                            {table.columns.slice(0, 5).map((column) => (
                              <td key={`${rowIndex}-${column}`} style={{ padding: '6px 8px', borderBottom: '1px solid var(--color-border-tertiary)', color: 'var(--color-text-secondary)' }}>
                                {String(row[column] ?? '--')}
                              </td>
                            ))}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              ))}
              {(symbolSnapshot?.tables ?? []).length === 0 && (
                <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{loading.research ? '正在加载数据表…' : '暂无结构化数据'}</div>
              )}
            </div>
          </div>
        </div>
      </div>

      <div style={{ ...panel, display: 'grid', gap: 12, alignContent: 'start' }}>
        <div>
          <div style={{ fontSize: 14, fontWeight: 700 }}>动作区</div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
            这里已经能把当前标的生成成 TradeIntent 草稿，并带到 Pipeline 继续补仓位、入场规则和风控审批。
          </div>
        </div>
        <div style={{ borderRadius: 12, padding: '12px 14px', background: 'rgba(16,59,47,0.06)', border: '1px solid rgba(16,59,47,0.10)' }}>
          <div style={{ fontSize: 11, fontWeight: 700, color: '#0E6A4C' }}>当前联动标的</div>
          <div style={{ marginTop: 8, fontSize: 14, fontWeight: 700 }}>{selectedSymbol ?? '--'}</div>
          <div style={{ marginTop: 4, fontSize: 12, color: 'var(--color-text-secondary)' }}>{activeInstrument?.name ?? '尚未选择标的'}</div>
          {currentIntent && (
            <div style={{ marginTop: 8, fontSize: 11, color: '#0E6A4C' }}>
              已存在草稿，状态：{currentIntent.approval_status ?? 'pending'}
            </div>
          )}
        </div>
        <button
          onClick={async () => {
            if (!selectedSymbol) return;
            setWatchlistAction('add');
            try {
              await updateWatchlist('add', selectedSymbol);
              setFeedback({ tone: 'success', text: `${selectedSymbol} 已加入自选` });
            } finally {
              setWatchlistAction(null);
            }
          }}
          disabled={!selectedSymbol || watchlistBusy}
          data-busy={watchlistAction === 'add'}
          className="finance-button"
          style={buttonVars('linear-gradient(180deg, #103B2F, #0C2F25)', 'linear-gradient(180deg, #164A3A, #103B2F)', '#D7F4E8', 'rgba(16,59,47,0.26)')}
        >
          {watchlistAction === 'add' && <span className="finance-button__dot" />}
          {watchlistAction === 'add' ? '加入中…' : '加入自选'}
        </button>
        <button
          onClick={async () => {
            if (!selectedSymbol) return;
            setWatchlistAction('delete');
            try {
              await updateWatchlist('delete', selectedSymbol);
              setFeedback({ tone: 'info', text: `${selectedSymbol} 已从自选移除` });
            } finally {
              setWatchlistAction(null);
            }
          }}
          disabled={!selectedSymbol || watchlistBusy}
          data-busy={watchlistAction === 'delete'}
          className="finance-button"
          style={buttonVars('var(--color-background-danger)', 'color-mix(in srgb, var(--color-background-danger) 75%, white 25%)', 'var(--color-text-danger)', 'rgba(226,75,74,0.25)')}
        >
          {watchlistAction === 'delete' && <span className="finance-button__dot" />}
          {watchlistAction === 'delete' ? '移除中…' : '从自选移除'}
        </button>
        <button
          onClick={createDraftIntent}
          disabled={!selectedSymbol}
          className="finance-button"
          style={buttonVars('linear-gradient(180deg, #BA7517, #9B6210)', 'linear-gradient(180deg, #C78223, #A36513)', '#FFF7EB', 'rgba(155,98,16,0.28)')}
        >
          生成 TradeIntent 草稿
        </button>
        {feedback && (
          <div
            style={{
              borderRadius: 12,
              padding: '10px 12px',
              background: feedback.tone === 'success' ? 'rgba(29,158,117,0.10)' : 'rgba(55,138,221,0.10)',
              color: feedback.tone === 'success' ? '#0E6A4C' : '#1A5EA0',
              border: feedback.tone === 'success' ? '1px solid rgba(29,158,117,0.18)' : '1px solid rgba(55,138,221,0.18)',
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            {feedback.text}
          </div>
        )}
        <div style={{ borderRadius: 12, padding: '12px 14px', background: 'var(--color-background-secondary)', fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.7 }}>
          当前闭环：
          <br />
          1. 从这里发起 `TradeIntent` 草稿
          <br />
          2. 自动跳到 Pipeline 表格继续完善
          <br />
          3. 交给 `risk_mcp` 做批量风控
        </div>
      </div>
    </div>
  );
};
