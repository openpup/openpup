import { create } from 'zustand';

export type ScenarioMode = 'default' | 'finance';

export type FinanceRoleKey = 'researcher' | 'strategist' | 'risk_officer' | 'executor' | 'reviewer';
export type FinanceSkillKey = 'premarket_scan' | 'intraday_check' | 'postmarket_review' | 'watchlist_cleanup' | 'emergency_stop';
export type FinanceConnectorKey = 'intel' | 'risk' | 'exec';
export type FinanceIntelCapabilityKey =
  | 'search_news'
  | 'get_quote'
  | 'get_candles'
  | 'screen_symbols'
  | 'list_watchlist'
  | 'add_watchlist'
  | 'remove_watchlist';
export type FinanceRiskCapabilityKey =
  | 'review_trade_intent'
  | 'validate_order'
  | 'validate_positions'
  | 'validate_market_status'
  | 'validate_exposure';
export type FinanceExecCapabilityKey =
  | 'get_account'
  | 'get_positions'
  | 'list_orders'
  | 'get_order_status'
  | 'place_order'
  | 'cancel_order';
export type FinanceCapabilityKey =
  | FinanceIntelCapabilityKey
  | FinanceRiskCapabilityKey
  | FinanceExecCapabilityKey;

export const FINANCE_ROLE_KEYS: FinanceRoleKey[] = ['researcher', 'strategist', 'risk_officer', 'executor', 'reviewer'];
export const FINANCE_SKILL_KEYS: FinanceSkillKey[] = ['premarket_scan', 'intraday_check', 'postmarket_review', 'watchlist_cleanup', 'emergency_stop'];
export const FINANCE_CONNECTOR_KEYS: FinanceConnectorKey[] = ['intel', 'risk', 'exec'];
export const FINANCE_CONNECTOR_CAPABILITY_KEYS: Record<FinanceConnectorKey, FinanceCapabilityKey[]> = {
  intel: ['search_news', 'get_quote', 'get_candles', 'screen_symbols', 'list_watchlist', 'add_watchlist', 'remove_watchlist'],
  risk: ['review_trade_intent', 'validate_order', 'validate_positions', 'validate_market_status', 'validate_exposure'],
  exec: ['get_account', 'get_positions', 'list_orders', 'get_order_status', 'place_order', 'cancel_order'],
};

export interface FinanceRoleBinding {
  pupKey: string | null;
}

export interface FinanceSkillBinding {
  mode: 'scenario_preset' | 'installed_skill';
  skillName: string | null;
}

export interface FinanceConnectorBinding {
  mode: 'mcp_server';
  serverName: string | null;
  capabilityBindings: Record<FinanceCapabilityKey, FinanceCapabilityBinding>;
}

export interface FinanceCapabilityBinding {
  mode: 'mcp_tool';
  toolName: string | null;
}

export interface FinanceRiskPreset {
  forceLeashed: boolean;
  requireManualApproval: boolean;
  singlePositionLimitPct: number;
  singleSectorLimitPct: number;
  dailyLossCircuitBreakerPct: number;
  boardLotSize: number;
  blockStSuspendedDelisting: boolean;
  enforceTradingWindow: boolean;
  enforceT1: boolean;
}

export interface FinanceScenarioConfig {
  roleBindings: Record<FinanceRoleKey, FinanceRoleBinding>;
  skillBindings: Record<FinanceSkillKey, FinanceSkillBinding>;
  connectorBindings: Record<FinanceConnectorKey, FinanceConnectorBinding>;
  riskPreset: FinanceRiskPreset;
}

export interface FinanceScenarioConfigPayload {
  roleBindings?: Array<{ role?: string; pupKey?: string | null }>;
  skillBindings?: Array<{ skill?: string; mode?: string; skillName?: string | null }>;
  connectorBindings?: Array<{
    connector?: string;
    mode?: string;
    serverName?: string | null;
    capabilityBindings?: Array<{ capability?: string; mode?: string; toolName?: string | null }>;
  }>;
  riskPreset?: Partial<FinanceRiskPreset>;
}

interface ScenarioState {
  mode: ScenarioMode;
  finance: FinanceScenarioConfig;
  setMode: (mode: ScenarioMode) => void;
  setFinanceConfig: (finance: FinanceScenarioConfig) => void;
  setFinanceRoleBinding: (role: FinanceRoleKey, pupKey: string | null) => void;
  setFinanceSkillBinding: (skill: FinanceSkillKey, binding: FinanceSkillBinding) => void;
  setFinanceConnectorBinding: (connector: FinanceConnectorKey, serverName: string | null) => void;
}

const STORAGE_KEY = 'openpup.scenarioStore.v1';

