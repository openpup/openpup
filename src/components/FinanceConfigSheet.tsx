import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  normalizeFinanceScenarioConfig,
  toFinanceScenarioPayload,
  useScenarioStore,
  type FinanceScenarioConfig,
  type FinanceScenarioConfigPayload,
  type FinanceConnectorKey,
  type FinanceRoleKey,
  type FinanceSkillKey,
} from '../stores/scenarioStore';
import { useLang } from '../i18n';

interface PupSummary {
  key: string;
  display_name: string;
  enabled: boolean;
}

interface InstalledSkill {
  name: string;
  category: string;
  source: string;
  enabled: boolean;
}

interface McpServer {
  name: string;
  base_url: string;
  enabled: boolean;
}

interface ScenarioSettingsSnapshot {
  finance: FinanceScenarioConfigPayload;
}

const roleMeta: Record<FinanceRoleKey, { zh: string; en: string; dutyZh: string; dutyEn: string }> = {
  researcher: { zh: '研究员', en: 'Researcher', dutyZh: '发现标的并输出 TradeIntent', dutyEn: 'Discover targets and produce TradeIntent' },
  strategist: { zh: '策略员', en: 'Strategist', dutyZh: '校正入场、出场、仓位与时效', dutyEn: 'Refine entry, exit, sizing, and validity' },
  risk_officer: { zh: '风控员', en: 'Risk Officer', dutyZh: '审批每笔交易', dutyEn: 'Approve each trade' },
  executor: { zh: '执行员', en: 'Executor', dutyZh: '执行已审批交易', dutyEn: 'Execute approved trades' },
  reviewer: { zh: '复盘员', en: 'Reviewer', dutyZh: '复盘归因与改进', dutyEn: 'Review outcomes and improve' },
};

const skillMeta: Record<FinanceSkillKey, { zh: string; en: string; triggerZh: string; triggerEn: string }> = {
  premarket_scan: { zh: '盘前扫描', en: 'Premarket Scan', triggerZh: '盘前扫描', triggerEn: 'premarket scan' },
  intraday_check: { zh: '盘中评估', en: 'Intraday Check', triggerZh: '盘中检查', triggerEn: 'intraday check' },
  postmarket_review: { zh: '收盘复盘', en: 'Postmarket Review', triggerZh: '收盘复盘', triggerEn: 'postmarket review' },
  watchlist_cleanup: { zh: '自选维护', en: 'Watchlist Cleanup', triggerZh: '自选清理', triggerEn: 'watchlist cleanup' },
  emergency_stop: { zh: '紧急止损', en: 'Emergency Stop', triggerZh: '紧急止损', triggerEn: 'emergency stop' },
};

const connectorMeta: Record<FinanceConnectorKey, { zh: string; en: string; alias: string }> = {
  intel: { zh: '资讯 / 行情 / 选股', en: 'Intel / Market / Screening', alias: 'mcp__intel__*' },
  risk: { zh: '风控审批', en: 'Risk Approval', alias: 'mcp__risk__*' },
  exec: { zh: '账户 / 持仓 / 下单', en: 'Account / Positions / Execution', alias: 'mcp__exec__*' },
};

const overlayStyle: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(10,16,13,0.28)',
  backdropFilter: 'blur(12px)',
  zIndex: 60,
  display: 'flex',
  justifyContent: 'flex-end',
};

const sheetStyle: React.CSSProperties = {
  width: 'min(460px, 92vw)',
  height: '100%',
  background: 'var(--color-background-primary)',
  borderLeft: '0.5px solid var(--color-border-tertiary)',
  boxShadow: '-24px 0 80px rgba(0,0,0,0.16)',
  overflowY: 'auto',
};

const cardStyle: React.CSSProperties = {
  borderRadius: 14,
  border: '0.5px solid var(--color-border-tertiary)',
  background: 'var(--color-background-secondary)',
  padding: '14px',
};

