import React, { useEffect, useRef, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MarkdownRenderer } from './components/MarkdownRenderer';
import { Onboarding } from './components/Onboarding';
import { SkillClaw } from './components/SkillClaw';
import { MemoryManager } from './components/MemoryManager';
import { Timeline } from './components/Timeline';
import { DiaryViewer } from './components/DiaryViewer';
import { McpSettings } from './components/McpSettings';
import { PupManager } from './components/PupManager';
import { TaskManager } from './components/TaskManager';
import { PermissionDialog, PermissionRequest } from './components/PermissionDialog';
import { PackChannel } from './components/PackChannel';
import { BridgeSettings } from './components/BridgeSettings';
import { usePackChannel } from './hooks/usePackChannel';
import { LangProvider, useLang, t } from './i18n';
import { buildPupMetaByKey, pupAccentColor, pupTagStyle } from './utils/pupVisuals';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  pup_key?: string;
  pup_name?: string;
  timestamp?: number;
}

interface StreamDonePayload {
  pup_key: string;
  pup_name: string;
  content: string;
}

interface StreamingPupState {
  key: string;
  name: string;
}

interface MemoryChip {
  content: string;
  memory_type: string;
  importance: number;
}

interface ActivityStep {
  kind: 'routing' | 'skill' | 'shell' | 'file_read' | 'file_write' | 'http' | 'memory' | 'task' | 'mcp' | 'tool_call' | string;
  label: string;
}

interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
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



interface PupConfig {
  key: string;
  display_name: string;
  description: string;
  enabled: boolean;
  is_custom: boolean;
}

interface ContextStats {
  pup_key: string;
  message_count: number;
  context_tokens: number;
  context_limit: number;
  compression_status: {
    is_compressed: boolean;
    last_compression_row: number;
  };
}

type NavItem = 'chat' | 'channel' | 'memories' | 'timeline' | 'skills' | 'pups' | 'tasks' | 'mcp' | 'bridge' | 'settings';

// ─── LLM Config Panel ────────────────────────────────────────────────────────

interface LlmConfigInfo {
  provider: string;
  model: string;
  mini_model: string;
  embed_model: string;
  api_base: string | null;
}

const PROVIDER_PRESETS: Record<string, { label: string; api_base: string; model: string; mini_model: string; embed_model: string }> = {
  openai:       { label: 'OpenAI',        api_base: '',                              model: 'gpt-4o',                   mini_model: 'gpt-4o-mini',           embed_model: 'text-embedding-3-small' },
  siliconflow:  { label: 'SiliconFlow',   api_base: 'https://api.siliconflow.cn/v1', model: 'deepseek-ai/DeepSeek-V3',  mini_model: 'Qwen/Qwen2.5-7B-Instruct', embed_model: 'BAAI/bge-m3' },
  ollama:       { label: 'Ollama (Local)', api_base: 'http://localhost:11434/v1',     model: 'llama3',                   mini_model: 'llama3',                embed_model: 'nomic-embed-text' },
  deepseek:     { label: 'DeepSeek',      api_base: 'https://api.deepseek.com/v1',   model: 'deepseek-chat',            mini_model: 'deepseek-chat',         embed_model: '' },
  openrouter:   { label: 'OpenRouter',    api_base: 'https://openrouter.ai/api/v1',  model: 'anthropic/claude-3.5-sonnet', mini_model: 'openai/gpt-4o-mini', embed_model: '' },
};