const DEFAULT_FINANCE_CONFIG: FinanceScenarioConfig = {
  roleBindings: {
    researcher: { pupKey: 'research' },
    strategist: { pupKey: 'strategist' },
    risk_officer: { pupKey: 'risk_officer' },
    executor: { pupKey: 'executor' },
    reviewer: { pupKey: 'reviewer' },
  },
  skillBindings: {
    premarket_scan: { mode: 'scenario_preset', skillName: null },
    intraday_check: { mode: 'scenario_preset', skillName: null },
    postmarket_review: { mode: 'scenario_preset', skillName: null },
    watchlist_cleanup: { mode: 'scenario_preset', skillName: null },
    emergency_stop: { mode: 'scenario_preset', skillName: null },
  },
  connectorBindings: {
    intel: {
      mode: 'mcp_server',
      serverName: 'intel',
      capabilityBindings: {
        search_news: { mode: 'mcp_tool', toolName: 'search_news' },
        get_quote: { mode: 'mcp_tool', toolName: 'get_quote' },
        get_candles: { mode: 'mcp_tool', toolName: 'get_candles' },
        screen_symbols: { mode: 'mcp_tool', toolName: 'screen_symbols' },
        list_watchlist: { mode: 'mcp_tool', toolName: 'list_watchlist' },
        add_watchlist: { mode: 'mcp_tool', toolName: 'add_watchlist' },
        remove_watchlist: { mode: 'mcp_tool', toolName: 'remove_watchlist' },
        review_trade_intent: { mode: 'mcp_tool', toolName: null },
        validate_order: { mode: 'mcp_tool', toolName: null },
        validate_positions: { mode: 'mcp_tool', toolName: null },
        validate_market_status: { mode: 'mcp_tool', toolName: null },
        validate_exposure: { mode: 'mcp_tool', toolName: null },
        get_account: { mode: 'mcp_tool', toolName: null },
        get_positions: { mode: 'mcp_tool', toolName: null },
        list_orders: { mode: 'mcp_tool', toolName: null },
        get_order_status: { mode: 'mcp_tool', toolName: null },
        place_order: { mode: 'mcp_tool', toolName: null },
        cancel_order: { mode: 'mcp_tool', toolName: null },
      },
    },
    risk: {
      mode: 'mcp_server',
      serverName: 'risk',
      capabilityBindings: {
        search_news: { mode: 'mcp_tool', toolName: null },
        get_quote: { mode: 'mcp_tool', toolName: null },
        get_candles: { mode: 'mcp_tool', toolName: null },
        screen_symbols: { mode: 'mcp_tool', toolName: null },
        list_watchlist: { mode: 'mcp_tool', toolName: null },
        add_watchlist: { mode: 'mcp_tool', toolName: null },
        remove_watchlist: { mode: 'mcp_tool', toolName: null },
        review_trade_intent: { mode: 'mcp_tool', toolName: 'review_trade_intent' },
        validate_order: { mode: 'mcp_tool', toolName: 'validate_order' },
        validate_positions: { mode: 'mcp_tool', toolName: 'validate_positions' },
        validate_market_status: { mode: 'mcp_tool', toolName: 'validate_market_status' },
        validate_exposure: { mode: 'mcp_tool', toolName: 'validate_exposure' },
        get_account: { mode: 'mcp_tool', toolName: null },
        get_positions: { mode: 'mcp_tool', toolName: null },
        list_orders: { mode: 'mcp_tool', toolName: null },
        get_order_status: { mode: 'mcp_tool', toolName: null },
        place_order: { mode: 'mcp_tool', toolName: null },
        cancel_order: { mode: 'mcp_tool', toolName: null },
      },
    },
    exec: {
      mode: 'mcp_server',
      serverName: 'exec',
      capabilityBindings: {
        search_news: { mode: 'mcp_tool', toolName: null },
        get_quote: { mode: 'mcp_tool', toolName: null },
        get_candles: { mode: 'mcp_tool', toolName: null },
        screen_symbols: { mode: 'mcp_tool', toolName: null },
        list_watchlist: { mode: 'mcp_tool', toolName: null },
        add_watchlist: { mode: 'mcp_tool', toolName: null },
        remove_watchlist: { mode: 'mcp_tool', toolName: null },
        review_trade_intent: { mode: 'mcp_tool', toolName: null },
        validate_order: { mode: 'mcp_tool', toolName: null },
        validate_positions: { mode: 'mcp_tool', toolName: null },
        validate_market_status: { mode: 'mcp_tool', toolName: null },
        validate_exposure: { mode: 'mcp_tool', toolName: null },
        get_account: { mode: 'mcp_tool', toolName: 'get_account' },
        get_positions: { mode: 'mcp_tool', toolName: 'get_positions' },
        list_orders: { mode: 'mcp_tool', toolName: 'list_orders' },
        get_order_status: { mode: 'mcp_tool', toolName: 'get_order_status' },
        place_order: { mode: 'mcp_tool', toolName: 'place_order' },
        cancel_order: { mode: 'mcp_tool', toolName: 'cancel_order' },
      },
    },
  },
  riskPreset: {
    forceLeashed: true,
    requireManualApproval: true,
    singlePositionLimitPct: 20,
    singleSectorLimitPct: 40,
    dailyLossCircuitBreakerPct: 3,
    boardLotSize: 100,
    blockStSuspendedDelisting: true,
    enforceTradingWindow: true,
    enforceT1: true,
  },
};

