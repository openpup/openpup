import React, { useEffect, useRef, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MarkdownRenderer } from './components/MarkdownRenderer';
import { MessageActions } from './components/MessageActions';
import { Onboarding } from './components/Onboarding';
import { SkillClaw } from './components/SkillClaw';
import { MemoryManager } from './components/MemoryManager';
import { Timeline } from './components/Timeline';
import { DiaryViewer } from './components/DiaryViewer';
import { McpSettings } from './components/McpSettings';
import { PupManager } from './components/PupManager';
import { TaskManager } from './components/TaskManager';
import { PermissionDialog } from './components/PermissionDialog';
import { PackChannel } from './components/PackChannel';
import { BridgeSettings } from './components/BridgeSettings';
import { KnowledgeBase } from './components/KnowledgeBase';
import { KnowledgeSettings } from './components/KnowledgeSettings';
import { DesktopSettings } from './components/DesktopSettings';
import { GroupChat } from './components/GroupChat';
import { FinanceWorkbench } from './components/FinanceWorkbench';
import { usePackChannel } from './hooks/usePackChannel';
import { LangProvider, useLang, t } from './i18n';
import { buildPupMetaByKey, pupAccentColor, pupTagStyle } from './utils/pupVisuals';
import { useChatStore } from './stores/chatStore';
import { useUIStore } from './stores/uiStore';
import { useAppStore } from './stores/appStore';
import type { ChatMessage, StreamingPupState, ActivityStep, TokenUsage } from './stores/chatStore';
import type { NavItem } from './stores/uiStore';
import type { PupConfig, MemoryChip, ContextStats } from './stores/appStore';
import type { PermissionRequest } from './components/PermissionDialog';

interface StreamDonePayload {
  pup_key: string;
  pup_name: string;
  content: string;
}

const ACTIVITY_ICON: Record<string, string> = {
  routing:    '→',
  skill:      '⚡',
  shell:      '$',
  file_read:  '📄',
  file_write: '✏️',
  http:       '🌐',
  memory:     '🧠',
  task:       '✓',
  mcp:        '🔌',
  tool_call:  '⚙',
};

const ACTIVITY_COLOR: Record<string, string> = {
  routing:    'text-stone-400',
  skill:      'text-amber-400',
  shell:      'text-cyan-400 font-mono',
  file_read:  'text-sky-400',
  file_write: 'text-violet-400',
  http:       'text-teal-400',
  memory:     'text-purple-400',
  task:       'text-emerald-400',
  mcp:        'text-orange-400',
  tool_call:  'text-stone-400',
};

const GROUP_CHAT_FULL_STORAGE_KEY = 'openpup.groupChatFull';

function isGroupChatFullEnabled() {
  return import.meta.env.DEV ||
    import.meta.env.VITE_OPENPUP_GROUP_CHAT === '1' ||
    window.localStorage.getItem(GROUP_CHAT_FULL_STORAGE_KEY) === 'true';
}

const GroupChatPreview: React.FC = () => {
  const { lang } = useLang();
  const ready = [
    t('group_preview_ready_1', lang),
    t('group_preview_ready_2', lang),
    t('group_preview_ready_3', lang),
    t('group_preview_ready_4', lang),
  ];
  const next = [
    t('group_preview_next_1', lang),
    t('group_preview_next_2', lang),
    t('group_preview_next_3', lang),
    t('group_preview_next_4', lang),
  ];
  const sectionStyle: React.CSSProperties = {
    border: '0.5px solid var(--color-border-tertiary)',
    borderRadius: '8px',
    padding: '14px',
    background: 'var(--color-background-secondary)',
    minWidth: 0,
  };

  return (
    <div className="flex-1 overflow-auto" style={{ background: 'var(--color-background-primary)', color: 'var(--color-text-primary)' }}>
      <div style={{ maxWidth: '760px', margin: '0 auto', padding: '52px 28px', display: 'grid', gap: '18px' }}>
        <div style={{ display: 'grid', gap: '10px' }}>
          <span style={{ width: 'fit-content', fontSize: '11px', fontWeight: 650, color: '#BA7517', background: 'rgba(186,117,23,0.1)', border: '0.5px solid rgba(186,117,23,0.18)', borderRadius: '999px', padding: '3px 8px' }}>
            {t('group_preview_badge', lang)}
          </span>
          <h1 style={{ margin: 0, fontSize: '24px', lineHeight: 1.2, fontWeight: 760, letterSpacing: 0 }}>
            {t('group_preview_title', lang)}
          </h1>
          <p style={{ margin: 0, maxWidth: '640px', color: 'var(--color-text-secondary)', fontSize: '13px', lineHeight: 1.7 }}>
            {t('group_preview_body', lang)}
          </p>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: '12px' }}>
          <div style={sectionStyle}>
            <div style={{ fontSize: '13px', fontWeight: 700, marginBottom: '10px' }}>{t('group_preview_ready', lang)}</div>
            <ul style={{ margin: 0, paddingLeft: '18px', color: 'var(--color-text-secondary)', fontSize: '12px', lineHeight: 1.75 }}>
              {ready.map((item) => <li key={item}>{item}</li>)}
            </ul>
          </div>
          <div style={sectionStyle}>
            <div style={{ fontSize: '13px', fontWeight: 700, marginBottom: '10px' }}>{t('group_preview_next', lang)}</div>
            <ul style={{ margin: 0, paddingLeft: '18px', color: 'var(--color-text-secondary)', fontSize: '12px', lineHeight: 1.75 }}>
              {next.map((item) => <li key={item}>{item}</li>)}
            </ul>
          </div>
        </div>

        <div style={{ color: 'var(--color-text-tertiary)', fontSize: '12px', lineHeight: 1.6 }}>
          {t('group_preview_note', lang)}
        </div>
      </div>
    </div>
  );
};



// Types are now in stores/ — see stores/appStore.ts, stores/uiStore.ts, stores/chatStore.ts

// ─── LLM Config Panel ────────────────────────────────────────────────────────

interface LlmConfigInfo {
  provider: string;
  model: string;
  mini_model: string;
  embed_model: string;
  api_base: string | null;
}

interface ProviderPayload {
  id: string;
  name: string;
  provider: string;
  apiBase: string;
  apiKey: string;
  enabled: boolean;
  models: string[];
}

interface RouteTargetPayload {
  providerId: string;
  model: string;
}

interface RoutingPayload {
  primary: RouteTargetPayload;
  mini: RouteTargetPayload;
  embedding: RouteTargetPayload;
}

interface ProviderTestResult {
  ok: boolean;
  message: string;
  modelsFound: number;
}

interface ProviderCatalogItem {
  key: string;
  label: string;
  defaultApiBase: string;
  defaultName: string;
  defaultId: string;
}

