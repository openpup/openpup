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

  if (!open) return null;

  return (
    <div style={overlayStyle} onClick={onClose}>
      <div style={sheetStyle} onClick={(e) => e.stopPropagation()}>
        <div style={{ padding: '20px 18px 18px', display: 'grid', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 }}>
            <div style={{ display: 'grid', gap: 8 }}>
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
                {lang === 'zh' ? 'Finance 配置' : 'Finance Config'}
              </span>
              <div style={{ fontSize: 24, fontWeight: 760, lineHeight: 1.1, color: 'var(--color-text-primary)' }}>
                {lang === 'zh' ? '金融场景控制台' : 'Finance Scenario Console'}
              </div>
              <div style={{ fontSize: 12, lineHeight: 1.7, color: 'var(--color-text-secondary)' }}>
                {lang === 'zh'
                  ? '这里专门处理角色绑定、技能预设与连接器路由。右侧 sidebar 现在只承担预览与上下文展示。'
                  : 'Use this sheet for bindings and routing. The right sidebar is now reserved for previews and context.'}
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

          <div style={{ ...cardStyle, background: 'linear-gradient(180deg, rgba(16,59,47,0.06), rgba(16,59,47,0.02))' }}>
            <div style={{ display: 'grid', gap: 8 }}>
              <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                {lang === 'zh' ? '当前路由' : 'Current Routing'}
              </div>
              <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.7 }}>
                {connectorSummary}
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                <span>{lang === 'zh' ? 'TradeIntent 契约' : 'TradeIntent contract'}</span>
                <span>•</span>
                <span>{lang === 'zh' ? '强制 leashed' : 'Leashed enforced'}</span>
                <span>•</span>
                <span>{lang === 'zh' ? '人工确认下单' : 'Manual order confirmation'}</span>
              </div>
            </div>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 10 }}>
            <div style={miniCardStyle}>
              <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {lang === 'zh' ? '专家绑定' : 'Experts'}
              </div>
              <div style={{ marginTop: 6, fontSize: 18, fontWeight: 760, color: 'var(--color-text-primary)' }}>{boundRoleCount}/5</div>
            </div>
            <div style={miniCardStyle}>
              <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {lang === 'zh' ? '技能覆写' : 'Overrides'}
              </div>
              <div style={{ marginTop: 6, fontSize: 18, fontWeight: 760, color: 'var(--color-text-primary)' }}>{customSkillCount}/5</div>
            </div>
            <div style={miniCardStyle}>
              <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {lang === 'zh' ? '连接器' : 'Connectors'}
              </div>
              <div style={{ marginTop: 6, fontSize: 18, fontWeight: 760, color: 'var(--color-text-primary)' }}>{boundConnectorCount}/3</div>
            </div>
          </div>

          {error && (
            <div style={{ ...cardStyle, color: 'var(--color-text-danger)', background: 'var(--color-background-danger)' }}>
              {error}
            </div>
          )}

          <section style={{ display: 'grid', gap: 10 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
              {lang === 'zh' ? '专家绑定' : 'Expert Bindings'}
            </div>
            {(Object.keys(roleMeta) as FinanceRoleKey[]).map((role) => (
              <div key={role} style={cardStyle}>
                <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                      {lang === 'zh' ? roleMeta[role].zh : roleMeta[role].en}
                    </div>
                    <div style={{ marginTop: 4, fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {lang === 'zh' ? roleMeta[role].dutyZh : roleMeta[role].dutyEn}
                    </div>
                  </div>
                  <span style={{
                    fontSize: 10,
                    color: finance.roleBindings[role].pupKey ? '#0E6A4C' : 'var(--color-text-tertiary)',
                    background: finance.roleBindings[role].pupKey ? 'rgba(29,158,117,0.10)' : 'var(--color-background-primary)',
                    borderRadius: 999,
                    padding: '3px 7px',
                    fontFamily: 'var(--font-mono)',
                  }}>{role}</span>
                </div>
                <div style={{ marginTop: 12 }}>
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
              </div>
            ))}
          </section>

          <section style={{ display: 'grid', gap: 10 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
              {lang === 'zh' ? '技能预设' : 'Skill Presets'}
            </div>
            {(Object.keys(skillMeta) as FinanceSkillKey[]).map((skill) => (
              <div key={skill} style={cardStyle}>
                <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                      {lang === 'zh' ? skillMeta[skill].zh : skillMeta[skill].en}
                    </div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                      {lang === 'zh' ? `触发词：${skillMeta[skill].triggerZh}` : `Trigger: ${skillMeta[skill].triggerEn}`}
                    </div>
                  </div>
                  <span style={{
                    fontSize: 10,
                    color: finance.skillBindings[skill].mode === 'installed_skill' ? '#0E6A4C' : 'var(--color-text-tertiary)',
                    background: finance.skillBindings[skill].mode === 'installed_skill' ? 'rgba(29,158,117,0.10)' : 'var(--color-background-primary)',
                    borderRadius: 999,
                    padding: '3px 7px',
                    fontFamily: 'var(--font-mono)',
                  }}>{finance.skillBindings[skill].mode === 'installed_skill' ? 'custom' : 'preset'}</span>
                </div>
                <div style={{ marginTop: 12 }}>
                  <select
                    value={finance.skillBindings[skill].mode === 'installed_skill' ? (finance.skillBindings[skill].skillName ?? '') : ''}
                    onChange={(e) => {
                      const skillName = e.target.value || null;
                      commitFinance({
                        ...finance,
                        skillBindings: {
                          ...finance.skillBindings,
                          [skill]: skillName
                            ? { mode: 'installed_skill', skillName }
                            : { mode: 'scenario_preset', skillName: null },
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
              </div>
            ))}
          </section>

          <section style={{ display: 'grid', gap: 10 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
              {lang === 'zh' ? '连接器绑定' : 'Connector Bindings'}
            </div>
            {(Object.keys(connectorMeta) as FinanceConnectorKey[]).map((connector) => (
              <div key={connector} style={cardStyle}>
                <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                      {lang === 'zh' ? connectorMeta[connector].zh : connectorMeta[connector].en}
                    </div>
                    <div style={{ marginTop: 4, fontSize: 11, color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                      {connectorMeta[connector].alias}
                    </div>
                  </div>
                  <span style={{
                    fontSize: 10,
                    color: finance.connectorBindings[connector].serverName ? '#0E6A4C' : 'var(--color-text-tertiary)',
                    background: finance.connectorBindings[connector].serverName ? 'rgba(29,158,117,0.10)' : 'var(--color-background-primary)',
                    borderRadius: 999,
                    padding: '3px 7px',
                    fontFamily: 'var(--font-mono)',
                  }}>{connector}</span>
                </div>
                <div style={{ marginTop: 12 }}>
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
              </div>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
};
