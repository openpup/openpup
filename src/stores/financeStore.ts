import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export type FinanceTab = 'overview' | 'research' | 'orders' | 'pipeline';
export type HealthStatus = 'up' | 'down' | 'unconfigured';

export interface FinanceServiceHealth {
  status: HealthStatus;
  message: string | null;
}

export interface FinanceHealthSnapshot {
  intel: FinanceServiceHealth;
  risk: FinanceServiceHealth;
  exec: FinanceServiceHealth;
  checked_at: string | null;
}

export interface FinanceMarketStatus {
  date: string | null;
  is_trading_day: boolean | null;
  current_session: string | null;
  is_open: boolean | null;
  server_time: string | null;
}

export interface FinanceBalance {
  total_assets: number;
  available_balance: number;
  frozen: number;
  total_pnl: number;
  total_pnl_pct: number;
}

export interface FinancePnl {
  [key: string]: unknown;
}

export interface WatchlistItem {
  code: string;
  name: string;
  price?: number | null;
  change_pct?: number | null;
  pe?: number | null;
  market_cap?: number | null;
  extra?: Record<string, unknown>;
}

export interface PositionItem {
  symbol: string;
  name: string;
  quantity: number;
  available_quantity: number;
  cost_price: number;
  current_price: number;
  market_value: number;
  pnl: number;
  pnl_pct: number;
  industry?: string | null;
}

export interface NewsItem {
  title: string;
  content: string;
  date: string;
  source: string;
  type: string;
  rating?: string | null;
  entity?: string | null;
}

export interface DataTable {
  title: string;
  entity: string;
  columns: string[];
  rows: Array<Record<string, unknown>>;
  condition?: string | null;
}

export interface OrderItem {
  [key: string]: unknown;
}

export interface TradeItem {
  [key: string]: unknown;
}

export interface TradeIntent {
  symbol: string;
  market: string;
  direction: string;
  thesis?: string;
  confidence?: number;
  entry_rule?: string;
  exit_rule?: string;
  max_position_pct?: number;
  time_horizon?: string;
  valid_until?: string;
  risk_notes?: string;
  tool_evidence?: string[];
  approval_status?: string;
  rejection_reason?: string | null;
  risk_flags?: string[];
  adjusted_position_pct?: number;
  checked_at?: string;
}

export interface FinanceOrderPreview {
  symbol: string;
  market: string;
  intent_direction: string;
  order_direction: string;
  approval_status: string;
  price: number;
  quantity: number;
  amount: number;
  position_pct: number;
  order_type: string;
  entry_rule?: string;
  thesis?: string;
  notes: string[];
}

export interface FinanceOverviewSnapshot {
  health: FinanceHealthSnapshot;
  market_status: FinanceMarketStatus;
  balance: FinanceBalance | null;
  positions: PositionItem[];
  watchlist: WatchlistItem[];
  pnl: FinancePnl | null;
  active_order_count: number;
  today_trade_count: number;
}

export interface FinanceSymbolSnapshot {
  symbol: string;
  news: NewsItem[];
  tables: DataTable[];
}

export interface FinanceOrdersSnapshot {
  balance: FinanceBalance | null;
  positions: PositionItem[];
  orders: OrderItem[];
  trades: TradeItem[];
  pnl: FinancePnl | null;
}

const OVERVIEW_TTL_MS = 20_000;
const ORDERS_TTL_MS = 20_000;
const WATCHLIST_TTL_MS = 45_000;
const SYMBOL_TTL_MS = 120_000;
const SCREEN_TTL_MS = 120_000;

let overviewCache: { data: FinanceOverviewSnapshot; fetchedAt: number } | null = null;
let ordersCache: { data: FinanceOrdersSnapshot; fetchedAt: number } | null = null;
let watchlistCache: { data: WatchlistItem[]; fetchedAt: number } | null = null;
const symbolCache = new Map<string, { data: FinanceSymbolSnapshot; fetchedAt: number }>();
const screenCache = new Map<string, { data: WatchlistItem[]; fetchedAt: number }>();
let overviewPending: Promise<void> | null = null;
let ordersPending: Promise<void> | null = null;
let watchlistPending: Promise<void> | null = null;
const symbolPending = new Map<string, Promise<void>>();
const screenPending = new Map<string, Promise<void>>();