const miniCardStyle: React.CSSProperties = {
  borderRadius: 12,
  border: '0.5px solid var(--color-border-tertiary)',
  background: 'var(--color-background-primary)',
  padding: '12px 13px',
};

const selectStyle: React.CSSProperties = {
  width: '100%',
  borderRadius: 10,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '9px 10px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
};

const inputStyle: React.CSSProperties = {
  width: '100%',
  borderRadius: 10,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '9px 10px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
};

const FINANCE_CONFIG_EXPANDED_KEY = 'openpup.financeConfigSheet.expanded';

const sectionShellStyle: React.CSSProperties = {
  borderRadius: 12,
  border: '0.5px solid var(--color-border-tertiary)',
  background: 'var(--color-background-secondary)',
  overflow: 'hidden',
};

const sectionHeaderButtonStyle: React.CSSProperties = {
  width: '100%',
  border: 'none',
  background: 'transparent',
  cursor: 'pointer',
  padding: '12px 13px 10px',
  textAlign: 'left',
};

const pillStyle: React.CSSProperties = {
  padding: '3px 8px',
  borderRadius: 999,
  fontSize: 10,
  fontWeight: 600,
  lineHeight: 1.1,
};

const ROLE_KEYS: FinanceRoleKey[] = ['researcher', 'strategist', 'risk_officer', 'executor', 'reviewer'];
const SKILL_KEYS: FinanceSkillKey[] = ['premarket_scan', 'intraday_check', 'postmarket_review', 'watchlist_cleanup', 'emergency_stop'];
const CONNECTOR_KEYS: FinanceConnectorKey[] = ['intel', 'risk', 'exec'];

const ToggleRow: React.FC<{
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}> = ({ label, checked, onChange }) => (
  <label style={{
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    padding: '10px 12px',
    borderRadius: 12,
    border: '0.5px solid var(--color-border-tertiary)',
    background: 'var(--color-background-primary)',
  }}>
    <span style={{ fontSize: 12, color: 'var(--color-text-primary)' }}>{label}</span>
    <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
  </label>
);

function readExpandedSection(): 'risk' | 'workflow' | 'roles' | 'connectors' {
  try {
    const raw = window.localStorage.getItem(FINANCE_CONFIG_EXPANDED_KEY);
    if (raw === 'risk' || raw === 'workflow' || raw === 'roles' || raw === 'connectors') return raw;
  } catch {
    // ignore
  }
  return 'risk';
}

function persistExpandedSection(section: 'risk' | 'workflow' | 'roles' | 'connectors') {
  try {
    window.localStorage.setItem(FINANCE_CONFIG_EXPANDED_KEY, section);
  } catch {
    // ignore
  }
}