const LlmConfigPanel: React.FC = () => {
  const { lang } = useLang();
  const [cfg, setCfg] = React.useState<LlmConfigInfo | null>(null);
  const [form, setForm] = React.useState({ provider: 'openai', model: '', miniModel: '', embedModel: '', apiKey: '', apiBase: '' });
  const [saving, setSaving] = React.useState(false);
  const [msg, setMsg] = React.useState<string | null>(null);
  const [msgIsError, setMsgIsError] = React.useState(false);

  React.useEffect(() => {
    invoke<LlmConfigInfo>('get_llm_config').then((c) => {
      setCfg(c);
      setForm({
        provider: c.provider,
        model: c.model,
        miniModel: c.mini_model,
        embedModel: c.embed_model,
        apiKey: '',
        apiBase: c.api_base ?? '',
      });
    }).catch(() => {});
  }, []);

  const applyPreset = (key: string) => {
    const p = PROVIDER_PRESETS[key];
    if (!p) return;
    setForm((f) => ({ ...f, provider: key === 'ollama' ? 'ollama' : 'openai', model: p.model, miniModel: p.mini_model, embedModel: p.embed_model, apiBase: p.api_base }));
  };

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      await invoke('set_llm_provider', {
        provider: form.provider,
        model: form.model,
        miniModel: form.miniModel || null,
        embedModel: form.embedModel || null,
        apiKey: form.apiKey || null,
        apiBase: form.apiBase || null,
      });
      setMsg(t('llm_saved', lang));
      setMsgIsError(false);
      setTimeout(() => setMsg(null), 2000);
    } catch (e) {
      setMsg(`${t('llm_save_failed', lang)}: ${e}`);
      setMsgIsError(true);
    } finally {
      setSaving(false);
    }
  };

  const field = (label: string, value: string, key: keyof typeof form, ph?: string, type = 'text') => (
    <div className="flex items-center gap-3">
      <label className="w-28 shrink-0" style={{ fontSize: "12px", color: 'var(--color-text-tertiary)' }}>{label}</label>
      <input
        type={type}
        className="flex-1 focus:outline-none transition-colors"
        style={{ fontSize: "13px", padding: '6px 10px', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)', color: 'var(--color-text-primary)' }}
        value={value}
        placeholder={ph}
        onChange={(e) => setForm((f) => ({ ...f, [key]: e.target.value }))}
      />
    </div>
  );

  return (
    <section>
      <h2 style={{ fontSize: "14px", fontWeight: 500, color: 'var(--color-text-primary)', marginBottom: '14px' }}>{t('llm_config_title', lang)}</h2>

      {/* Provider presets */}
      <div className="flex flex-wrap gap-2 mb-4">
        {Object.entries(PROVIDER_PRESETS).map(([k, p]) => {
          const isSelected = form.provider === (k === 'ollama' ? 'ollama' : 'openai') && (form.apiBase || '') === p.api_base;
          return (
            <button key={k} onClick={() => applyPreset(k)} style={{
              fontSize: "12px", padding: '3px 10px', borderRadius: '20px', cursor: 'pointer',
              border: `0.5px solid ${isSelected ? '#BA7517' : 'var(--color-border-secondary)'}`,
              color: isSelected ? '#BA7517' : 'var(--color-text-secondary)',
              background: isSelected ? 'var(--color-background-warning)' : 'transparent',
            }}>{p.label}</button>
          );
        })}
      </div>

      <div className="space-y-2.5">
        {field(t('llm_main_model', lang), form.model, 'model', 'gpt-4o / deepseek-chat')}
        {field(t('llm_mini_model', lang), form.miniModel, 'miniModel', t('llm_mini_ph', lang))}
        {field(t('llm_embed_model', lang), form.embedModel, 'embedModel', 'text-embedding-3-small / BAAI/bge-m3')}
        {field('API Base', form.apiBase, 'apiBase', t('llm_base_ph', lang))}
        {field('API Key', form.apiKey, 'apiKey', t('llm_key_ph', lang), 'password')}
      </div>

      <div className="mt-3 space-y-1 leading-relaxed" style={{ fontSize: "11px", color: 'var(--color-text-tertiary)' }}>
        <div>{t('llm_hint_mini', lang)}</div>
        <div>{t('llm_hint_embed', lang)}</div>
      </div>

      <div className="flex items-center gap-3 mt-4">
        <button
          onClick={() => void save()} disabled={saving}
          style={{ padding: '6px 12px', borderRadius: '8px', border: 'none', background: 'var(--color-text-primary)', color: 'var(--color-background-primary)', fontSize: "13px", cursor: saving ? 'not-allowed' : 'pointer', opacity: saving ? 0.5 : 1 }}
        >
          {saving ? t('llm_saving', lang) : t('llm_save', lang)}
        </button>
        {msg && <span style={{ fontSize: "13px", color: msgIsError ? 'var(--color-text-danger)' : 'var(--color-text-success)' }}>{msg}</span>}
        {cfg && <span className="ml-auto" style={{ fontSize: "11px", color: 'var(--color-text-tertiary)' }}>{t('llm_current', lang)} {cfg.model}</span>}
      </div>
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
/** Detect the host platform once — used for titlebar layout. */
const detectPlatform = (): 'macos' | 'windows' | 'linux' => {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('macintosh') || ua.includes('mac os')) return 'macos';
  if (ua.includes('windows')) return 'windows';
  return 'linux';
};

const AppInner: React.FC = () => {
  const { lang, setLang } = useLang();
  const platform = useMemo(detectPlatform, []);
  const [isMaximized, setIsMaximized] = useState(false);
  const [theme, setTheme] = React.useState<'dark' | 'light'>(() => {
    const saved = localStorage.getItem('openpup_theme');
    return (saved === 'light' ? 'light' : 'dark');
  });
  const [onboardingDone, setOnboardingDone] = useState<boolean | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingContent, setStreamingContent] = useState('');
  const [streamingReasoningContent, setStreamingReasoningContent] = useState('');
  const [streamingPupName, setStreamingPupName] = useState<StreamingPupState | null>(null);
  const [streamingSteps, setStreamingSteps] = useState<ActivityStep[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [activeNav, setActiveNav] = useState<NavItem>('chat');
  const [channelDetailMode, setChannelDetailMode] = useState(false);
  const activeNavRef = useRef<string>('chat');
  useEffect(() => { activeNavRef.current = activeNav; }, [activeNav]);
  // Auto-expand sidebar section when an item inside it becomes active
  useEffect(() => {
    const toolsItems: NavItem[] = ['memories', 'timeline', 'tasks', 'skills'];
    const configItems: NavItem[] = ['pups', 'mcp', 'bridge', 'settings'];
    if (toolsItems.includes(activeNav)) setToolsExpanded(true);
    if (configItems.includes(activeNav)) setConfigExpanded(true);
  }, [activeNav]);
  const [memoryChips, setMemoryChips] = useState<MemoryChip[]>([]);
  const [permissionRequest, setPermissionRequest] = useState<PermissionRequest | null>(null);
  const [pups, setPups] = useState<PupConfig[]>([]);
  const [selectedPupKey, setSelectedPupKey] = useState<string>('alpha');
  const [memoriesTab, setMemoriesTab] = useState<'long_term' | 'diary'>('long_term');
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
  // Context stats
  const [contextStats, setContextStats] = useState<ContextStats | null>(null);
  // Token usage monitoring
  const [tokenUsage, setTokenUsage] = useState<TokenUsage | null>(null);
  // Settings state
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importPath, setImportPath] = useState('');
  const [settingsMsg, setSettingsMsg] = useState<string | null>(null);
  const [settingsErr, setSettingsErr] = useState<string | null>(null);
  const [execMode, setExecMode] = useState<'leashed' | 'free_run'>('leashed');
  const [membersExpanded, setMembersExpanded] = useState(true);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [toolsExpanded, setToolsExpanded] = useState(false);
  const [configExpanded, setConfigExpanded] = useState(false);
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

  // Track window maximized state for Linux custom titlebar button icon
  useEffect(() => {
    if (platform !== 'linux') return;
    const win = getCurrentWindow();
    win.isMaximized().then(setIsMaximized).catch(() => {});
    const unlisten = win.onResized(() => {
      win.isMaximized().then(setIsMaximized).catch(() => {});
    });
    return () => { unlisten.then((f) => f()); };
  }, [platform]);

  // Apply theme on first render
  React.useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    invoke<string>('get_execution_mode').then((m) => setExecMode(m as 'leashed' | 'free_run')).catch(() => {});
    loadPups();

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
    await invoke('approve_permission', { request_id: req.request_id, skill_name: req.skill_name, remember }).catch(() => {});
  };

  const handleDeny = async () => {
    if (!permissionRequest) return;
    const id = permissionRequest.request_id;
    setPermissionRequest(null);
    await invoke('deny_permission', { request_id: id }).catch(() => {});
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
    try { await invoke('import_workspace', { backup_path: importPath.trim() }); setSettingsMsg(t('settings_import_success', lang)); }
    catch (e: unknown) { setSettingsErr(String(e)); }
    finally { setImporting(false); }
  };

  if (onboardingDone === null) return (
    <div className="min-h-screen flex items-center justify-center" style={{ background: 'var(--color-background-secondary)' }}>
      <div className="flex flex-col items-center gap-3">
        <span className="text-[22px] font-medium select-none" style={{ color: 'var(--color-text-secondary)' }}>
          open<span style={{ color: '#1D9E75' }}>pup</span>
        </span>
        <div className="w-4 h-4 rounded-full border-2 border-t-transparent animate-spin" style={{ borderColor: '#1D9E75', borderTopColor: 'transparent' }} />
      </div>
    </div>
  );

  if (!onboardingDone) return (
    <Onboarding onComplete={() => {
      setOnboardingDone(true); loadMemoryChips();
      setMessages([{ id: 'welcome', role: 'assistant', content: t('onboarding_complete_message', lang), pup_key: 'alpha', pup_name: 'Alpha' }]);
    }} />
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

  const isPrimaryView = activeNav === 'chat' || activeNav === 'channel';
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

      {/* ── Custom overlay titlebar — adapts to macOS / Windows / Linux ── */}
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
          paddingRight: platform === 'windows' ? '140px' : (platform === 'linux' ? '0px' : '12px'),
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
        {/* ── Linux window control buttons (macOS has traffic lights, Windows has native overlay controls) ── */}
        {platform === 'linux' && (
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
        /* Expanded 176px sidebar */
        <div className="flex flex-col shrink-0 overflow-y-auto" style={{ width: '188px', borderRight: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
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

          <div className="flex flex-col flex-1 px-2 pt-3 pb-3 gap-4">
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
                onClick={() => setToolsExpanded(v => !v)}
                style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', width: '100%', padding: '0 8px 6px', fontSize: "10px", fontWeight: 500, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', background: 'none', border: 'none', cursor: 'pointer' }}
              >
                <span>{t('sidebar_tools', lang)}</span>
                <span style={{ fontSize: "10px", opacity: 0.6 }}>{toolsExpanded ? '▾' : '▸'}</span>
              </button>
              {toolsExpanded && (
                <>
                  <NavBtn label={t('nav_timeline', lang)} navKey="timeline" />
                  <NavBtn label={t('nav_memories', lang)} navKey="memories" />
                  <NavBtn label={t('nav_tasks', lang)} navKey="tasks" />
                </>
              )}
            </div>

            {/* Config section — collapsible */}
            <div>
              <button
                onClick={() => setConfigExpanded(v => !v)}
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

            <div className="flex-1" />

            {/* Mode pill + utilities */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
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
                  display: 'flex', alignItems: 'center', gap: '6px', width: '100%',
                  padding: '5px 10px', borderRadius: '6px', fontSize: "12px",
                  color: 'var(--color-text-secondary)', background: 'var(--color-background-secondary)',
                  border: 'none', cursor: 'pointer',
                }}
              >
                <span style={{ width: '7px', height: '7px', borderRadius: '50%', background: execMode === 'free_run' ? '#BA7517' : '#1D9E75', flexShrink: 0 }} />
                {execMode === 'leashed' ? t('mode_leashed', lang) : t('mode_free', lang)}
              </button>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '0 4px' }}>
                <button
                  onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
                  style={{ fontSize: "11px", color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '2px 4px' }}
                >
                  {t('settings_lang_toggle_button', lang)}
                </button>
                {/* Theme toggle: sun/moon icon */}
                <button
                  onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
                  title={theme === 'dark' ? t('theme_light', lang) : t('theme_dark', lang)}
                  style={{ color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '2px 4px', lineHeight: 1 }}
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
                  style={{ color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '2px 4px' }}
                  title="GitHub"
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M12 2C6.477 2 2 6.477 2 12c0 4.418 2.865 8.166 6.839 9.489.5.092.682-.217.682-.482 0-.237-.009-.868-.013-1.703-2.782.604-3.369-1.341-3.369-1.341-.454-1.155-1.11-1.462-1.11-1.462-.908-.62.069-.608.069-.608 1.003.07 1.531 1.03 1.531 1.03.892 1.529 2.341 1.087 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.11-4.555-4.943 0-1.091.39-1.984 1.029-2.683-.103-.253-.446-1.27.098-2.647 0 0 .84-.269 2.75 1.025A9.578 9.578 0 0 1 12 6.836a9.59 9.59 0 0 1 2.504.337c1.909-1.294 2.747-1.025 2.747-1.025.546 1.377.202 2.394.1 2.647.64.699 1.028 1.592 1.028 2.683 0 3.842-2.339 4.687-4.566 4.935.359.309.678.919.678 1.852 0 1.336-.012 2.415-.012 2.743 0 .267.18.578.688.48C19.138 20.163 22 16.418 22 12c0-5.523-4.477-10-10-10z"/>
                  </svg>
                </button>
                <button
                  onClick={() => setSidebarCollapsed(true)}
                  style={{ color: 'var(--color-text-tertiary)', cursor: 'pointer', background: 'none', border: 'none', padding: '2px 4px', lineHeight: 1 }}
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
                      <div key={m.id} className={isNew ? 'animate-msg-in' : ''} style={{ maxWidth: '80%' }}>
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