const isFresh = (fetchedAt: number, ttlMs: number) => Date.now() - fetchedAt < ttlMs;

interface FinanceStore {
  activeTab: FinanceTab;
  selectedSymbol: string | null;
  overview: FinanceOverviewSnapshot | null;
  symbolSnapshot: FinanceSymbolSnapshot | null;
  ordersSnapshot: FinanceOrdersSnapshot | null;
  watchlist: WatchlistItem[];
  screenerResults: WatchlistItem[];
  intents: TradeIntent[];
  pipelineInput: string;
  pipelineError: string | null;
  orderPreview: FinanceOrderPreview | null;
  orderExecutionResult: OrderItem | null;
  loading: {
    overview: boolean;
    research: boolean;
    orders: boolean;
    pipeline: boolean;
  };
  error: string | null;
  setActiveTab: (tab: FinanceTab) => void;
  setSelectedSymbol: (symbol: string | null) => void;
  setPipelineInput: (value: string) => void;
  setIntents: (intents: TradeIntent[]) => void;
  addDraftIntent: (intent: TradeIntent) => void;
  removeIntent: (symbol: string, index: number) => void;
  updateIntent: (index: number, patch: Partial<TradeIntent>) => void;
  applyPipelineInput: () => void;
  prepareOrderAt: (index: number) => Promise<void>;
  placeOrderAt: (index: number) => Promise<void>;
  clearOrderPreview: () => void;
  loadOverview: (force?: boolean) => Promise<void>;
  loadOrders: (force?: boolean) => Promise<void>;
  loadSymbolSnapshot: (symbol: string, force?: boolean) => Promise<void>;
  loadWatchlist: (force?: boolean) => Promise<void>;
  runScreenStocks: (query: string, force?: boolean) => Promise<void>;
  updateWatchlist: (action: 'add' | 'delete', stock: string) => Promise<void>;
  checkIntentAt: (index: number) => Promise<void>;
  batchCheckIntents: () => Promise<void>;
}