export const FinanceConfigSheet: React.FC<{
  open: boolean;
  onClose: () => void;
}> = ({ open, onClose }) => {
  const { lang } = useLang();
  const {
    finance,
    setFinanceConfig,
  } = useScenarioStore();
  const [pups, setPups] = useState<PupSummary[]>([]);
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [servers, setServers] = useState<McpServer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<'risk' | 'workflow' | 'roles' | 'connectors'>(() => readExpandedSection());

  useEffect(() => {
    if (!open) return;
    const load = async () => {
      setError(null);
      try {
        const [scenarioSnapshot, pupList, skillList, serverList] = await Promise.all([
          invoke<ScenarioSettingsSnapshot>('get_scenario_settings_snapshot'),
          invoke<PupSummary[]>('list_pups'),
          invoke<InstalledSkill[]>('list_skills'),
          invoke<McpServer[]>('list_mcp_servers'),
        ]);
        setFinanceConfig(normalizeFinanceScenarioConfig(scenarioSnapshot.finance));
        setPups(pupList.filter((item) => item.enabled));
        setSkills(skillList.filter((item) => item.enabled));
        setServers(serverList);
      } catch (e) {
        setError(String(e));
      }
    };
    void load();
  }, [open, setFinanceConfig]);

  const commitFinance = (nextFinance: FinanceScenarioConfig) => {
    setFinanceConfig(nextFinance);
    void invoke<ScenarioSettingsSnapshot>('save_finance_scenario_settings', {
      finance: toFinanceScenarioPayload(nextFinance),
    })
      .then((snapshot) => {
        setFinanceConfig(normalizeFinanceScenarioConfig(snapshot.finance));
      })
      .catch((e) => {
        setError(String(e));
      });
  };

  const connectorSummary = useMemo(() => (
    (['intel', 'risk', 'exec'] as const)
      .map((connector) => `${connector}→${finance.connectorBindings[connector].serverName ?? 'unbound'}`)
      .join(' · ')
  ), [finance]);
  const boundRoleCount = useMemo(
    () => (Object.values(finance.roleBindings).filter((item) => !!item.pupKey).length),
    [finance],
  );
  const customSkillCount = useMemo(
    () => (Object.values(finance.skillBindings).filter((item) => item.mode === 'installed_skill' && item.skillName).length),
    [finance],
  );
  const boundConnectorCount = useMemo(
    () => (Object.values(finance.connectorBindings).filter((item) => !!item.serverName).length),
    [finance],
  );
  const activeGuardCount = useMemo(
    () => [
      finance.riskPreset.forceLeashed,
      finance.riskPreset.requireManualApproval,
      finance.riskPreset.blockStSuspendedDelisting,
      finance.riskPreset.enforceTradingWindow,
      finance.riskPreset.enforceT1,
    ].filter(Boolean).length,
    [finance],
  );
  const workflowPresetCount = useMemo(
    () => (Object.values(finance.skillBindings).filter((item) => item.mode === 'scenario_preset').length),
    [finance],
  );
  const riskSummary = useMemo(() => ([
    [lang === 'zh' ? '单票' : 'Single name', `${finance.riskPreset.singlePositionLimitPct}%`],
    [lang === 'zh' ? '行业' : 'Sector', `${finance.riskPreset.singleSectorLimitPct}%`],
    [lang === 'zh' ? '日亏' : 'Daily stop', `${finance.riskPreset.dailyLossCircuitBreakerPct}%`],
    [lang === 'zh' ? '手数' : 'Lot size', `${finance.riskPreset.boardLotSize}`],
  ]), [finance, lang]);
  const workflowSummary = useMemo(() => (
    SKILL_KEYS.map((skill) => ({
      skill,
      label: lang === 'zh' ? skillMeta[skill].zh : skillMeta[skill].en,
      value: finance.skillBindings[skill].mode === 'installed_skill'
        ? (finance.skillBindings[skill].skillName ?? (lang === 'zh' ? '自定义' : 'Custom'))
        : (lang === 'zh' ? '场景预设' : 'Scenario preset'),
    }))
  ), [finance, lang]);
  const roleSummary = useMemo(() => (
    ROLE_KEYS.map((role) => ({
      role,
      label: lang === 'zh' ? roleMeta[role].zh : roleMeta[role].en,
      value: finance.roleBindings[role].pupKey ?? (lang === 'zh' ? '未绑定' : 'Unbound'),
    }))
  ), [finance, lang]);
  const connectorSummaryRows = useMemo(() => (
    CONNECTOR_KEYS.map((connector) => ({
      connector,
      label: lang === 'zh' ? connectorMeta[connector].zh : connectorMeta[connector].en,
      value: finance.connectorBindings[connector].serverName ?? (lang === 'zh' ? '未绑定' : 'Unbound'),
    }))
  ), [finance, lang]);

  const setExpandedSection = (section: 'risk' | 'workflow' | 'roles' | 'connectors') => {
    setExpanded(section);
    persistExpandedSection(section);
  };

  const updateRiskNumber = (
    key: 'singlePositionLimitPct' | 'singleSectorLimitPct' | 'dailyLossCircuitBreakerPct' | 'boardLotSize',
    value: string,
  ) => {
    const parsed = Number.parseInt(value, 10);
    if (Number.isNaN(parsed)) return;
    commitFinance({
      ...finance,
      riskPreset: {
        ...finance.riskPreset,
        [key]: Math.max(0, parsed),
      },
    });
  };

  const updateRiskBool = (
    key: 'forceLeashed' | 'requireManualApproval' | 'blockStSuspendedDelisting' | 'enforceTradingWindow' | 'enforceT1',
    checked: boolean,
  ) => {
    commitFinance({
      ...finance,
      riskPreset: {
        ...finance.riskPreset,
        [key]: checked,
      },
    });
  };

  if (!open) return null;

  return (
    <div style={overlayStyle} onClick={onClose}>
      <div style={sheetStyle} onClick={(e) => e.stopPropagation()}>
        <div style={{ padding: '16px 15px 16px', display: 'grid', gap: 12 }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 }}>
            <div style={{ display: 'grid', gap: 7 }}>
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
                border: '0.5px solid rgba(29,158,117,0.22)',
              }}>
                {lang === 'zh' ? 'Finance 场景' : 'Finance Scenario'}
              </span>
              <div style={{ fontSize: 20, fontWeight: 760, lineHeight: 1.08, color: 'var(--color-text-primary)' }}>
                {lang === 'zh' ? '金融场景配置' : 'Finance scenario settings'}
              </div>
              <div style={{ fontSize: 12, lineHeight: 1.65, color: 'var(--color-text-secondary)' }}>
                {lang === 'zh'
                  ? '定义风险边界、场景流程、角色绑定与连接器实现。'
                  : 'Define risk boundaries, workflow presets, role bindings, and connector implementations.'}
              </div>
            </div>
            <button
              onClick={onClose}
              style={{
                width: 32,
                height: 32,
                borderRadius: 10,
                border: '0.5px solid var(--color-border-tertiary)',
                background: 'var(--color-background-secondary)',
                color: 'var(--color-text-tertiary)',
                cursor: 'pointer',
              }}
            >
              ✕
            </button>
          </div>

          <div style={{
            ...cardStyle,
            padding: '13px',
            background: 'linear-gradient(180deg, rgba(16,59,47,0.06), rgba(16,59,47,0.02))',
            display: 'grid',
            gap: 9,
          }}>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              <span style={{ ...pillStyle, color: '#0E6A4C', background: 'rgba(29,158,117,0.12)', border: '0.5px solid rgba(29,158,117,0.20)' }}>
                {finance.riskPreset.forceLeashed ? 'Leashed' : (lang === 'zh' ? '可配置' : 'Configurable')}
              </span>
              <span style={{ ...pillStyle, color: '#8B6418', background: 'rgba(182,135,43,0.12)', border: '0.5px solid rgba(182,135,43,0.18)' }}>
                {finance.riskPreset.requireManualApproval ? (lang === 'zh' ? '人工确认' : 'Manual confirm') : (lang === 'zh' ? '可继续审批流' : 'Approval optional')}
              </span>
              <span style={{ ...pillStyle, color: '#2257C1', background: 'rgba(34,87,193,0.10)', border: '0.5px solid rgba(34,87,193,0.16)' }}>
                {lang === 'zh' ? 'TradeIntent 契约' : 'TradeIntent contract'}
              </span>
            </div>
            <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.65 }}>
              {lang === 'zh'
                ? 'Finance 会话会优先遵守风险规则，并通过固定角色链路完成研究、策略、风控、执行与复盘。'
                : 'Finance sessions prioritize safety rules and move through a fixed role chain for research, strategy, risk, execution, and review.'}
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 8 }}>
              {[
                [lang === 'zh' ? '风险' : 'Risk', `${activeGuardCount} ${lang === 'zh' ? '条' : 'guards'}`],
                [lang === 'zh' ? '流程' : 'Workflow', `${workflowPresetCount}/5 ${lang === 'zh' ? '预设' : 'preset'}`],
                [lang === 'zh' ? '角色' : 'Roles', `${boundRoleCount}/5`],
                [lang === 'zh' ? '连接器' : 'Connectors', `${boundConnectorCount}/3`],
              ].map(([label, value]) => (
                <div key={label} style={{ ...miniCardStyle, padding: '10px 11px' }}>
                  <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>{label}</div>
                  <div style={{ marginTop: 5, fontSize: 16, fontWeight: 760, color: 'var(--color-text-primary)' }}>{value}</div>
                </div>
              ))}
            </div>
          </div>

          <section style={{ display: 'grid', gap: 10 }}>
            <div style={sectionShellStyle}>
              <button onClick={() => setExpandedSection(expanded === 'risk' ? 'workflow' : 'risk')} style={sectionHeaderButtonStyle}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? '风险规则' : 'Risk Rules'}</div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.55 }}>
                      {lang === 'zh'
                        ? `单票 ${finance.riskPreset.singlePositionLimitPct}% · 行业 ${finance.riskPreset.singleSectorLimitPct}% · 日亏 ${finance.riskPreset.dailyLossCircuitBreakerPct}%`
                        : `Single ${finance.riskPreset.singlePositionLimitPct}% · Sector ${finance.riskPreset.singleSectorLimitPct}% · Daily stop ${finance.riskPreset.dailyLossCircuitBreakerPct}%`}
                    </div>
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{expanded === 'risk' ? '▾' : '▸'}</span>
                </div>
              </button>
              {expanded === 'risk' && (
                <div style={{ padding: '0 13px 13px', display: 'grid', gap: 10 }}>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 8 }}>
                    {riskSummary.map(([label, value]) => (
                      <div key={label} style={{ ...miniCardStyle, padding: '9px 10px', borderRadius: 10 }}>
                        <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>{label}</div>
                        <div style={{ marginTop: 5, fontSize: 15, fontWeight: 760, color: 'var(--color-text-primary)' }}>{value}</div>
                      </div>
                    ))}
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '单票仓位上限 (%)' : 'Single position limit (%)'}</span>
                      <input type="number" min={0} step={1} value={String(finance.riskPreset.singlePositionLimitPct)} onChange={(e) => updateRiskNumber('singlePositionLimitPct', e.target.value)} style={inputStyle} />
                    </label>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '单行业仓位上限 (%)' : 'Single sector limit (%)'}</span>
                      <input type="number" min={0} step={1} value={String(finance.riskPreset.singleSectorLimitPct)} onChange={(e) => updateRiskNumber('singleSectorLimitPct', e.target.value)} style={inputStyle} />
                    </label>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '日亏熔断阈值 (%)' : 'Daily loss circuit breaker (%)'}</span>
                      <input type="number" min={0} step={1} value={String(finance.riskPreset.dailyLossCircuitBreakerPct)} onChange={(e) => updateRiskNumber('dailyLossCircuitBreakerPct', e.target.value)} style={inputStyle} />
                    </label>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '最小委托手数' : 'Board lot size'}</span>
                      <input type="number" min={0} step={1} value={String(finance.riskPreset.boardLotSize)} onChange={(e) => updateRiskNumber('boardLotSize', e.target.value)} style={inputStyle} />
                    </label>
                  </div>
                  <div style={{ display: 'grid', gap: 7 }}>
                    <ToggleRow label={lang === 'zh' ? '强制 leashed 模式' : 'Force leashed mode'} checked={finance.riskPreset.forceLeashed} onChange={(checked) => updateRiskBool('forceLeashed', checked)} />
                    <ToggleRow label={lang === 'zh' ? '所有下单都必须人工确认' : 'Require manual confirmation for all orders'} checked={finance.riskPreset.requireManualApproval} onChange={(checked) => updateRiskBool('requireManualApproval', checked)} />
                    <ToggleRow label={lang === 'zh' ? '禁止 ST / 停牌 / 退市标的' : 'Block ST / suspended / delisting names'} checked={finance.riskPreset.blockStSuspendedDelisting} onChange={(checked) => updateRiskBool('blockStSuspendedDelisting', checked)} />
                    <ToggleRow label={lang === 'zh' ? '限制在交易时段内执行' : 'Restrict execution to trading hours'} checked={finance.riskPreset.enforceTradingWindow} onChange={(checked) => updateRiskBool('enforceTradingWindow', checked)} />
                    <ToggleRow label={lang === 'zh' ? '强制 T+1 规则' : 'Enforce T+1 rule'} checked={finance.riskPreset.enforceT1} onChange={(checked) => updateRiskBool('enforceT1', checked)} />
                  </div>
                </div>
              )}
            </div>

            <div style={sectionShellStyle}>
              <button onClick={() => setExpandedSection(expanded === 'workflow' ? 'risk' : 'workflow')} style={sectionHeaderButtonStyle}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? '场景流程' : 'Workflow'}</div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>
                      {lang === 'zh' ? `${workflowPresetCount}/5 使用场景预设，${customSkillCount}/5 使用自定义 skill` : `${workflowPresetCount}/5 use scenario presets, ${customSkillCount}/5 use custom skills`}
                    </div>
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{expanded === 'workflow' ? '▾' : '▸'}</span>
                </div>
              </button>
              {expanded === 'workflow' && (
                <div style={{ padding: '0 13px 13px', display: 'grid', gap: 9 }}>
                  {SKILL_KEYS.map((skill) => (
                    <div key={skill} style={{ ...miniCardStyle, display: 'grid', gap: 9, borderRadius: 10, padding: '11px 12px' }}>
                      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                        <div>
                          <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? skillMeta[skill].zh : skillMeta[skill].en}</div>
                          <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                            {lang === 'zh' ? `触发词：${skillMeta[skill].triggerZh}` : `Trigger: ${skillMeta[skill].triggerEn}`}
                          </div>
                        </div>
                        <span style={{
                          ...pillStyle,
                          color: finance.skillBindings[skill].mode === 'installed_skill' ? '#0E6A4C' : 'var(--color-text-secondary)',
                          background: finance.skillBindings[skill].mode === 'installed_skill' ? 'rgba(29,158,117,0.10)' : 'var(--color-background-secondary)',
                          border: '0.5px solid var(--color-border-tertiary)',
                          fontFamily: 'var(--font-mono)',
                        }}>
                          {finance.skillBindings[skill].mode === 'installed_skill'
                            ? (lang === 'zh' ? '自定义' : 'Custom')
                            : (lang === 'zh' ? '预设' : 'Preset')}
                        </span>
                      </div>
                      <select
                        value={finance.skillBindings[skill].mode === 'installed_skill' ? (finance.skillBindings[skill].skillName ?? '') : ''}
                        onChange={(e) => {
                          const skillName = e.target.value || null;
                          commitFinance({
                            ...finance,
                            skillBindings: {
                              ...finance.skillBindings,
                              [skill]: skillName ? { mode: 'installed_skill', skillName } : { mode: 'scenario_preset', skillName: null },
                            },
                          });
                        }}
                        style={selectStyle}
                      >
                        <option value="">{lang === 'zh' ? '使用场景预设' : 'Use scenario preset'}</option>
                        {skills.map((item) => (
                          <option key={item.name} value={item.name}>{item.name} · {item.category || item.source}</option>
                        ))}
                      </select>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div style={sectionShellStyle}>
              <button onClick={() => setExpandedSection(expanded === 'roles' ? 'workflow' : 'roles')} style={sectionHeaderButtonStyle}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? '角色绑定' : 'Role Bindings'}</div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>
                      {lang === 'zh' ? `${boundRoleCount}/5 角色已绑定到专家` : `${boundRoleCount}/5 roles mapped to specialists`}
                    </div>
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{expanded === 'roles' ? '▾' : '▸'}</span>
                </div>
              </button>
              {expanded === 'roles' && (
                <div style={{ padding: '0 13px 13px', display: 'grid', gap: 9 }}>
                  {ROLE_KEYS.map((role) => (
                    <div key={role} style={{ ...miniCardStyle, display: 'grid', gap: 9, borderRadius: 10, padding: '11px 12px' }}>
                      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                        <div>
                          <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? roleMeta[role].zh : roleMeta[role].en}</div>
                          <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.55 }}>
                            {lang === 'zh' ? roleMeta[role].dutyZh : roleMeta[role].dutyEn}
                          </div>
                        </div>
                        <span style={{
                          ...pillStyle,
                          color: finance.roleBindings[role].pupKey ? '#0E6A4C' : 'var(--color-text-tertiary)',
                          background: finance.roleBindings[role].pupKey ? 'rgba(29,158,117,0.10)' : 'var(--color-background-secondary)',
                          border: '0.5px solid var(--color-border-tertiary)',
                          fontFamily: 'var(--font-mono)',
                        }}>
                          {role}
                        </span>
                      </div>
                      <select
                        value={finance.roleBindings[role].pupKey ?? ''}
                        onChange={(e) => {
                          commitFinance({
                            ...finance,
                            roleBindings: {
                              ...finance.roleBindings,
                              [role]: { pupKey: e.target.value || null },
                            },
                          });
                        }}
                        style={selectStyle}
                      >
                        <option value="">{lang === 'zh' ? '未绑定' : 'Unbound'}</option>
                        {pups.map((pup) => (
                          <option key={pup.key} value={pup.key}>{pup.display_name} ({pup.key})</option>
                        ))}
                      </select>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div style={sectionShellStyle}>
              <button onClick={() => setExpandedSection(expanded === 'connectors' ? 'roles' : 'connectors')} style={sectionHeaderButtonStyle}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? '连接器绑定' : 'Connector Bindings'}</div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-secondary)' }}>
                      {connectorSummary}
                    </div>
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{expanded === 'connectors' ? '▾' : '▸'}</span>
                </div>
              </button>
              {expanded === 'connectors' && (
                <div style={{ padding: '0 13px 13px', display: 'grid', gap: 9 }}>
                  {CONNECTOR_KEYS.map((connector) => (
                    <div key={connector} style={{ ...miniCardStyle, display: 'grid', gap: 9, borderRadius: 10, padding: '11px 12px' }}>
                      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                        <div>
                          <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>{lang === 'zh' ? connectorMeta[connector].zh : connectorMeta[connector].en}</div>
                          <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                            {connectorMeta[connector].alias}
                          </div>
                        </div>
                        <span style={{
                          ...pillStyle,
                          color: finance.connectorBindings[connector].serverName ? '#0E6A4C' : 'var(--color-text-tertiary)',
                          background: finance.connectorBindings[connector].serverName ? 'rgba(29,158,117,0.10)' : 'var(--color-background-secondary)',
                          border: '0.5px solid var(--color-border-tertiary)',
                          fontFamily: 'var(--font-mono)',
                        }}>
                          {connector}
                        </span>
                      </div>
                      <select
                        value={finance.connectorBindings[connector].serverName ?? ''}
                        onChange={(e) => {
                          commitFinance({
                            ...finance,
                            connectorBindings: {
                              ...finance.connectorBindings,
                              [connector]: { mode: 'mcp_server', serverName: e.target.value || null },
                            },
                          });
                        }}
                        style={selectStyle}
                      >
                        <option value="">{lang === 'zh' ? '未绑定' : 'Unbound'}</option>
                        {servers.map((server) => (
                          <option key={server.name} value={server.name}>
                            {server.name}{server.enabled ? '' : ` ${lang === 'zh' ? '(停用)' : '(disabled)'}`} · {server.base_url}
                          </option>
                        ))}
                      </select>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </section>

          {error && (
            <div style={{ ...cardStyle, color: 'var(--color-text-danger)', background: 'var(--color-background-danger)' }}>
              {error}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