const LlmConfigPanel: React.FC = () => {
  const { lang } = useLang();
  const [cfg, setCfg] = React.useState<LlmConfigInfo | null>(null);
  const [providers, setProviders] = React.useState<ProviderPayload[]>([]);
  const [routing, setRouting] = React.useState<RoutingPayload | null>(null);
  const [selectedProviderId, setSelectedProviderId] = React.useState<string | null>(null);
  const [providerForm, setProviderForm] = React.useState<ProviderPayload>({
    id: '',
    name: '',
    provider: 'openai_compatible',
    apiBase: 'https://api.openai.com/v1',
    apiKey: '',
    enabled: true,
    models: [],
  });
  const [modelsText, setModelsText] = React.useState('');
  const [savingProvider, setSavingProvider] = React.useState(false);
  const [savingRouting, setSavingRouting] = React.useState(false);
  const [testingProvider, setTestingProvider] = React.useState(false);
  const [syncingProviderModels, setSyncingProviderModels] = React.useState(false);
  const [deletingProvider, setDeletingProvider] = React.useState(false);
  const [msg, setMsg] = React.useState<string | null>(null);
  const [msgIsError, setMsgIsError] = React.useState(false);
  const [providerDrawerOpen, setProviderDrawerOpen] = React.useState(false);
  const [openProviderMenuKey, setOpenProviderMenuKey] = React.useState<keyof RoutingPayload | null>(null);
  const [openModelMenuKey, setOpenModelMenuKey] = React.useState<keyof RoutingPayload | null>(null);
  const [providerCatalog, setProviderCatalog] = React.useState<ProviderCatalogItem[]>([]);

  React.useEffect(() => {
    const loadAll = async () => {
      try {
        const [c, providerList, currentRouting, catalog] = await Promise.all([
          invoke<LlmConfigInfo>('get_llm_config'),
          invoke<ProviderPayload[]>('list_llm_providers'),
          invoke<RoutingPayload>('get_llm_routing'),
          invoke<ProviderCatalogItem[]>('list_llm_provider_catalog'),
        ]);
        setCfg(c);
        setProviders(providerList);
        setRouting(currentRouting);
        setProviderCatalog(catalog);
        if (providerList[0]) {
          setSelectedProviderId(providerList[0].id);
          setProviderForm({ ...providerList[0], apiKey: '' });
          setModelsText(providerList[0].models.join(', '));
        }
      } catch {}
    };
    void loadAll();
  }, []);

  const setProviderSelection = (value: string) => {
    const previousCatalog = providerCatalog.find((item) => item.key === providerForm.provider);
    const nextCatalog = providerCatalog.find((item) => item.key === value);
    setProviderForm((f) => ({
      ...f,
      provider: nextCatalog?.key ?? value,
      id:
        !f.id.trim() || f.id === previousCatalog?.defaultId
          ? (nextCatalog?.defaultId ?? f.id)
          : f.id,
      name:
        !f.name.trim() || f.name === previousCatalog?.defaultName
          ? (nextCatalog?.defaultName ?? f.name)
          : f.name,
      apiBase:
        !f.apiBase.trim() || f.apiBase === previousCatalog?.defaultApiBase
          ? (nextCatalog?.defaultApiBase ?? f.apiBase)
          : f.apiBase,
    }));
  };

  const syncCurrentProviderModels = async () => {
    if (syncingProviderModels) return;
    setSyncingProviderModels(true);
    try {
      const models = await invoke<string[]>('refresh_llm_provider_models', {
        providerId: providerForm.id.trim(),
        provider: {
          ...providerForm,
          models: modelsText.split(',').map((item) => item.trim()).filter(Boolean),
        },
      });
      setProviderForm((f) => ({ ...f, models }));
      setModelsText(models.join(', '));
      setProviders((list) => list.map((item) => item.id === providerForm.id ? { ...item, models } : item));
      setMsg(`${t('llm_provider_synced_prefix', lang)} ${models.length} ${t('llm_provider_synced_suffix', lang)}`);
      setMsgIsError(false);
    } catch (e) {
      setMsg(String(e));
      setMsgIsError(true);
    } finally {
      setSyncingProviderModels(false);
    }
  };

  const testCurrentProvider = async () => {
    if (testingProvider) return;
    setTestingProvider(true);
    try {
      const result = await invoke<ProviderTestResult>('test_llm_provider', {
        provider: { ...providerForm, models: modelsText.split(',').map((item) => item.trim()).filter(Boolean) },
      });
      setMsg(result.message);
      setMsgIsError(!result.ok);
    } catch (e) {
      setMsg(String(e));
      setMsgIsError(true);
    } finally {
      setTestingProvider(false);
    }
  };

  const deleteCurrentProvider = async () => {
    if (!selectedProviderId || deletingProvider) return;
    if (!window.confirm(t('llm_provider_delete_confirm', lang))) return;
    setDeletingProvider(true);
    try {
      await invoke('delete_llm_provider', { providerId: selectedProviderId });
      const nextProviders = await invoke<ProviderPayload[]>('list_llm_providers');
      setProviders(nextProviders);
      const next = nextProviders[0];
      setSelectedProviderId(next?.id ?? null);
      if (next) {
        setProviderForm({ ...next, apiKey: '' });
        setModelsText(next.models.join(', '));
      } else {
        setProviderForm({ id: '', name: '', provider: 'openai_compatible', apiBase: 'https://api.openai.com/v1', apiKey: '', enabled: true, models: [] });
        setModelsText('');
      }
    } catch (e) {
      setMsg(String(e));
      setMsgIsError(true);
    } finally {
      setDeletingProvider(false);
    }
  };

  const saveProvider = async () => {
    setSavingProvider(true);
    setMsg(null);
    try {
      const payload = {
        ...providerForm,
        models: modelsText.split(',').map((item) => item.trim()).filter(Boolean),
      };
      await invoke('save_llm_provider', { provider: payload });
      const nextProviders = await invoke<ProviderPayload[]>('list_llm_providers');
      setProviders(nextProviders);
      setSelectedProviderId(payload.id);
      setProviderForm((f) => ({ ...f, apiKey: '' }));
      setMsg(t('llm_provider_saved', lang));
      setMsgIsError(false);
      setTimeout(() => setMsg(null), 2000);
    } catch (e) {
      setMsg(`${t('llm_provider_save_failed', lang)}: ${e}`);
      setMsgIsError(true);
    } finally {
      setSavingProvider(false);
    }
  };

  const saveRouting = async () => {
    if (!routing) return;
    setSavingRouting(true);
    setMsg(null);
    try {
      await invoke('set_llm_routing', { routing });
      const latest = await invoke<LlmConfigInfo>('get_llm_config');
      setCfg(latest);
      setMsg(t('llm_routing_saved', lang));
      setMsgIsError(false);
      setTimeout(() => setMsg(null), 2000);
    } catch (e) {
      setMsg(`${t('llm_routing_save_failed', lang)}: ${e}`);
      setMsgIsError(true);
    } finally {
      setSavingRouting(false);
    }
  };

  const selectProvider = (provider: ProviderPayload) => {
    setSelectedProviderId(provider.id);
    setProviderForm({ ...provider, apiKey: '' });
    setModelsText(provider.models.join(', '));
    setProviderDrawerOpen(true);
  };

  const enabledProviderCount = providers.filter((provider) => provider.enabled).length;
  const configuredRouteCount = routing
    ? [routing.primary, routing.mini, routing.embedding].filter((route) => route.providerId && route.model).length
    : 0;

  const statCard = (label: string, value: string | number, tone: 'default' | 'accent' = 'default') => (
    <div className="flex items-baseline gap-2">
      <span style={{ fontSize: '12px', fontWeight: 650, color: tone === 'accent' ? '#BA7517' : 'var(--color-text-primary)' }}>{value}</span>
      <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', whiteSpace: 'nowrap' }}>{label}</span>
    </div>
  );

  const routeLabel = (key: keyof RoutingPayload) => {
    if (key === 'primary') return t('llm_main_model', lang);
    if (key === 'mini') return t('llm_mini_model', lang);
    return t('llm_embed_model', lang);
  };

  const field = (label: string, value: string, onChange: (value: string) => void, ph?: string, type = 'text') => (
    <div className="space-y-1.5">
      <label style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>{label}</label>
      <input
        type={type}
        className="flex-1 focus:outline-none transition-colors"
        style={{ width: '100%', fontSize: "13px", padding: '9px 10px', borderRadius: '10px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)' }}
        value={value}
        placeholder={ph}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );

  return (
    <section>
      <h2 style={{ fontSize: '14px', lineHeight: 1.2, fontWeight: 500, color: 'var(--color-text-primary)', margin: '0 0 14px 0' }}>
        {t('llm_config_title', lang)}
      </h2>

      <div className="grid gap-4 xl:grid-cols-[minmax(240px,0.9fr)_minmax(0,1.1fr)]">
        <div className="space-y-2">
            <div className="flex items-start gap-2">
              <div className="flex min-w-0 flex-1 flex-wrap gap-2">
              {providers.length === 0 && (
                <div style={{ color: 'var(--color-text-tertiary)', fontSize: '12px', padding: '6px 0' }}>
                  {t('llm_provider_empty', lang)}
                </div>
              )}
              {providers.map((provider) => (
                <button
                  key={provider.id}
                  onClick={() => selectProvider(provider)}
                  className="text-left transition-colors"
                  style={{
                    maxWidth: '100%',
                    border: '0.5px solid var(--color-border-secondary)',
                    background: provider.enabled
                      ? 'var(--color-background-primary)'
                      : 'var(--color-background-secondary)',
                    padding: '3px 10px',
                    borderRadius: '20px',
                  }}
                >
                  <div className="flex items-center">
                    <span
                      className="truncate"
                      style={{
                        maxWidth: '220px',
                        fontSize: '12px',
                        fontWeight: 400,
                        color: provider.enabled ? 'var(--color-text-secondary)' : 'var(--color-text-tertiary)',
                      }}
                    >
                      {provider.name}
                    </span>
                  </div>
                </button>
              ))}
              </div>
              <button
                onClick={() => {
                  setSelectedProviderId(null);
                  setProviderForm({ id: '', name: '', provider: 'openai_compatible', apiBase: 'https://api.openai.com/v1', apiKey: '', enabled: true, models: [] });
                  setModelsText('');
                  setProviderDrawerOpen(true);
                }}
                style={{
                  border: '0.5px dashed var(--color-border-secondary)',
                  borderRadius: '20px',
                  background: 'transparent',
                  padding: '3px 10px',
                  fontSize: '12px',
                  color: 'var(--color-text-secondary)',
                  flex: '0 0 auto',
                }}
              >
                {t('llm_provider_new', lang)}
              </button>
            </div>
        </div>

        <div className="space-y-2">
          {routing && (
            <div className="space-y-2 pt-1">
              {(['primary', 'mini', 'embedding'] as Array<keyof RoutingPayload>).map((key) => {
                const selectedProvider = providers.find((provider) => provider.id === routing[key].providerId);
                return (
                  <div
                    key={key}
                    className="grid gap-2 md:grid-cols-[112px_minmax(0,1fr)] md:items-center"
                  >
                    <div style={{ fontSize: '12px', fontWeight: 400, color: 'var(--color-text-tertiary)' }}>{routeLabel(key)}</div>
                    <div
                      className="flex flex-col md:flex-row"
                      style={{
                        border: '0.5px solid var(--color-border-secondary)',
                        borderRadius: '18px',
                        minHeight: '40px',
                        overflow: 'visible',
                      }}
                      >
                        <div
                          className="relative"
                          style={{
                            width: '188px',
                            borderRight: '0.5px solid var(--color-border-secondary)',
                            background: 'var(--color-background-primary)',
                            borderTopLeftRadius: '18px',
                            borderBottomLeftRadius: '18px',
                            overflow: 'visible',
                          }}
                        >
                          <button
                            type="button"
                            onClick={() => {
                              setOpenProviderMenuKey((current) => current === key ? null : key);
                              setOpenModelMenuKey(null);
                            }}
                            className="h-full w-full focus:outline-none"
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'center',
                              padding: '0 24px',
                              fontSize: '12px',
                              border: 'none',
                              background: 'transparent',
                              color: 'var(--color-text-primary)',
                              whiteSpace: 'nowrap',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              textAlign: 'center',
                              minHeight: '40px',
                            }}
                          >
                            {selectedProvider?.name || ''}
                          </button>
                          <span
                            style={{
                              position: 'absolute',
                              right: '8px',
                              top: '50%',
                              transform: 'translateY(-50%)',
                              fontSize: '8px',
                              color: 'var(--color-text-tertiary)',
                              pointerEvents: 'none',
                            }}
                          >
                            ▾
                          </span>
                          {openProviderMenuKey === key && providers.length > 0 && (
                            <div
                              style={{
                                position: 'absolute',
                                left: '0',
                                right: '0',
                                top: 'calc(100% + 6px)',
                                zIndex: 25,
                                maxHeight: '180px',
                                overflowY: 'auto',
                                border: '0.5px solid var(--color-border-secondary)',
                                borderRadius: '12px',
                                background: 'var(--color-background-primary)',
                                boxShadow: '0 10px 30px rgba(15, 23, 42, 0.12)',
                              }}
                            >
                              {providers.map((provider, index) => (
                                <button
                                  key={provider.id}
                                  type="button"
                                  onMouseDown={(e) => {
                                    e.preventDefault();
                                    setRouting((prev) => prev ? ({ ...prev, [key]: { ...prev[key], providerId: provider.id } }) : prev);
                                    setOpenProviderMenuKey(null);
                                  }}
                                  className="w-full text-center"
                                  style={{
                                    display: 'block',
                                    padding: '8px 12px',
                                    fontSize: '12px',
                                    border: 'none',
                                    background: 'transparent',
                                    color: 'var(--color-text-primary)',
                                    borderBottom: index === providers.length - 1 ? 'none' : '0.5px solid var(--color-border-secondary)',
                                  }}
                                >
                                  {provider.name}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      <div
                        className="min-w-0 flex-1"
                        style={{
                          background: 'var(--color-background-secondary)',
                          borderTopRightRadius: '18px',
                          borderBottomRightRadius: '18px',
                          overflow: 'visible',
                        }}
                      >
                        <div className="relative h-full">
                          <input
                            value={routing[key].model}
                            onChange={(e) => setRouting((prev) => prev ? ({ ...prev, [key]: { ...prev[key], model: e.target.value } }) : prev)}
                            className="h-full w-full focus:outline-none"
                            autoComplete="off"
                            onFocus={() => {
                              setOpenModelMenuKey(key);
                              setOpenProviderMenuKey(null);
                            }}
                            onClick={() => {
                              setOpenModelMenuKey(key);
                              setOpenProviderMenuKey(null);
                            }}
                            onBlur={() => {
                              window.setTimeout(() => {
                                setOpenModelMenuKey((current) => current === key ? null : current);
                              }, 120);
                            }}
                            style={{ fontSize: '12px', padding: '0 12px', border: 'none', background: 'transparent', color: 'var(--color-text-primary)' }}
                          />
                          {openModelMenuKey === key && (selectedProvider?.models?.length ?? 0) > 0 && (
                            <div
                              style={{
                                position: 'absolute',
                                left: '0',
                                right: '0',
                                top: 'calc(100% + 6px)',
                                zIndex: 20,
                                maxHeight: '180px',
                                overflowY: 'auto',
                                border: '0.5px solid var(--color-border-secondary)',
                                borderRadius: '12px',
                                background: 'var(--color-background-primary)',
                                boxShadow: '0 10px 30px rgba(15, 23, 42, 0.12)',
                              }}
                            >
                              {selectedProvider?.models.map((model) => (
                                <button
                                  key={model}
                                  type="button"
                                  onMouseDown={(e) => {
                                    e.preventDefault();
                                    setRouting((prev) => prev ? ({ ...prev, [key]: { ...prev[key], model } }) : prev);
                                    setOpenModelMenuKey(null);
                                  }}
                                  className="w-full text-left"
                                  style={{
                                    display: 'block',
                                    padding: '8px 12px',
                                    fontSize: '12px',
                                    border: 'none',
                                    background: 'transparent',
                                    color: 'var(--color-text-primary)',
                                    borderBottom: model === selectedProvider.models[selectedProvider.models.length - 1] ? 'none' : '0.5px solid var(--color-border-secondary)',
                                  }}
                                >
                                  {model}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
          <div className="mt-3 space-y-1 leading-relaxed" style={{ fontSize: "11px", color: 'var(--color-text-tertiary)' }}>
            <div>{t('llm_hint_mini', lang)}</div>
            <div>{t('llm_hint_embed', lang)}</div>
          </div>
          <div className="flex items-center gap-3 pt-1">
            <button
              onClick={() => void saveRouting()}
              disabled={savingRouting || !routing}
              style={{ padding: '7px 11px', borderRadius: '10px', border: 'none', background: 'var(--color-text-primary)', color: 'var(--color-background-primary)', fontSize: "13px", cursor: savingRouting ? 'not-allowed' : 'pointer', opacity: savingRouting ? 0.5 : 1 }}
            >
              {t('llm_save', lang)}
            </button>
            <div className="ml-auto flex flex-wrap items-center gap-x-5 gap-y-1">
              {statCard(t('llm_provider_count', lang), providers.length, 'accent')}
              {statCard(t('llm_provider_ready_count', lang), enabledProviderCount)}
              {statCard(t('llm_route_count', lang), configuredRouteCount)}
              {cfg && <span style={{ fontSize: "11px", color: 'var(--color-text-tertiary)' }}>{t('llm_current', lang)} {cfg.model}</span>}
            </div>
          </div>
        </div>
      </div>

      {providerDrawerOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-5"
          style={{ background: 'rgba(17, 24, 39, 0.28)' }}
          onClick={() => setProviderDrawerOpen(false)}
        >
          <div
            className="w-full max-w-[640px] overflow-auto"
            style={{
              maxHeight: 'min(760px, calc(100vh - 40px))',
              border: '0.5px solid var(--color-border-primary)',
              borderRadius: '16px',
              background: 'var(--color-background-primary)',
              boxShadow: '0 18px 48px rgba(15, 23, 42, 0.16)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              className="flex items-center justify-between px-4 py-3"
              style={{ borderBottom: '0.5px solid var(--color-border-tertiary)' }}
            >
              <div>
                <h3 style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)', margin: 0 }}>{t('llm_provider_editor', lang)}</h3>
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginTop: '4px' }}>
                  {selectedProviderId || providerForm.id ? `${t('llm_provider_active', lang)} · ${providerForm.name || providerForm.id || 'Provider'}` : t('llm_provider_editor_empty', lang)}
                </div>
              </div>
              <button onClick={() => setProviderDrawerOpen(false)} style={{ fontSize: '12px', color: 'var(--color-text-secondary)' }}>
                {t('llm_provider_close', lang)}
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="space-y-1.5">
                  {field(t('llm_provider_id', lang), providerForm.id, (value) => setProviderForm((f) => ({ ...f, id: value })), 'openrouter-main')}
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_id_hint', lang)}</div>
                </div>
                <div className="space-y-1.5">
                  {field(t('llm_provider_name', lang), providerForm.name, (value) => setProviderForm((f) => ({ ...f, name: value })), 'OpenRouter Main')}
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_name_hint', lang)}</div>
                </div>
                <div className="space-y-1.5 md:col-span-2">
                  <label style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_vendor', lang)}</label>
                  <select
                    value={providerForm.provider}
                    onChange={(e) => setProviderSelection(e.target.value)}
                    className="w-full focus:outline-none transition-colors"
                    style={{ width: '100%', height: '37px', fontSize: "13px", padding: '0 10px', borderRadius: '10px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)' }}
                  >
                    {!providerCatalog.some((item) => item.key === providerForm.provider) && providerForm.provider && (
                      <option value={providerForm.provider}>{providerForm.provider}</option>
                    )}
                    {providerCatalog.map((option) => (
                      <option key={option.key} value={option.key}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_vendor_hint', lang)}</div>
                </div>
              </div>

              <div className="grid gap-3 md:grid-cols-2">
                {field(t('llm_provider_base', lang), providerForm.apiBase, (value) => setProviderForm((f) => ({ ...f, apiBase: value })), 'https://api.openai.com/v1')}
                {field(t('llm_provider_secret', lang), providerForm.apiKey, (value) => setProviderForm((f) => ({ ...f, apiKey: value })), providerForm.provider === 'ollama' ? t('llm_provider_secret_optional', lang) : 'sk-...', 'password')}
              </div>

              <div className="space-y-1.5">
                <label style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_models', lang)}</label>
                <textarea
                  value={modelsText}
                  onChange={(e) => setModelsText(e.target.value)}
                  rows={4}
                  className="w-full focus:outline-none"
                  style={{ fontSize: '13px', padding: '10px 12px', borderRadius: '12px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)' }}
                />
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{t('llm_provider_models_hint', lang)}</div>
              </div>

              <div className="flex flex-wrap items-center gap-3 pt-1">
                <label className="flex items-center gap-2" style={{ fontSize: '12px', color: 'var(--color-text-secondary)' }}>
                  <input
                    type="checkbox"
                    checked={providerForm.enabled}
                    onChange={(e) => setProviderForm((f) => ({ ...f, enabled: e.target.checked }))}
                  />
                  {t('llm_provider_enabled', lang)}
                </label>
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => void testCurrentProvider()}
                    disabled={!providerForm.provider.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider}
                    style={{
                      padding: '7px 10px',
                      borderRadius: '10px',
                      border: '0.5px solid var(--color-border-secondary)',
                      background: 'var(--color-background-secondary)',
                      color: 'var(--color-text-secondary)',
                      fontSize: '12px',
                      cursor: (!providerForm.provider.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 'not-allowed' : 'pointer',
                      opacity: (!providerForm.provider.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 0.5 : 1,
                    }}
                  >
                    {testingProvider ? t('llm_provider_testing', lang) : t('llm_provider_test', lang)}
                  </button>
                  <button
                    onClick={() => void syncCurrentProviderModels()}
                    disabled={!providerForm.id.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider}
                    style={{
                      padding: '7px 10px',
                      borderRadius: '10px',
                      border: '0.5px solid var(--color-border-secondary)',
                      background: 'var(--color-background-secondary)',
                      color: 'var(--color-text-secondary)',
                      fontSize: '12px',
                      cursor: (!providerForm.id.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 'not-allowed' : 'pointer',
                      opacity: (!providerForm.id.trim() || testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 0.5 : 1,
                    }}
                  >
                    {syncingProviderModels ? t('llm_provider_refreshing', lang) : t('llm_provider_refresh', lang)}
                  </button>
                  {selectedProviderId && (
                    <button
                      onClick={() => void deleteCurrentProvider()}
                      disabled={testingProvider || syncingProviderModels || deletingProvider || savingProvider}
                      style={{
                        padding: '7px 10px',
                        borderRadius: '10px',
                        border: '0.5px solid color-mix(in srgb, var(--color-text-danger) 24%, transparent)',
                        background: 'transparent',
                        color: 'var(--color-text-danger)',
                        fontSize: '12px',
                        cursor: (testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 'not-allowed' : 'pointer',
                        opacity: (testingProvider || syncingProviderModels || deletingProvider || savingProvider) ? 0.5 : 1,
                      }}
                    >
                      {deletingProvider ? t('llm_provider_deleting', lang) : t('llm_provider_delete', lang)}
                    </button>
                  )}
                  <button
                    onClick={() => void saveProvider()}
                    disabled={savingProvider || testingProvider || syncingProviderModels || deletingProvider}
                    style={{
                      padding: '7px 12px',
                      borderRadius: '10px',
                      border: 'none',
                      background: 'var(--color-text-primary)',
                      color: 'var(--color-background-primary)',
                      fontSize: "13px",
                      cursor: (savingProvider || testingProvider || syncingProviderModels || deletingProvider) ? 'not-allowed' : 'pointer',
                      opacity: (savingProvider || testingProvider || syncingProviderModels || deletingProvider) ? 0.5 : 1,
                      fontWeight: 600,
                      minWidth: '68px',
                    }}
                  >
                    {t('llm_provider_save', lang)}
                  </button>
                </div>
              </div>

              {msg && (
                <div
                  style={{
                    fontSize: '12px',
                    color: msgIsError ? 'var(--color-text-danger)' : 'var(--color-text-success)',
                  }}
                >
                  {msg}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </section>
  );
};

// ─────────────────────────────────────────────────────────────────────────────

function memoryTypeIcon(type: string): string {
  if (type === 'preference') return '❤️';
  if (type === 'boundary' || type === 'restriction') return '❌';
  if (type === 'fact') return '📋';
  if (type === 'event' || type === 'recent') return '🔼';
  return '📌';
}

// Inner app that uses lang context
type HostPlatform = 'macos' | 'windows' | 'linux' | 'android' | 'ios';

/** Detect the host platform once — used for titlebar layout. */
const detectPlatform = (): HostPlatform => {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('android')) return 'android';
  if (ua.includes('iphone') || ua.includes('ipad') || ua.includes('ipod')) return 'ios';
  if (ua.includes('macintosh') || ua.includes('mac os')) return 'macos';
  if (ua.includes('windows')) return 'windows';
  return 'linux';
};

const isMobilePlatform = (p: HostPlatform) => p === 'android' || p === 'ios';

const AppInner: React.FC = () => {
  const { lang, setLang } = useLang();
  const platform = useMemo(detectPlatform, []);
  const isMobile = isMobilePlatform(platform);
  const showDesktopControls = platform === 'windows' || platform === 'linux';
  const showDesktopTitlebar = !isMobile;
  const [groupChatFullEnabled] = useState(isGroupChatFullEnabled);

  // ── Zustand stores ──────────────────────────────────────────────────────────
  const {
    messages, setMessages, streamingContent, setStreamingContent,
    streamingReasoningContent, setStreamingReasoningContent,
    streamingPup: streamingPupName, setStreamingPup: setStreamingPupName,
    streamingSteps, setStreamingSteps, input, setInput, sending, setSending,
    tokenUsage, setTokenUsage, resetStreaming,
  } = useChatStore();

  const {
    theme, setTheme, activeNav, setActiveNav, channelDetailMode, setChannelDetailMode,
    sidebarCollapsed, setSidebarCollapsed, membersExpanded, setMembersExpanded,
    toolsExpanded, setToolsExpanded, configExpanded, setConfigExpanded,
    isMaximized, setIsMaximized, selectedPupKey, setSelectedPupKey,
    memoriesTab, setMemoriesTab,
  } = useUIStore();

  const {
    onboardingDone, setOnboardingDone, pups, setPups,
    memoryChips, setMemoryChips, kbSourceCount, setKbSourceCount,
    permissionRequest, setPermissionRequest,
    contextStats, setContextStats, execMode, setExecMode,
    exporting, setExporting, importing, setImporting,
    importPath, setImportPath, settingsMsg, setSettingsMsg,
    settingsErr, setSettingsErr,
  } = useAppStore();
  const activeNavRef = useRef<string>('chat');
  useEffect(() => { activeNavRef.current = activeNav; }, [activeNav]);
  // Auto-expand sidebar section when an item inside it becomes active
  useEffect(() => {
    const toolsItems: NavItem[] = ['finance', 'memories', 'timeline', 'tasks', 'skills', 'knowledge'];
    const configItems: NavItem[] = ['pups', 'mcp', 'bridge', 'settings'];
    if (toolsItems.includes(activeNav)) setToolsExpanded(true);
    if (configItems.includes(activeNav)) setConfigExpanded(true);
  }, [activeNav]);
  // pups, memoryChips, permissionRequest, kbSourceCount, selectedPupKey, memoriesTab → appStore/uiStore
  const {
    channels,
    activeChannelId,
    messages: channelMessages,
    plan: channelPlan,
    workflow: channelWorkflow,
    isCompleted: channelCompleted,
    completionEventCount,
    activeCount: activeChannelCount,
    error: channelError,
    setActiveChannelId,
    clearCompletedChannels,
    clearStaleChannels,
    continueChannel,
    requestChannelChanges,
    submitChannelReviewComment,
    abortChannel,
  } = usePackChannel();

  useEffect(() => {
    if (channelPlan?.channel_id) {
      setActiveNav('channel');
      setChannelDetailMode(true);
    }
  }, [channelPlan]);

  useEffect(() => {
    if (activeNav === 'channel' && activeChannelId && channelMessages.length > 0) {
      setChannelDetailMode(true);
    }
  }, [activeNav, activeChannelId, channelMessages.length]);

  useEffect(() => {
    if (completionEventCount === 0) return;
    if (activeNavRef.current !== 'channel') return;
    setChannelDetailMode(false);
    setActiveNav('chat');
  }, [completionEventCount]);
  // contextStats, tokenUsage, settings state, layout state → stores
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  /** True while CJK IME composition is active; `isComposing` alone misses some Enter-to-commit keydowns. */
  const imeComposingRef = useRef(false);
  // Debounce token-by-token scroll during streaming
  const scrollTimer = useRef<ReturnType<typeof setTimeout>>();
  // Track which message IDs have already been animated so history doesn't re-animate
  const animatedMsgIds = useRef(new Set<string>());

  const insertMention = (pupKey: string) => {
    const mention = `@${pupKey} `;
    setInput((prev) => (prev === '' || prev.endsWith(' ') ? prev + mention : `${prev} ${mention}`));
    inputRef.current?.focus();
  };
  // Ref that always holds the latest accumulated streaming content, so the
  // stream_done handler can read it without stale-closure issues.
  const streamingContentRef = useRef('');

  const loadPups = () => {
    invoke<PupConfig[]>('list_pups')
      .then((list) => setPups(list.filter((p) => p.enabled)))
      .catch(() => {});
  };

  React.useEffect(() => {
    localStorage.setItem('openpup_theme', theme);
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);

  // Track window maximized state for custom titlebar button icon (Windows & Linux)
  useEffect(() => {
    if (!showDesktopControls) return;
    const win = getCurrentWindow();
    win.isMaximized().then(setIsMaximized).catch(() => {});
    const unlisten = win.onResized(() => {
      win.isMaximized().then(setIsMaximized).catch(() => {});
    });
    return () => { unlisten.then((f) => f()); };
  }, [showDesktopControls, setIsMaximized]);

  // Apply theme on first render
  React.useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    invoke<string>('get_execution_mode').then((m) => setExecMode(m as 'leashed' | 'free_run')).catch(() => {});
    loadPups();
    invoke<{ id: string }[]>('kb_list_sources').then((s) => setKbSourceCount(s.length)).catch(() => {});

    invoke<boolean>('check_onboarding_completed')
      .then((done) => {
        setOnboardingDone(done);
        if (done) {
          loadMemoryChips();
          setMessages([{ id: 'welcome', role: 'assistant', content: t('chat_welcome', lang), pup_key: 'alpha', pup_name: 'Alpha' }]);
          // Load initial context stats + token usage
          void invoke<ContextStats>('get_context_stats', { pupKey: 'alpha' })
            .then((stats) => setContextStats(stats))
            .catch((e) => {
              console.warn('get_context_stats failed on init:', e);
              setContextStats(null);
            });
          void invoke<TokenUsage>('get_token_usage')
            .then((usage) => setTokenUsage(usage))
            .catch(() => {});
        }
      })
      .catch(() => setOnboardingDone(false));

    // React StrictMode runs effects twice (mount→cleanup→remount).
    // Use a `cancelled` flag so that if cleanup fires before the async listeners resolve,
    // the resolved listeners are immediately unregistered instead of leaking.
    let cancelled = false;
    const cleanupFns: Array<() => void> = [];

    void Promise.all([
      listen<PermissionRequest>('permission_request', (e) => setPermissionRequest(e.payload)),
      listen<ActivityStep>('stream_activity', (e) => {
        setStreamingSteps((prev) => [...prev, e.payload]);
      }),
      listen<string>('stream_token', (e) => {
        streamingContentRef.current += e.payload;
        setStreamingContent((p) => p + e.payload);
      }),
      listen<string>('stream_reasoning_token', (e) => {
        // Reasoning tokens shown separately; not saved to history.
        setStreamingReasoningContent((p) => p + e.payload);
      }),
      listen<StreamDonePayload>('stream_done', (e) => {
        const { pup_key, pup_name, content } = e.payload;
        // Use backend authoritative content — immune to token ordering issues.
        streamingContentRef.current = '';
        setStreamingContent('');
        setStreamingReasoningContent('');
        setStreamingPupName(null);
        setStreamingSteps([]);
        setSending(false);
        if (content) {
          setMessages((msgs) => [
            ...msgs,
            { id: crypto.randomUUID(), role: 'assistant', content, pup_key, pup_name },
          ]);
          void loadMemoryChips();
          // Load context stats after message completes
          void invoke<ContextStats>('get_context_stats', { pupKey: selectedPupKey })
            .then((stats) => {
              setContextStats(stats);
            })
            .catch((e) => {
              console.warn('get_context_stats failed after stream_done:', e);
            });
          // Load token usage
          void invoke<TokenUsage>('get_token_usage')
            .then((usage) => setTokenUsage(usage))
            .catch(() => {});
        }
      }),
      listen<string>('stream_error', (e) => {
        setMessages((msgs) => [
          ...msgs,
          { id: crypto.randomUUID(), role: 'assistant', content: `⚠️ ${e.payload}`, pup_key: 'alpha', pup_name: 'Alpha' },
        ]);
        setStreamingContent('');
        setStreamingReasoningContent('');
        setStreamingPupName(null);
        setStreamingSteps([]);
        setSending(false);
      }),
    ]).then((fns) => {
      if (cancelled) {
        // Cleanup already ran before listeners resolved — unregister them immediately.
        fns.forEach((f) => f());
      } else {
        cleanupFns.push(...fns);
      }
    });

    return () => {
      cancelled = true;
      cleanupFns.forEach((f) => f());
    };
  }, []);

  // Keep title-bar context stats in sync with current pup selection.
  useEffect(() => {
    if (!onboardingDone) return;
    void invoke<ContextStats>('get_context_stats', { pupKey: selectedPupKey })
      .then((stats) => setContextStats(stats))
      .catch((e) => {
        console.warn('get_context_stats failed on pup switch:', e);
      });
  }, [onboardingDone, selectedPupKey]);

  // New message added → smooth scroll
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [messages]);

  // Switching back to chat should always restore the latest message in view.
  useEffect(() => {
    if (activeNav !== 'chat') return;
    let secondFrame = 0;
    const firstFrame = requestAnimationFrame(() => {
      secondFrame = requestAnimationFrame(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: 'instant', block: 'end' });
      });
    });
    return () => {
      cancelAnimationFrame(firstFrame);
      if (secondFrame) cancelAnimationFrame(secondFrame);
    };
  }, [activeNav]);

  // Streaming tokens → debounced instant scroll (avoids a reflow per token)
  useEffect(() => {
    if (!streamingContent) return;
    clearTimeout(scrollTimer.current);
    scrollTimer.current = setTimeout(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: 'instant', block: 'end' });
    }, 80);
    return () => clearTimeout(scrollTimer.current);
  }, [streamingContent]);

  const loadMemoryChips = async () => {
    try { setMemoryChips(await invoke<MemoryChip[]>('get_top_memories', { limit: 5 })); } catch {}
  };

  const handleApprove = async (remember: boolean) => {
    if (!permissionRequest) return;
    const req = permissionRequest;
    setPermissionRequest(null);
    await invoke('approve_permission', { requestId: req.request_id, skillName: req.skill_name, remember }).catch(() => {});
  };

  const handleDeny = async () => {
    if (!permissionRequest) return;
    const id = permissionRequest.request_id;
    setPermissionRequest(null);
    await invoke('deny_permission', { requestId: id }).catch(() => {});
  };

  const send = async () => {
    const trimmed = input.trim();
    if (!trimmed || sending) return;
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'user', content: trimmed }]);
    setInput('');
    if (inputRef.current) { inputRef.current.style.height = 'auto'; }
    streamingContentRef.current = '';
    setStreamingContent('');
    setStreamingReasoningContent('');
    setStreamingSteps([]);
    setStreamingPupName({ key: 'alpha', name: 'Alpha' });
    setSending(true);
    const forcedPup = selectedPupKey !== 'alpha' ? selectedPupKey : null;
    await invoke('send_message', { input: trimmed, forcedPup }).catch((e: unknown) => {
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'assistant', content: `⚠️ ${String(e)}`, pup_key: 'alpha', pup_name: 'Alpha' }]);
      setStreamingContent('');
      setStreamingReasoningContent('');
      setStreamingPupName(null);
      setSending(false);
    });
  };

  const abort = async () => {
    // Reset UI immediately — don't wait for the backend stream_done event.
    streamingContentRef.current = '';
    setStreamingContent('');
    setStreamingReasoningContent('');
    setStreamingPupName(null);
    setSending(false);
    await invoke('abort_message').catch(() => {});
  };

  const onKeyDown: React.KeyboardEventHandler<HTMLTextAreaElement> = (e) => {
    if (e.key !== 'Enter' || e.shiftKey) return;
    const ne = e.nativeEvent;
    if (imeComposingRef.current || ne.isComposing || ne.keyCode === 229) return;
    e.preventDefault();
    void send();
  };

  const exportWorkspace = async () => {
    setExporting(true); setSettingsErr(null); setSettingsMsg(null);
    try { setSettingsMsg(`${t('settings_export_success_prefix', lang)} ${await invoke<string>('export_workspace')}`); }
    catch (e: unknown) { setSettingsErr(String(e)); }
    finally { setExporting(false); }
  };

  const importWorkspace = async () => {
    if (!importPath.trim()) { setSettingsErr(t('settings_path_required', lang)); return; }
    setImporting(true); setSettingsErr(null); setSettingsMsg(null);
    try { await invoke('import_workspace', { backupPath: importPath.trim() }); setSettingsMsg(t('settings_import_success', lang)); }
    catch (e: unknown) { setSettingsErr(String(e)); }
    finally { setImporting(false); }
  };

  // Minimal titlebar rendered for loading / onboarding screens so the window
  // remains draggable and closeable (especially on Windows where native
  // decorations are disabled).
  const minimalTitleBar = showDesktopTitlebar ? (
    <div
      data-tauri-drag-region
      style={{
        height: '35px',
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        paddingLeft: platform === 'macos' ? '78px' : '12px',
        paddingRight: platform === 'macos' ? '12px' : '0px',
        background: 'var(--color-background-secondary)',
        borderBottom: '0.5px solid var(--color-border-tertiary)',
        userSelect: 'none',
      } as React.CSSProperties}
    >
      <span data-tauri-drag-region style={{ fontSize: '13px', fontWeight: 500, color: 'var(--color-text-secondary)' }}>
        open<span style={{ color: '#1D9E75' }}>pup</span>
      </span>
      {showDesktopControls && (
        <div style={{ display: 'flex', alignItems: 'stretch', height: '100%', marginLeft: '8px', flexShrink: 0 }}>
          <button
            aria-label="Minimize"
            onClick={() => { void getCurrentWindow().minimize(); }}
            style={{ background: 'none', border: 'none', width: '46px', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--color-text-tertiary)' }}
            onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-background-tertiary, rgba(128,128,128,0.15))'; }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; }}
          >
            <svg width="10" height="1" viewBox="0 0 10 1"><rect width="10" height="1" fill="currentColor" /></svg>
          </button>
          <button
            aria-label={isMaximized ? 'Restore' : 'Maximize'}
            onClick={() => { void getCurrentWindow().toggleMaximize(); }}
            style={{ background: 'none', border: 'none', width: '46px', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--color-text-tertiary)' }}
            onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-background-tertiary, rgba(128,128,128,0.15))'; }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; }}
          >
            {isMaximized ? (
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1"><rect x="2" y="3" width="6" height="6" rx="0.5" /><polyline points="3,3 3,1.5 8.5,1.5 8.5,7 7,7" /></svg>
            ) : (
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1"><rect x="1" y="1" width="8" height="8" rx="0.5" /></svg>
            )}
          </button>
          <button
            aria-label="Close"
            onClick={() => { void getCurrentWindow().close(); }}
            style={{ background: 'none', border: 'none', width: '46px', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--color-text-tertiary)' }}
            onMouseEnter={(e) => { e.currentTarget.style.background = '#e81123'; e.currentTarget.style.color = '#fff'; }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; e.currentTarget.style.color = 'var(--color-text-tertiary)'; }}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor" strokeWidth="1.2"><line x1="1" y1="1" x2="9" y2="9" /><line x1="9" y1="1" x2="1" y2="9" /></svg>
          </button>
        </div>
      )}
    </div>
  ) : null;

  if (onboardingDone === null) return (
    <div className="h-screen flex flex-col" style={{ background: 'var(--color-background-secondary)' }}>
      {minimalTitleBar}
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <div className="flex flex-col items-center gap-3">
          <span className="text-[22px] font-medium select-none" style={{ color: 'var(--color-text-secondary)' }}>
            open<span style={{ color: '#1D9E75' }}>pup</span>
          </span>
          <div className="w-4 h-4 rounded-full border-2 border-t-transparent animate-spin" style={{ borderColor: '#1D9E75', borderTopColor: 'transparent' }} />
        </div>
      </div>
    </div>
  );

  if (!onboardingDone) return (
    <div className="h-screen flex flex-col" style={{ background: 'var(--color-background-primary)' }}>
      {minimalTitleBar}
      <div style={{ flex: 1, overflow: 'hidden' }}>
        <Onboarding onComplete={() => {
          setOnboardingDone(true); loadMemoryChips();
          setMessages([{ id: 'welcome', role: 'assistant', content: t('onboarding_complete_message', lang), pup_key: 'alpha', pup_name: 'Alpha' }]);
        }} />
      </div>
    </div>
  );

  // Sidebar nav button matching spec style
  const NavBtn: React.FC<{ label: string; navKey: NavItem; onClick?: () => void }> = ({ label, navKey, onClick }) => {
    const isActive = activeNav === navKey;
    return (
      <button
        onClick={() => { setActiveNav(navKey); onClick?.(); }}
        style={{
          display: 'flex', alignItems: 'center', gap: '7px',
          padding: '6px 10px',
          margin: '1px 6px',
          borderRadius: '6px',
          fontSize: "13px",
          fontWeight: isActive ? 500 : 400,
          color: isActive ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
          background: isActive ? 'var(--color-background-secondary)' : 'transparent',
          border: 'none',
          cursor: 'pointer',
        }}
      >
        <span>{label}</span>
        {navKey === 'knowledge' && kbSourceCount > 0 && (
          <span style={{
            marginLeft: 'auto',
            minWidth: '18px',
            height: '18px',
            padding: '0 6px',
            borderRadius: '999px',
            background: 'rgba(29,158,117,0.14)',
            color: '#1D9E75',
            fontSize: '11px',
            lineHeight: '18px',
            textAlign: 'center',
            fontWeight: 500,
          }}>
            {kbSourceCount}
          </span>
        )}
        {navKey === 'channel' && activeChannelCount > 0 && (
          <span style={{
            marginLeft: 'auto',
            minWidth: '18px',
            height: '18px',
            padding: '0 6px',
            borderRadius: '999px',
            background: 'rgba(186,117,23,0.14)',
            color: '#BA7517',
            fontSize: '11px',
            lineHeight: '18px',
            textAlign: 'center',
            fontWeight: 500,
          }}>
            {activeChannelCount}
          </span>
        )}
      </button>
    );
  };

  const GroupsEntryIcon = ({ active }: { active: boolean }) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" style={{ color: active ? '#BA7517' : 'currentColor' }}>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );

  const FinanceEntryIcon = ({ active }: { active: boolean }) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" style={{ color: active ? '#0E6A4C' : 'currentColor' }}>
      <path d="M4 19h16" />
      <path d="M6 16l3-5 3 2 4-7 2 3" />
      <circle cx="6" cy="16" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="9" cy="11" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="13" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="16" cy="6" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="18" cy="9" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  );

  const isPrimaryView = activeNav === 'chat' || activeNav === 'channel' || activeNav === 'groups';
  const isChatActive = activeNav === 'chat';
  const getPupAccent = (pupKey: string) => pupAccentColor(pupKey);
  const getPupTagStyle = (pupKey: string) => pupTagStyle(pupKey);
  const pupMetaByKey = buildPupMetaByKey(pups);

  const formatTokens = (n: number) =>
    n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M`
    : n >= 1_000 ? `${(n / 1_000).toFixed(1)}k`
    : String(n);

  return (
    <div className="h-screen flex flex-col overflow-hidden" style={{ background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)' }}>

      {/* ── Custom overlay titlebar — desktop only ── */}
      {showDesktopTitlebar && (
      <div
        data-tauri-drag-region
        style={{
          height: '35px',
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          // macOS: reserve left space for traffic-light buttons
          // Windows: reserve right space for native overlay controls (~138px)
          // Linux: no native controls — custom buttons rendered on the right
          paddingLeft: platform === 'macos' ? '78px' : '12px',
          paddingRight: platform === 'macos' ? '12px' : '0px',
          background: 'var(--color-background-secondary)',
          borderBottom: '0.5px solid var(--color-border-tertiary)',
          userSelect: 'none',
        } as React.CSSProperties}
      >
        <span data-tauri-drag-region style={{
          fontSize: '13px',
          fontWeight: 500,
          color: 'var(--color-text-secondary)',
        }}>open<span style={{ color: '#1D9E75' }}>pup</span></span>
        <div data-tauri-drag-region style={{ display: 'flex', alignItems: 'center', gap: '10px', fontFamily: 'var(--font-mono)', fontSize: '10px', color: 'var(--color-text-tertiary)' }}>
          {/* Context stats + compress button */}
          {contextStats && (
            <div data-tauri-drag-region style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
              {contextStats.context_tokens > 0 ? (
                <span
                  data-tauri-drag-region
                  title={`${contextStats.context_tokens.toLocaleString()} / ${contextStats.context_limit.toLocaleString()} tokens (${Math.round(contextStats.context_tokens / contextStats.context_limit * 100)}%)`}
                  style={{
                    color: contextStats.context_tokens / contextStats.context_limit > 0.8
                      ? 'var(--color-text-danger, #e55)'
                      : contextStats.context_tokens / contextStats.context_limit > 0.5
                        ? 'var(--color-text-warning, #ea3)'
                        : 'var(--color-text-tertiary)',
                  }}
                >
                  {formatTokens(contextStats.context_tokens)}/{formatTokens(contextStats.context_limit)} ctx
                </span>
              ) : (
                <span data-tauri-drag-region style={{ opacity: 0.5 }}>-- ctx</span>
              )}
              {contextStats.compression_status.is_compressed && (
                <span data-tauri-drag-region style={{ fontSize: '9px', opacity: 0.6 }}>{t('ctx_compressed', lang)}</span>
              )}
              {!contextStats.compression_status.is_compressed && contextStats.message_count > 10 && (
                <button
                  title={t('ctx_compress_tip', lang)}
                  onClick={() => {
                    void invoke('compress_pup_context', { pupKey: selectedPupKey })
                      .then(() => invoke<ContextStats>('get_context_stats', { pupKey: selectedPupKey }))
                      .then((stats) => setContextStats(stats))
                      .catch(() => {});
                  }}
                  style={{
                    background: 'none', border: 'none', padding: '0 2px', cursor: 'pointer',
                    color: 'var(--color-text-tertiary)', fontSize: '10px', lineHeight: 1, opacity: 0.6,
                  }}
                >&#x2198;</button>
              )}
            </div>
          )}
          {/* Token usage — always visible */}
          {tokenUsage && (
            <div
              data-tauri-drag-region
              title={`${t('token_input', lang)}: ${tokenUsage.prompt_tokens.toLocaleString()}\n${t('token_output', lang)}: ${tokenUsage.completion_tokens.toLocaleString()}`}
              style={{ display: 'flex', alignItems: 'center', gap: '7px' }}
            >
              <span data-tauri-drag-region style={{ display: 'inline-flex', alignItems: 'center', gap: '2px' }}>
                <svg width="8" height="8" viewBox="0 0 8 8"><path d="M4 1L7 5.5H1Z" fill="var(--color-text-success)" opacity="0.75" /></svg>
                {formatTokens(tokenUsage.prompt_tokens)}
              </span>
              <span data-tauri-drag-region style={{ display: 'inline-flex', alignItems: 'center', gap: '2px' }}>
                <svg width="8" height="8" viewBox="0 0 8 8"><path d="M4 7L1 2.5H7Z" fill="var(--color-text-warning)" opacity="0.75" /></svg>
                {formatTokens(tokenUsage.completion_tokens)}
              </span>
            </div>
          )}
        </div>
        {/* ── Custom window control buttons for Windows & Linux (macOS uses native traffic lights) ── */}
        {showDesktopControls && (
          <div style={{ display: 'flex', alignItems: 'stretch', height: '100%', marginLeft: '8px', flexShrink: 0 }}>
            <button
              aria-label="Minimize"
              onClick={() => { void getCurrentWindow().minimize(); }}
              style={{
                background: 'none', border: 'none', width: '46px', cursor: 'pointer',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: 'var(--color-text-tertiary)',
              }}
              onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-background-tertiary, rgba(128,128,128,0.15))'; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; }}
            >
              <svg width="10" height="1" viewBox="0 0 10 1"><rect width="10" height="1" fill="currentColor" /></svg>
            </button>
            <button
              aria-label={isMaximized ? 'Restore' : 'Maximize'}
              onClick={() => { void getCurrentWindow().toggleMaximize(); }}
              style={{
                background: 'none', border: 'none', width: '46px', cursor: 'pointer',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: 'var(--color-text-tertiary)',
              }}
              onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-background-tertiary, rgba(128,128,128,0.15))'; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; }}
            >
              {isMaximized ? (
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
                  <rect x="2" y="3" width="6" height="6" rx="0.5" />
                  <polyline points="3,3 3,1.5 8.5,1.5 8.5,7 7,7" />
                </svg>
              ) : (
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
                  <rect x="1" y="1" width="8" height="8" rx="0.5" />
                </svg>
              )}
            </button>
            <button
              aria-label="Close"
              onClick={() => { void getCurrentWindow().close(); }}
              style={{
                background: 'none', border: 'none', width: '46px', cursor: 'pointer',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: 'var(--color-text-tertiary)',
              }}
              onMouseEnter={(e) => { e.currentTarget.style.background = '#e81123'; e.currentTarget.style.color = '#fff'; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = 'none'; e.currentTarget.style.color = 'var(--color-text-tertiary)'; }}
            >
              <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor" strokeWidth="1.2">
                <line x1="1" y1="1" x2="9" y2="9" />
                <line x1="9" y1="1" x2="1" y2="9" />
              </svg>
            </button>
          </div>
        )}
      </div>
      )}

      <div className="flex-1 flex overflow-hidden">

      {/* ── Left Sidebar ── */}
      {sidebarCollapsed ? (
        /* Collapsed strip */
        <div className="flex flex-col items-center pt-4 pb-3 shrink-0" style={{ width: '48px', borderRight: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
          <div className="flex flex-col gap-3">
            <button onClick={() => { setActiveNav('chat'); inputRef.current?.focus(); }} className="p-2 -m-2 group" title="Alpha">
              <span className="block w-2 h-2 rounded-full" style={{ background: getPupAccent('alpha') }} />
            </button>
            {pups.filter((p) => p.key !== 'alpha').map((pup) => (
              <button key={pup.key} onClick={() => { setActiveNav('chat'); insertMention(pup.key); }} className="p-2 -m-2" title={pup.display_name}>
                <span className="block w-2 h-2 rounded-full" style={{ background: getPupAccent(pup.key) }} />
              </button>
            ))}
          </div>
          <div className="flex-1" />
          <button
            onClick={() => setActiveNav('finance')}
            title={t('nav_finance', lang)}
            style={{
              width: '34px',
              height: '34px',
              marginBottom: '10px',
              borderRadius: '11px',
              border: activeNav === 'finance' ? '0.5px solid rgba(16,59,47,0.18)' : '0.5px solid transparent',
              boxShadow: activeNav === 'finance' ? '0 8px 20px rgba(16,59,47,0.16)' : 'none',
              background: activeNav === 'finance' ? 'linear-gradient(180deg, rgba(16,59,47,0.18), rgba(16,59,47,0.08))' : 'var(--color-background-secondary)',
              color: activeNav === 'finance' ? '#0E6A4C' : 'var(--color-text-tertiary)',
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              alignSelf: 'center',
            }}
          >
            <FinanceEntryIcon active={activeNav === 'finance'} />
          </button>
          <div style={{ width: '100%', display: 'flex', justifyContent: 'flex-end', paddingRight: '4px' }}>
            <button
              onClick={() => setSidebarCollapsed(false)}
              style={{ fontSize: "13px", color: 'var(--color-text-tertiary)', padding: '4px 8px', background: 'none', border: 'none', cursor: 'pointer' }}
              title={t('sidebar_expand', lang)}
            >
              ›
            </button>
          </div>
        </div>
      ) : (
        /* Expanded sidebar */
        <div className="flex flex-col shrink-0" style={{ width: '196px', borderRight: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)', overflow: 'hidden' }}>
          {/* Primary mode switch */}
          <div className="px-3 py-3" style={{ borderBottom: '0.5px solid var(--color-border-tertiary)' }}>
            <div style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: '4px',
              alignItems: 'center',
              padding: '4px',
              borderRadius: '12px',
              background: 'var(--color-background-secondary)',
            }}>
              <button
                onClick={() => { setActiveNav('chat'); inputRef.current?.focus(); }}
                style={{
                  border: 'none',
                  borderRadius: '9px',
                  padding: '8px 10px',
                  fontSize: '12px',
                  fontWeight: activeNav === 'chat' ? 600 : 500,
                  color: activeNav === 'chat' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                  background: activeNav === 'chat' ? 'var(--color-background-primary)' : 'transparent',
                  cursor: 'pointer',
                }}
              >
                {t('nav_chat', lang)}
              </button>
              <button
                onClick={() => { setActiveNav('channel'); setChannelDetailMode(false); }}
                style={{
                  border: 'none',
                  borderRadius: '9px',
                  padding: '8px 10px',
                  fontSize: '12px',
                  fontWeight: activeNav === 'channel' ? 600 : 500,
                  color: activeNav === 'channel' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                  background: activeNav === 'channel' ? 'var(--color-background-primary)' : 'transparent',
                  cursor: 'pointer',
                  position: 'relative',
                }}
              >
                <span>{t('nav_pack_channel', lang)}</span>
                {activeChannelCount > 0 && (
                  <span style={{
                    position: 'absolute',
                    top: '5px',
                    right: '8px',
                    minWidth: '16px',
                    height: '16px',
                    padding: '0 4px',
                    borderRadius: '999px',
                    background: 'rgba(186,117,23,0.16)',
                    color: '#BA7517',
                    fontSize: '10px',
                    lineHeight: '16px',
                    textAlign: 'center',
                    fontWeight: 600,
                  }}>
                    {activeChannelCount}
                  </span>
                )}
              </button>
            </div>
          </div>

          <div className="flex flex-col flex-1 overflow-y-auto px-2 pt-3 pb-3 gap-4">
            {/* 狗群 section */}
            <div style={{ display: isPrimaryView ? 'block' : 'none' }}>
              <div className="px-2 pb-1.5" style={{ fontSize: "10px", fontWeight: 500, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                {t('pup_section', lang)}
              </div>
              <button
                onClick={() => { setActiveNav('chat'); setSelectedPupKey('alpha'); inputRef.current?.focus(); }}
                style={{
                  display: 'flex', alignItems: 'center', gap: '6px', width: '100%',
                  padding: '6px 10px', margin: '1px 6px', borderRadius: '6px', fontSize: "13px",
                  fontWeight: isChatActive && selectedPupKey === 'alpha' ? 500 : 400,
                  color: isChatActive && selectedPupKey === 'alpha' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                  background: isChatActive && selectedPupKey === 'alpha' ? 'var(--color-background-secondary)' : 'transparent',
                  border: 'none', cursor: 'pointer',
                }}
              >
                <span style={{ width: '7px', height: '7px', borderRadius: '50%', background: getPupAccent('alpha'), flexShrink: 0 }} />
                Alpha
              </button>
              {pups.filter((p) => p.key !== 'alpha').map((pup) => (
                <button
                  key={pup.key}
                  onClick={() => { setActiveNav('chat'); setSelectedPupKey(pup.key); insertMention(pup.key); }}
                  style={{
                    display: 'flex', alignItems: 'center', gap: '6px', width: '100%',
                    padding: '6px 10px', margin: '1px 6px', borderRadius: '6px', fontSize: "13px", fontWeight: 400,
                    color: isChatActive && selectedPupKey === pup.key ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                    background: isChatActive && selectedPupKey === pup.key ? 'var(--color-background-secondary)' : 'transparent',
                    border: 'none', cursor: 'pointer',
                  }}
                >
                  <span style={{ width: '7px', height: '7px', borderRadius: '50%', background: getPupAccent(pup.key), flexShrink: 0 }} />
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{pup.display_name}</span>
                </button>
              ))}
            </div>

            {/* 工具 section — collapsible */}
            <div>
              <button
                onClick={() => setToolsExpanded(!toolsExpanded)}
                style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', width: '100%', padding: '0 8px 6px', fontSize: "10px", fontWeight: 500, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', background: 'none', border: 'none', cursor: 'pointer' }}
              >
                <span>{t('sidebar_tools', lang)}</span>
                <span style={{ fontSize: "10px", opacity: 0.6 }}>{toolsExpanded ? '▾' : '▸'}</span>
              </button>
              {toolsExpanded && (
                <>
                  <NavBtn label={t('nav_timeline', lang)} navKey="timeline" />
                  <NavBtn label={t('nav_memories', lang)} navKey="memories" />
                  <NavBtn label={t('nav_knowledge', lang)} navKey="knowledge" onClick={() => invoke<{ id: string }[]>('kb_list_sources').then(s => setKbSourceCount(s.length)).catch(() => {})} />
                  <NavBtn label={t('nav_tasks', lang)} navKey="tasks" />
                </>
              )}
            </div>

            {/* Config section — collapsible */}
            <div>
              <button
                onClick={() => setConfigExpanded(!configExpanded)}
                style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', width: '100%', padding: '0 8px 6px', fontSize: "10px", fontWeight: 500, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', background: 'none', border: 'none', cursor: 'pointer' }}
              >
                <span>{t('sidebar_config', lang)}</span>
                <span style={{ fontSize: "10px", opacity: 0.6 }}>{configExpanded ? '▾' : '▸'}</span>
              </button>
              {configExpanded && (
                <>
                  <NavBtn label={t('nav_pups', lang)} navKey="pups" onClick={loadPups} />
                  <NavBtn label={t('nav_skills', lang)} navKey="skills" />
                  <NavBtn label={t('nav_mcp', lang)} navKey="mcp" />
                  <NavBtn label={t('nav_bridge', lang)} navKey="bridge" />
                  <NavBtn label={t('nav_settings', lang)} navKey="settings" />
                </>
              )}
            </div>

          </div>

          {/* Secondary destination + footer utilities */}
          <div className="px-2 pb-3 pt-2">
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <button
                onClick={() => setActiveNav('finance')}
                title={t('nav_finance', lang)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: '8px',
                  width: '100%',
                  minHeight: '28px',
                  padding: '4px 8px',
                  borderRadius: '8px',
                  fontSize: '12px',
                  fontWeight: activeNav === 'finance' ? 600 : 500,
                  color: activeNav === 'finance' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                  background: activeNav === 'finance' ? 'rgba(16,59,47,0.08)' : 'transparent',
                  border: 'none',
                  cursor: 'pointer',
                }}
              >
                <span style={{
                  width: '18px',
                  height: '18px',
                  borderRadius: '5px',
                  display: 'inline-flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  background: activeNav === 'finance' ? 'rgba(16,59,47,0.12)' : 'transparent',
                  color: activeNav === 'finance' ? '#0E6A4C' : 'var(--color-text-tertiary)',
                  flexShrink: 0,
                }}>
                  <FinanceEntryIcon active={activeNav === 'finance'} />
                </span>
                <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {t('nav_finance', lang)}
                </span>
                <span style={{ marginLeft: 'auto', flexShrink: 0, fontSize: '10px', fontWeight: 650, color: '#0E6A4C', background: 'rgba(29,158,117,0.10)', borderRadius: '999px', padding: '1px 6px' }}>
                  Market
                </span>
              </button>

              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(5, minmax(0, 1fr))',
                alignItems: 'center',
                gap: '2px',
                padding: '6px 0 0',
                borderTop: '0.5px solid var(--color-border-tertiary)',
              }}>
                <button
                  onClick={async () => {
                    const next = execMode === 'leashed' ? 'free_run' : 'leashed';
                    await invoke('set_execution_mode', { mode: next }).catch(() => {});
                    setExecMode(next);
                  }}
                  title={execMode === 'leashed'
                    ? t('mode_tooltip_leashed', lang)
                    : t('mode_tooltip_free', lang)
                  }
                  style={{
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    minWidth: 0,
                    height: '24px',
                    borderRadius: '6px',
                    fontSize: '12px',
                    color: 'var(--color-text-tertiary)',
                    cursor: 'pointer', background: 'none', border: 'none', padding: '0 3px'
                  }}
                >
                  {execMode === 'leashed' ? '🔒' : '🐕'}
                </button>
                <button
                  onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
                  style={{ height: '24px', borderRadius: '6px', fontSize: "11px", color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '0 3px' }}
                >
                  {t('settings_lang_toggle_button', lang)}
                </button>
                {/* Theme toggle: sun/moon icon */}
                <button
                  onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
                  title={theme === 'dark' ? t('theme_light', lang) : t('theme_dark', lang)}
                  style={{ height: '24px', borderRadius: '6px', color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '0 3px', lineHeight: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                >
                  {theme === 'dark' ? (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
                    </svg>
                  ) : (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
                    </svg>
                  )}
                </button>
                <button
                  onClick={() => invoke('open_url', { url: 'https://github.com/openpup/openpup' })}
                  style={{ height: '24px', borderRadius: '6px', color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '0 3px', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                  title="GitHub"
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M12 2C6.477 2 2 6.477 2 12c0 4.418 2.865 8.166 6.839 9.489.5.092.682-.217.682-.482 0-.237-.009-.868-.013-1.703-2.782.604-3.369-1.341-3.369-1.341-.454-1.155-1.11-1.462-1.11-1.462-.908-.62.069-.608.069-.608 1.003.07 1.531 1.03 1.531 1.03.892 1.529 2.341 1.087 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.11-4.555-4.943 0-1.091.39-1.984 1.029-2.683-.103-.253-.446-1.27.098-2.647 0 0 .84-.269 2.75 1.025A9.578 9.578 0 0 1 12 6.836a9.59 9.59 0 0 1 2.504.337c1.909-1.294 2.747-1.025 2.747-1.025.546 1.377.202 2.394.1 2.647.64.699 1.028 1.592 1.028 2.683 0 3.842-2.339 4.687-4.566 4.935.359.309.678.919.678 1.852 0 1.336-.012 2.415-.012 2.743 0 .267.18.578.688.48C19.138 20.163 22 16.418 22 12c0-5.523-4.477-10-10-10z"/>
                  </svg>
                </button>
                <button
                  onClick={() => setSidebarCollapsed(true)}
                  style={{ height: '24px', borderRadius: '6px', color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '0 3px', lineHeight: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                  title={t('sidebar_collapse', lang)}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M15 19l-7-7 7-7" />
                  </svg>
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

        {/* ── Main content ── */}
        <div className="flex-1 flex flex-col overflow-hidden" style={{ background: 'var(--color-background-primary)' }}>
          {/* ── Chat ── */}
          {activeNav === 'chat' && (
            <>
              {/* Memory chips */}
              {memoryChips.length > 0 && (
                <div style={{ borderBottom: '0.5px solid var(--color-border-tertiary)', padding: '8px 14px', flexShrink: 0, display: 'flex', alignItems: 'center', gap: '6px', overflow: 'hidden' }}>
                  {memoryChips.slice(0, 5).map((chip, i) => (
                    <span key={i} style={{
                      display: 'inline-flex', alignItems: 'center', gap: '4px', flexShrink: 0, maxWidth: '180px',
                      fontSize: "11px", padding: '3px 9px', borderRadius: '20px',
                      background: 'var(--color-background-secondary)',
                      border: '0.5px solid var(--color-border-tertiary)',
                      color: 'var(--color-text-secondary)',
                      overflow: 'hidden',
                    }}>
                      <span style={{ width: '5px', height: '5px', borderRadius: '50%', background: '#1D9E75', flexShrink: 0 }} />
                      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{chip.content}</span>
                    </span>
                  ))}
                </div>
              )}

              {/* Messages */}
              <div
                ref={chatContainerRef}
                style={{ flex: 1, overflowY: 'auto', padding: '14px' }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px', maxWidth: '720px', margin: '0 auto', width: '100%' }}>
                  {messages.map((m) => {
                    const isNew = !animatedMsgIds.current.has(m.id);
                    animatedMsgIds.current.add(m.id);
                    const accentColor = getPupAccent(m.pup_key ?? 'alpha');
                    return m.role === 'user' ? (
                      <div key={m.id} className={isNew ? 'animate-msg-in' : ''} style={{ display: 'flex', justifyContent: 'flex-end' }}>
                        <div style={{
                          maxWidth: '72%',
                          background: 'var(--color-background-info)',
                          color: 'var(--color-text-info)',
                          borderRadius: '10px 10px 4px 10px',
                          padding: '9px 11px',
                          fontSize: "13px",
                          lineHeight: 1.6,
                        }}>
                          {m.content}
                        </div>
                      </div>
                    ) : (
                      <div key={m.id} className={`msg-bubble ${isNew ? 'animate-msg-in' : ''}`} style={{ maxWidth: '80%' }}>
                        {m.pup_name && (
                          <span style={{
                            display: 'inline-block', marginBottom: '4px',
                            fontSize: "10px", padding: '1px 7px', borderRadius: '10px',
                            ...getPupTagStyle(m.pup_key ?? 'alpha'),
                          }}>
                            {m.pup_name}
                          </span>
                        )}
                        <div style={{
                          background: 'var(--color-background-primary)',
                          border: '0.5px solid var(--color-border-tertiary)',
                          borderRadius: '10px 10px 10px 4px',
                          borderLeft: `2px solid ${accentColor}`,
                          padding: '9px 11px',
                          fontSize: "13px",
                          lineHeight: 1.6,
                          color: 'var(--color-text-primary)',
                        }}>
                          <div className="prose prose-sm max-w-none" style={{ color: 'var(--color-text-primary)' }}>
                            <MarkdownRenderer>{m.content}</MarkdownRenderer>
                          </div>
                        </div>
                        <MessageActions
                          messageId={m.id}
                          content={m.content}
                        />
                      </div>
                    );
                  })}
                  {sending && (
                    <div className="animate-msg-in" style={{ maxWidth: '80%' }}>
                      <span style={{
                        display: 'inline-block', marginBottom: '4px',
                        fontSize: "10px", padding: '1px 7px', borderRadius: '10px',
                        ...getPupTagStyle(streamingPupName?.key ?? 'alpha'),
                      }}>
                        {streamingPupName?.name || 'Alpha'}
                      </span>
                      <div style={{
                        background: 'var(--color-background-primary)',
                        border: '0.5px solid var(--color-border-tertiary)',
                        borderRadius: '10px 10px 10px 4px',
                        borderLeft: `2px solid ${getPupAccent(streamingPupName?.key ?? 'alpha')}`,
                        padding: '9px 11px',
                        fontSize: "13px",
                        lineHeight: 1.6,
                        color: 'var(--color-text-primary)',
                      }}>
                        <div className="flex flex-col gap-1.5">
                          {streamingSteps.length > 0 && (
                            <div className="flex flex-col gap-0.5">
                              {streamingSteps.map((step, i) => {
                                const isLast = i === streamingSteps.length - 1;
                                const icon = ACTIVITY_ICON[step.kind] ?? '⚙';
                                return (
                                  <div key={i} className={`flex items-center gap-1.5 transition-opacity ${isLast ? 'opacity-100' : 'opacity-30'}`} style={{ fontSize: "11px", color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                                    <span className="shrink-0">{icon}</span>
                                    <span className="truncate" style={{ maxWidth: '320px' }}>{step.label}</span>
                                  </div>
                                );
                              })}
                            </div>
                          )}
                          {streamingReasoningContent && (
                            <div className="italic max-h-20 overflow-y-auto" style={{ fontSize: "11px", color: 'var(--color-text-tertiary)' }}>
                              {streamingReasoningContent}
                            </div>
                          )}
                          <div className="prose prose-sm max-w-none" style={{ color: 'var(--color-text-primary)' }}>
                            {streamingContent
                              ? <MarkdownRenderer>{streamingContent}</MarkdownRenderer>
                              : <span className="animate-pulse" style={{ color: 'var(--color-text-tertiary)' }}>{t('chat_thinking', lang)}</span>
                            }
                          </div>
                        </div>
                      </div>
                    </div>
                  )}
                  <div ref={messagesEndRef} />
                </div>
              </div>

              {/* Input */}
              <div style={{ flexShrink: 0, padding: '10px 14px', borderTop: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
              <div style={{ display: 'flex', gap: '8px', maxWidth: '720px', margin: '0 auto', width: '100%' }}>
                <textarea
                  ref={inputRef}
                  rows={1}
                  placeholder={t('chat_placeholder_alpha', lang)}
                  value={input}
                  onChange={(e) => {
                    setInput(e.target.value);
                    const el = e.target;
                    el.style.height = 'auto';
                    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
                  }}
                  onCompositionStart={() => { imeComposingRef.current = true; }}
                  onCompositionEnd={() => { imeComposingRef.current = false; }}
                  onKeyDown={onKeyDown}
                  style={{
                    flex: 1, resize: 'none', outline: 'none',
                    fontSize: "13px", padding: '8px 10px', borderRadius: '8px',
                    border: '0.5px solid var(--color-border-secondary)',
                    background: 'var(--color-background-primary)',
                    color: 'var(--color-text-primary)',
                    fontFamily: 'inherit',
                    lineHeight: '1.5',
                    overflowY: 'auto',
                  }}
                />
                {sending ? (
                  <button
                    onClick={() => void abort()}
                    style={{
                      alignSelf: 'flex-end', padding: '8px 14px', borderRadius: '8px', border: 'none',
                      background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)',
                      cursor: 'pointer', fontSize: "13px",
                    }}
                  >■</button>
                ) : (
                  <button
                    onClick={() => void send()}
                    disabled={!input.trim()}
                    style={{
                      alignSelf: 'flex-end', padding: '8px 14px', borderRadius: '8px', border: 'none',
                      background: 'var(--color-text-primary)', color: 'var(--color-background-primary)',
                      cursor: input.trim() ? 'pointer' : 'not-allowed', fontSize: "13px",
                      opacity: input.trim() ? 1 : 0.25,
                    }}
                  >↑</button>
                )}
              </div>
              </div>
            </>
          )}

          {/* ── Group Chat Preview / Experimental UI ── */}
          {activeNav === 'groups' && (groupChatFullEnabled ? <GroupChat /> : <GroupChatPreview />)}

          {/* ── Finance ── */}
          {activeNav === 'finance' && <FinanceWorkbench />}

          {/* ── Memories ── */}
          {activeNav === 'memories' && (
            <div className="flex-1 overflow-hidden flex flex-col px-4 py-4">
              <div className="flex items-center gap-1 mb-4 shrink-0">
                {(['long_term', 'diary'] as const).map((tab) => (
                  <button key={tab} onClick={() => setMemoriesTab(tab)}
                    style={{
                      padding: '3px 10px', borderRadius: '20px', fontSize: "12px", border: 'none', cursor: 'pointer',
                      fontWeight: memoriesTab === tab ? 500 : 400,
                      background: memoriesTab === tab ? 'var(--color-background-secondary)' : 'transparent',
                      color: memoriesTab === tab ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                    }}>
                    {t(tab === 'long_term' ? 'tab_long_term' : 'tab_diary', lang)}
                  </button>
                ))}
              </div>
              <div className="flex-1 overflow-auto min-h-0">
                {memoriesTab === 'long_term' ? <MemoryManager /> : <DiaryViewer />}
              </div>
            </div>
          )}

          {/* ── Pack Channel ── */}
          {activeNav === 'channel' && (
            <div className="flex-1 overflow-hidden flex flex-col">
              <PackChannel
                channels={channels}
                activeChannelId={activeChannelId}
                messages={channelMessages}
                error={channelError}
                plan={channelPlan}
                workflow={channelWorkflow}
                isCompleted={channelCompleted}
                detailMode={channelDetailMode}
                onSelectChannel={(id) => {
                  setActiveChannelId(id);
                  setChannelDetailMode(true);
                }}
                onBackToList={() => setChannelDetailMode(false)}
                onOpenFinalReply={() => {
                  setChannelDetailMode(false);
                  setActiveNav('chat');
                }}
                onClearCompleted={() => void clearCompletedChannels()}
                onClearStale={() => void clearStaleChannels()}
                onContinueChannel={(channelId, comment) => void continueChannel(channelId, comment)}
                onRequestChannelChanges={(channelId, comment, replyTo) => void requestChannelChanges(channelId, comment, replyTo)}
                onSubmitReviewComment={(channelId, comment, replyTo) => void submitChannelReviewComment(channelId, comment, replyTo)}
                onAbortChannel={(channelId, comment) => void abortChannel(channelId, comment)}
                pupMetaByKey={pupMetaByKey}
              />
            </div>
          )}

          {/* ── Timeline ── */}
          {activeNav === 'timeline' && (
            <div className="flex-1 overflow-auto px-5 py-4"><Timeline /></div>
          )}

          {/* ── Knowledge Base ── */}
          {activeNav === 'knowledge' && (
            <div className="flex-1 overflow-hidden flex flex-col px-5 py-4"><KnowledgeBase /></div>
          )}

          {/* ── Tasks ── */}
          {activeNav === 'tasks' && (
            <div className="flex-1 overflow-auto px-5 py-4"><TaskManager /></div>
          )}

          {/* ── Skills ── */}
          {activeNav === 'skills' && (
            <div className="flex-1 overflow-hidden flex flex-col px-5 py-4"><SkillClaw /></div>
          )}

          {/* ── Pups ── */}
          {activeNav === 'pups' && (
            <div className="flex-1 overflow-auto px-5 py-4"><PupManager /></div>
          )}

          {/* ── MCP ── */}
          {activeNav === 'mcp' && (
            <div className="flex-1 overflow-auto px-5 py-4"><McpSettings /></div>
          )}

          {/* ── Bridge ── */}
          {activeNav === 'bridge' && (
            <div className="flex-1 overflow-auto px-5 py-4"><BridgeSettings /></div>
          )}

          {/* ── Settings ── */}
          {activeNav === 'settings' && (
            <div className="flex-1 overflow-auto px-4 py-4 space-y-8" style={{ fontSize: "13px" }}>

              <LlmConfigPanel />

              <div style={{ borderTop: '0.5px solid var(--color-border-tertiary)' }} />

              <KnowledgeSettings />

              <div style={{ borderTop: '0.5px solid var(--color-border-tertiary)' }} />

              <DesktopSettings />

              <div style={{ borderTop: '0.5px solid var(--color-border-tertiary)' }} />

              <section>
                <h2 className="mb-3" style={{ fontSize: "14px", fontWeight: 500, color: 'var(--color-text-primary)' }}>{t('settings_backup', lang)}</h2>
                <button
                  style={{ padding: '6px 12px', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', fontSize: "13px", cursor: exporting ? 'not-allowed' : 'pointer', opacity: exporting ? 0.4 : 1 }}
                  onClick={() => void exportWorkspace()} disabled={exporting}>
                  {exporting ? t('settings_exporting', lang) : t('settings_export', lang)}
                </button>
              </section>

              <section>
                <h2 className="mb-3" style={{ fontSize: "14px", fontWeight: 500, color: 'var(--color-text-primary)' }}>{t('settings_restore', lang)}</h2>
                <input
                  className="w-full focus:outline-none mb-3"
                  style={{ fontSize: "13px", padding: '7px 10px', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', color: 'var(--color-text-primary)' }}
                  placeholder={t('settings_restore_ph', lang)} value={importPath} onChange={(e) => setImportPath(e.target.value)} />
                <button
                  style={{ padding: '6px 12px', borderRadius: '8px', border: 'none', background: '#1D9E75', color: '#fff', fontSize: "13px", cursor: importing ? 'not-allowed' : 'pointer', opacity: importing ? 0.4 : 1 }}
                  onClick={() => void importWorkspace()} disabled={importing}>
                  {importing ? t('settings_importing', lang) : t('settings_import', lang)}
                </button>
              </section>

              {settingsErr && <p style={{ color: 'var(--color-text-danger)', background: 'var(--color-background-danger)', padding: '8px 10px', borderRadius: '8px', fontSize: "13px" }}>{settingsErr}</p>}
              {settingsMsg && <p style={{ color: 'var(--color-text-success)', background: 'var(--color-background-success)', padding: '8px 10px', borderRadius: '8px', fontSize: "13px" }}>{settingsMsg}</p>}
            </div>
          )}
        </div>

      {permissionRequest && (
        <PermissionDialog request={permissionRequest}
          onApprove={(remember) => void handleApprove(remember)}
          onDeny={() => void handleDeny()} />
      )}
      </div>{/* close flex-1 wrapper */}
    </div>
  );
};

export const App: React.FC = () => (
  <LangProvider>
    <AppInner />
  </LangProvider>
);