function isFinanceRoleKey(value: string): value is FinanceRoleKey {
  return FINANCE_ROLE_KEYS.includes(value as FinanceRoleKey);
}

function isFinanceSkillKey(value: string): value is FinanceSkillKey {
  return FINANCE_SKILL_KEYS.includes(value as FinanceSkillKey);
}

function isFinanceConnectorKey(value: string): value is FinanceConnectorKey {
  return FINANCE_CONNECTOR_KEYS.includes(value as FinanceConnectorKey);
}

function isFinanceCapabilityKey(value: string): value is FinanceCapabilityKey {
  return Object.values(FINANCE_CONNECTOR_CAPABILITY_KEYS).some((keys) => keys.includes(value as FinanceCapabilityKey));
}

function defaultCapabilityBindings(): Record<FinanceCapabilityKey, FinanceCapabilityBinding> {
  const defaults = structuredClone(DEFAULT_FINANCE_CONFIG.connectorBindings);
  return {
    ...defaults.intel.capabilityBindings,
    ...defaults.risk.capabilityBindings,
    ...defaults.exec.capabilityBindings,
  };
}

export function normalizeFinanceScenarioConfig(raw: unknown): FinanceScenarioConfig {
  const fallback = structuredClone(DEFAULT_FINANCE_CONFIG);
  if (!raw || typeof raw !== 'object') return fallback;

  const input = raw as FinanceScenarioConfig | FinanceScenarioConfigPayload;
  const roleBindings = { ...fallback.roleBindings };
  const skillBindings = { ...fallback.skillBindings };
  const connectorBindings = { ...fallback.connectorBindings };

  if (Array.isArray((input as FinanceScenarioConfigPayload).roleBindings)) {
    for (const item of (input as FinanceScenarioConfigPayload).roleBindings ?? []) {
      if (item?.role && isFinanceRoleKey(item.role)) {
        roleBindings[item.role] = { pupKey: item.pupKey ?? null };
      }
    }
  } else if ((input as FinanceScenarioConfig).roleBindings) {
    for (const key of FINANCE_ROLE_KEYS) {
      const item = (input as FinanceScenarioConfig).roleBindings[key];
      if (item) roleBindings[key] = { pupKey: item.pupKey ?? null };
    }
  }

  if (Array.isArray((input as FinanceScenarioConfigPayload).skillBindings)) {
    for (const item of (input as FinanceScenarioConfigPayload).skillBindings ?? []) {
      if (item?.skill && isFinanceSkillKey(item.skill)) {
        skillBindings[item.skill] = {
          mode: item.mode === 'installed_skill' ? 'installed_skill' : 'scenario_preset',
          skillName: item.skillName ?? null,
        };
      }
    }
  } else if ((input as FinanceScenarioConfig).skillBindings) {
    for (const key of FINANCE_SKILL_KEYS) {
      const item = (input as FinanceScenarioConfig).skillBindings[key];
      if (item) {
        skillBindings[key] = {
          mode: item.mode === 'installed_skill' ? 'installed_skill' : 'scenario_preset',
          skillName: item.skillName ?? null,
        };
      }
    }
  }

  if (Array.isArray((input as FinanceScenarioConfigPayload).connectorBindings)) {
    for (const item of (input as FinanceScenarioConfigPayload).connectorBindings ?? []) {
      if (item?.connector && isFinanceConnectorKey(item.connector)) {
        const capabilityBindings = {
          ...defaultCapabilityBindings(),
          ...connectorBindings[item.connector].capabilityBindings,
        };
        for (const capabilityItem of item.capabilityBindings ?? []) {
          if (capabilityItem?.capability && isFinanceCapabilityKey(capabilityItem.capability)) {
            capabilityBindings[capabilityItem.capability] = {
              mode: 'mcp_tool',
              toolName: capabilityItem.toolName ?? null,
            };
          }
        }
        connectorBindings[item.connector] = {
          mode: 'mcp_server',
          serverName: item.serverName ?? null,
          capabilityBindings,
        };
      }
    }
  } else if ((input as FinanceScenarioConfig).connectorBindings) {
    for (const key of FINANCE_CONNECTOR_KEYS) {
      const item = (input as FinanceScenarioConfig).connectorBindings[key];
      if (item) {
        const capabilityBindings = {
          ...defaultCapabilityBindings(),
          ...(item.capabilityBindings ?? {}),
        };
        for (const capability of Object.keys(capabilityBindings)) {
          capabilityBindings[capability as FinanceCapabilityKey] = {
            mode: 'mcp_tool',
            toolName: item.capabilityBindings?.[capability as FinanceCapabilityKey]?.toolName ?? capabilityBindings[capability as FinanceCapabilityKey].toolName ?? null,
          };
        }
        connectorBindings[key] = {
          mode: 'mcp_server',
          serverName: item.serverName ?? null,
          capabilityBindings,
        };
      }
    }
  }

  const rawRiskPreset = (input as FinanceScenarioConfigPayload).riskPreset ?? (input as FinanceScenarioConfig).riskPreset;
  const riskPreset: FinanceRiskPreset = {
    ...fallback.riskPreset,
    ...(rawRiskPreset ?? {}),
  };

  return {
    roleBindings,
    skillBindings,
    connectorBindings,
    riskPreset,
  };
}