export const useFinanceStore = create<FinanceStore>((set, get) => ({
  activeTab: 'overview',
  selectedSymbol: null,
  overview: null,
  symbolSnapshot: null,
  ordersSnapshot: null,
  watchlist: [],
  screenerResults: [],
  intents: [],
  pipelineInput: '',
  pipelineError: null,
  orderPreview: null,
  orderExecutionResult: null,
  loading: {
    overview: false,
    research: false,
    orders: false,
    pipeline: false,
  },
  error: null,
  setActiveTab: (tab) => set({ activeTab: tab }),
  setSelectedSymbol: (symbol) => set({ selectedSymbol: symbol }),
  setPipelineInput: (value) => set({ pipelineInput: value, pipelineError: null }),
  setIntents: (intents) => set({ intents, pipelineInput: JSON.stringify(intents, null, 2), pipelineError: null }),
  addDraftIntent: (intent) => {
    const existing = get().intents;
    const deduped = existing.filter((item) => !(item.symbol === intent.symbol && item.direction === intent.direction));
    const intents = [intent, ...deduped];
    set({
      intents,
      pipelineInput: JSON.stringify(intents, null, 2),
      pipelineError: null,
      activeTab: 'pipeline',
    });
  },
  removeIntent: (symbol, index) => {
    const intents = get().intents.filter((item, itemIndex) => !(item.symbol === symbol && itemIndex === index));
    set({
      intents,
      pipelineInput: JSON.stringify(intents, null, 2),
      pipelineError: null,
    });
  },
  updateIntent: (index, patch) => {
    const intents = get().intents.map((item, itemIndex) => (
      itemIndex === index
        ? {
            ...item,
            ...patch,
            approval_status: patch.approval_status ?? 'pending',
            rejection_reason: patch.approval_status === 'rejected' ? patch.rejection_reason ?? item.rejection_reason ?? null : null,
          }
        : item
    ));
    set({
      intents,
      pipelineInput: JSON.stringify(intents, null, 2),
      pipelineError: null,
    });
  },
  applyPipelineInput: () => {
    const intents = JSON.parse(get().pipelineInput) as TradeIntent[];
    set({ intents, pipelineError: null, orderPreview: null, orderExecutionResult: null });
  },
  prepareOrderAt: async (index) => {
    const intent = get().intents[index];
    if (!intent) return;
    set((state) => ({ loading: { ...state.loading, pipeline: true }, pipelineError: null, orderExecutionResult: null }));
    try {
      const orderPreview = await invoke<FinanceOrderPreview>('finance_prepare_order', { intent });
      set({ orderPreview });
    } catch (error) {
      set({ pipelineError: String(error), orderPreview: null });
    } finally {
      set((state) => ({ loading: { ...state.loading, pipeline: false } }));
    }
  },
  placeOrderAt: async (index) => {
    const intent = get().intents[index];
    if (!intent) return;
    set((state) => ({ loading: { ...state.loading, pipeline: true }, pipelineError: null }));
    try {
      const orderExecutionResult = await invoke<OrderItem>('finance_place_order', { intent });
      set({ orderExecutionResult, orderPreview: null });
      await get().loadOrders(true);
      await get().loadOverview(true);
    } catch (error) {
      set({ pipelineError: String(error) });
    } finally {
      set((state) => ({ loading: { ...state.loading, pipeline: false } }));
    }
  },
  clearOrderPreview: () => set({ orderPreview: null, orderExecutionResult: null }),
  loadOverview: async (force = false) => {
    if (!force && overviewCache && isFresh(overviewCache.fetchedAt, OVERVIEW_TTL_MS)) {
      set((state) => ({
        overview: overviewCache!.data,
        watchlist: state.watchlist.length > 0 ? state.watchlist : overviewCache!.data.watchlist,
        selectedSymbol: state.selectedSymbol ?? overviewCache!.data.watchlist[0]?.code ?? null,
      }));
      return;
    }
    if (!force && overviewPending) {
      await overviewPending;
      return;
    }
    set((state) => ({ loading: { ...state.loading, overview: true }, error: null }));
    overviewPending = (async () => {
      try {
        const overview = await invoke<FinanceOverviewSnapshot>('finance_overview_snapshot');
        overviewCache = { data: overview, fetchedAt: Date.now() };
        set({ overview, watchlist: overview.watchlist, selectedSymbol: get().selectedSymbol ?? overview.watchlist[0]?.code ?? null });
      } catch (error) {
        set({ error: String(error) });
      } finally {
        overviewPending = null;
        set((state) => ({ loading: { ...state.loading, overview: false } }));
      }
    })();
    await overviewPending;
  },
  loadOrders: async (force = false) => {
    if (!force && ordersCache && isFresh(ordersCache.fetchedAt, ORDERS_TTL_MS)) {
      set({ ordersSnapshot: ordersCache.data });
      return;
    }
    if (!force && ordersPending) {
      await ordersPending;
      return;
    }
    set((state) => ({ loading: { ...state.loading, orders: true }, error: null }));
    ordersPending = (async () => {
      try {
        const ordersSnapshot = await invoke<FinanceOrdersSnapshot>('finance_orders_snapshot');
        ordersCache = { data: ordersSnapshot, fetchedAt: Date.now() };
        set({ ordersSnapshot });
      } catch (error) {
        set({ error: String(error) });
      } finally {
        ordersPending = null;
        set((state) => ({ loading: { ...state.loading, orders: false } }));
      }
    })();
    await ordersPending;
  },
  loadSymbolSnapshot: async (symbol, force = false) => {
    if (!symbol.trim()) return;
    const cached = symbolCache.get(symbol);
    if (!force && cached && isFresh(cached.fetchedAt, SYMBOL_TTL_MS)) {
      set({ symbolSnapshot: cached.data, selectedSymbol: symbol });
      return;
    }
    const pending = symbolPending.get(symbol);
    if (!force && pending) {
      await pending;
      return;
    }
    set((state) => ({ loading: { ...state.loading, research: true }, error: null, selectedSymbol: symbol }));
    const task = (async () => {
      try {
        const symbolSnapshot = await invoke<FinanceSymbolSnapshot>('finance_symbol_snapshot', { symbol });
        symbolCache.set(symbol, { data: symbolSnapshot, fetchedAt: Date.now() });
        set({ symbolSnapshot });
      } catch (error) {
        set({ error: String(error) });
      } finally {
        symbolPending.delete(symbol);
        set((state) => ({ loading: { ...state.loading, research: false } }));
      }
    })();
    symbolPending.set(symbol, task);
    await task;
  },
  loadWatchlist: async (force = false) => {
    if (!force && watchlistCache && isFresh(watchlistCache.fetchedAt, WATCHLIST_TTL_MS)) {
      set((state) => ({ watchlist: watchlistCache!.data, selectedSymbol: state.selectedSymbol ?? watchlistCache!.data[0]?.code ?? null }));
      return;
    }
    if (!force && watchlistPending) {
      await watchlistPending;
      return;
    }
    set((state) => ({ loading: { ...state.loading, research: true }, error: null }));
    watchlistPending = (async () => {
      try {
        const watchlist = await invoke<WatchlistItem[]>('finance_get_watchlist');
        watchlistCache = { data: watchlist, fetchedAt: Date.now() };
        set((state) => ({ watchlist, selectedSymbol: state.selectedSymbol ?? watchlist[0]?.code ?? null }));
      } catch (error) {
        set({ error: String(error) });
      } finally {
        watchlistPending = null;
        set((state) => ({ loading: { ...state.loading, research: false } }));
      }
    })();
    await watchlistPending;
  },
  runScreenStocks: async (query, force = false) => {
    if (!query.trim()) return;
    const cacheKey = query.trim();
    const cached = screenCache.get(cacheKey);
    if (!force && cached && isFresh(cached.fetchedAt, SCREEN_TTL_MS)) {
      set({ screenerResults: cached.data });
      return;
    }
    const pending = screenPending.get(cacheKey);
    if (!force && pending) {
      await pending;
      return;
    }
    set((state) => ({ loading: { ...state.loading, research: true }, error: null }));
    const task = (async () => {
      try {
        const result = await invoke<{ stocks: WatchlistItem[] }>('finance_screen_stocks', { query, limit: 30, sortBy: null, sortDesc: true });
        const stocks = result.stocks ?? [];
        screenCache.set(cacheKey, { data: stocks, fetchedAt: Date.now() });
        set({ screenerResults: stocks });
      } catch (error) {
        set({ error: String(error) });
      } finally {
        screenPending.delete(cacheKey);
        set((state) => ({ loading: { ...state.loading, research: false } }));
      }
    })();
    screenPending.set(cacheKey, task);
    await task;
  },
  updateWatchlist: async (action, stock) => {
    await invoke('finance_update_watchlist', { action, stock });
    watchlistCache = null;
    overviewCache = null;
    await get().loadWatchlist(true);
  },
  checkIntentAt: async (index) => {
    const target = get().intents[index];
    if (!target) return;
    set((state) => ({ loading: { ...state.loading, pipeline: true }, pipelineError: null, error: null }));
    try {
      const result = await invoke<{ results: TradeIntent[] }>('finance_batch_check', { intents: [target] });
      const checked = result.results?.[0];
      if (!checked) return;
      const intents = get().intents.map((item, itemIndex) => (itemIndex === index ? checked : item));
      set({
        intents,
        pipelineInput: JSON.stringify(intents, null, 2),
      });
    } catch (error) {
      set({ pipelineError: String(error) });
    } finally {
      set((state) => ({ loading: { ...state.loading, pipeline: false } }));
    }
  },
  batchCheckIntents: async () => {
    set((state) => ({ loading: { ...state.loading, pipeline: true }, pipelineError: null, error: null }));
    try {
      const parsed = JSON.parse(get().pipelineInput) as TradeIntent[];
      const result = await invoke<{ results: TradeIntent[] }>('finance_batch_check', { intents: parsed });
      const intents = result.results ?? parsed;
      set({ intents, pipelineInput: JSON.stringify(intents, null, 2) });
    } catch (error) {
      set({ pipelineError: String(error) });
    } finally {
      set((state) => ({ loading: { ...state.loading, pipeline: false } }));
    }
  },
}));