export function toFinanceScenarioPayload(finance: FinanceScenarioConfig): FinanceScenarioConfigPayload {
  return {
    roleBindings: FINANCE_ROLE_KEYS.map((role) => ({
      role,
      pupKey: finance.roleBindings[role]?.pupKey ?? null,
    })),
    skillBindings: FINANCE_SKILL_KEYS.map((skill) => ({
      skill,
      mode: finance.skillBindings[skill]?.mode ?? 'scenario_preset',
      skillName: finance.skillBindings[skill]?.skillName ?? null,
    })),
    connectorBindings: FINANCE_CONNECTOR_KEYS.map((connector) => ({
      connector,
      mode: 'mcp_server',
      serverName: finance.connectorBindings[connector]?.serverName ?? null,
      capabilityBindings: FINANCE_CONNECTOR_CAPABILITY_KEYS[connector].map((capability) => ({
        capability,
        mode: 'mcp_tool',
        toolName: finance.connectorBindings[connector]?.capabilityBindings?.[capability]?.toolName ?? null,
      })),
    })),
    riskPreset: { ...finance.riskPreset },
  };
}

function safeLoad(): Pick<ScenarioState, 'mode' | 'finance'> | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Pick<ScenarioState, 'mode' | 'finance'>;
    if (!parsed || (parsed.mode !== 'default' && parsed.mode !== 'finance')) return null;
    return {
      mode: parsed.mode,
      finance: normalizeFinanceScenarioConfig(parsed.finance),
    };
  } catch {
    return null;
  }
}

function persist(state: Pick<ScenarioState, 'mode' | 'finance'>) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Ignore persistence failures and keep the in-memory state.
  }
}

const persisted = typeof window !== 'undefined' ? safeLoad() : null;

export const useScenarioStore = create<ScenarioState>((set) => ({
  mode: persisted?.mode ?? 'default',
  finance: persisted?.finance ?? DEFAULT_FINANCE_CONFIG,

  setMode: (mode) =>
    set((state) => {
      const next = { ...state, mode };
      persist({ mode: next.mode, finance: next.finance });
      return { mode };
    }),

  setFinanceConfig: (finance) =>
    set((state) => {
      const normalized = normalizeFinanceScenarioConfig(finance);
      persist({ mode: state.mode, finance: normalized });
      return { finance: normalized };
    }),

  setFinanceRoleBinding: (role, pupKey) =>
    set((state) => {
      const finance = {
        ...state.finance,
        roleBindings: {
          ...state.finance.roleBindings,
          [role]: { pupKey },
        },
      };
      persist({ mode: state.mode, finance });
      return { finance };
    }),

  setFinanceSkillBinding: (skill, binding) =>
    set((state) => {
      const finance = {
        ...state.finance,
        skillBindings: {
          ...state.finance.skillBindings,
          [skill]: binding,
        },
      };
      persist({ mode: state.mode, finance });
      return { finance };
    }),

  setFinanceConnectorBinding: (connector, serverName) =>
    set((state) => {
      const finance = {
        ...state.finance,
        connectorBindings: {
          ...state.finance.connectorBindings,
          [connector]: {
            mode: 'mcp_server',
            serverName,
            capabilityBindings: state.finance.connectorBindings[connector].capabilityBindings,
          },
        },
      };
      persist({ mode: state.mode, finance });
      return { finance };
    }),
}));
