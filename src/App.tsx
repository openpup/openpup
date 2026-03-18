import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { MarkdownRenderer } from './components/MarkdownRenderer';
import { Onboarding } from './components/Onboarding';
import { SkillStore } from './components/SkillStore';
import { MemoryManager } from './components/MemoryManager';
import { Timeline } from './components/Timeline';
import { DiaryViewer } from './components/DiaryViewer';
import { McpSettings } from './components/McpSettings';
import { PupManager } from './components/PupManager';
import { TaskManager } from './components/TaskManager';
import { PermissionDialog, PermissionRequest } from './components/PermissionDialog';
import { PackChannel } from './components/PackChannel';
import { LangProvider, useLang, t } from './i18n';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  pup_name?: string;
}

interface StreamDonePayload {
  pup_name: string;
  content: string;
}

interface MemoryChip {
  content: string;
  memory_type: string;
  importance: number;
}

interface SkillSuggestion {
  skill_name: string;
  repo_url: string;
  reason: string;
}

interface ActivityStep {
  kind: 'routing' | 'skill' | 'tool_call' | string;
  label: string;
}


interface PupConfig {
  key: string;
  display_name: string;
  description: string;
  enabled: boolean;
  is_custom: boolean;
}

type NavItem = 'chat' | 'channel' | 'memories' | 'timeline' | 'skills' | 'pups' | 'tasks' | 'mcp' | 'settings';

const PUP_DOT: Record<string, string> = {
  alpha: 'bg-emerald-500',
  dev: 'bg-sky-500',
  writer: 'bg-amber-400',
  ops: 'bg-stone-400',
  research: 'bg-purple-500',
  life_admin: 'bg-pink-500',
};

const PUP_TAG: Record<string, string> = {
  Alpha: 'bg-emerald-900/60 text-emerald-300',
  'Dev Pup': 'bg-sky-900/60 text-sky-300',
  'Writer Pup': 'bg-amber-900/60 text-amber-300',
  'Ops Pup': 'bg-stone-700 text-stone-300',
  'Research Pup': 'bg-purple-900/60 text-purple-300',
  'Life Admin Pup': 'bg-pink-900/60 text-pink-300',
};

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
  ollama:       { label: 'Ollama (本地)', api_base: 'http://localhost:11434/v1',     model: 'llama3',                   mini_model: 'llama3',                embed_model: 'nomic-embed-text' },
  deepseek:     { label: 'DeepSeek',      api_base: 'https://api.deepseek.com/v1',   model: 'deepseek-chat',            mini_model: 'deepseek-chat',         embed_model: '' },
  openrouter:   { label: 'OpenRouter',    api_base: 'https://openrouter.ai/api/v1',  model: 'anthropic/claude-3.5-sonnet', mini_model: 'openai/gpt-4o-mini', embed_model: '' },
};

const LlmConfigPanel: React.FC = () => {
  const [cfg, setCfg] = React.useState<LlmConfigInfo | null>(null);
  const [form, setForm] = React.useState({ provider: 'openai', model: '', miniModel: '', embedModel: '', apiKey: '', apiBase: '' });
  const [saving, setSaving] = React.useState(false);
  const [msg, setMsg] = React.useState<string | null>(null);

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
      setMsg('已保存');
      setTimeout(() => setMsg(null), 2000);
    } catch (e) {
      setMsg(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const field = (label: string, value: string, key: keyof typeof form, ph?: string, type = 'text') => (
    <div className="flex items-center gap-2">
      <label className="w-24 shrink-0 text-stone-500">{label}</label>
      <input
        type={type}
        className="flex-1 rounded-lg bg-stone-800 border border-stone-700 px-2.5 py-1.5 text-xs text-stone-100 placeholder:text-stone-600 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
        value={value}
        placeholder={ph}
        onChange={(e) => setForm((f) => ({ ...f, [key]: e.target.value }))}
      />
    </div>
  );

  return (
    <section>
      <h2 className="text-sm font-semibold mb-3 text-stone-200">模型配置</h2>

      {/* Provider presets */}
      <div className="flex flex-wrap gap-1.5 mb-3">
        {Object.entries(PROVIDER_PRESETS).map(([k, p]) => (
          <button key={k} onClick={() => applyPreset(k)}
            className={`text-[11px] px-2.5 py-1 rounded-full border transition-colors ${
              form.provider === (k === 'ollama' ? 'ollama' : 'openai') && (form.apiBase || '') === p.api_base
                ? 'border-amber-500 text-amber-400 bg-amber-500/10'
                : 'border-stone-700 text-stone-500 hover:border-stone-600 hover:text-stone-400'
            }`}>{p.label}</button>
        ))}
      </div>

      <div className="space-y-2">
        {field('主模型', form.model, 'model', 'gpt-4o / deepseek-chat')}
        {field('Mini 模型', form.miniModel, 'miniModel', '用于意图分类（便宜快速）')}
        {field('嵌入模型', form.embedModel, 'embedModel', 'text-embedding-3-small / BAAI/bge-m3')}
        {field('API Base', form.apiBase, 'apiBase', '留空使用 OpenAI 默认')}
        {field('API Key', form.apiKey, 'apiKey', '留空保持现有密钥', 'password')}
      </div>

      <div className="mt-2.5 text-[10px] text-stone-600 space-y-0.5">
        <div>· Mini 模型：意图分类、记忆提取——用便宜模型降低成本</div>
        <div>· 嵌入模型：长期记忆语义检索——需支持 /embeddings 端点</div>
      </div>

      <div className="flex items-center gap-3 mt-3">
        <button
          onClick={() => void save()} disabled={saving}
          className="px-4 py-1.5 rounded-lg bg-amber-500 text-stone-950 text-xs font-medium hover:bg-amber-400 disabled:opacity-40 transition-colors"
        >
          {saving ? '保存中…' : '保存'}
        </button>
        {msg && <span className={`text-xs ${msg.startsWith('保存失败') ? 'text-red-400' : 'text-emerald-400'}`}>{msg}</span>}
        {cfg && <span className="text-[10px] text-stone-600 ml-auto">当前: {cfg.model}</span>}
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
const AppInner: React.FC = () => {
  const { lang, setLang } = useLang();
  const [theme, setTheme] = React.useState<'dark' | 'light'>(() => {
    const saved = localStorage.getItem('openpup_theme');
    return (saved === 'light' ? 'light' : 'dark');
  });
  const [onboardingDone, setOnboardingDone] = useState<boolean | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingContent, setStreamingContent] = useState('');
  const [streamingReasoningContent, setStreamingReasoningContent] = useState('');
  const [streamingPupName, setStreamingPupName] = useState('');
  const [streamingSteps, setStreamingSteps] = useState<ActivityStep[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [activeNav, setActiveNav] = useState<NavItem>('chat');
  const activeNavRef = useRef<string>('chat');
  useEffect(() => { activeNavRef.current = activeNav; }, [activeNav]);
  const [memoryChips, setMemoryChips] = useState<MemoryChip[]>([]);
  const [permissionRequest, setPermissionRequest] = useState<PermissionRequest | null>(null);
  const [skillSuggestion, setSkillSuggestion] = useState<SkillSuggestion | null>(null);
  const [pups, setPups] = useState<PupConfig[]>([]);
  const [selectedPupKey, setSelectedPupKey] = useState<string>('alpha');
  const [memoriesTab, setMemoriesTab] = useState<'long_term' | 'diary'>('long_term');
  // Settings state
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importPath, setImportPath] = useState('');
  const [settingsMsg, setSettingsMsg] = useState<string | null>(null);
  const [settingsErr, setSettingsErr] = useState<string | null>(null);
  const [execMode, setExecMode] = useState<'leashed' | 'free_run'>('leashed');
  const [membersExpanded, setMembersExpanded] = useState(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

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
          setMessages([{ id: 'welcome', role: 'assistant', content: t('chat_welcome', lang), pup_name: 'Alpha' }]);
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
      listen<SkillSuggestion>('skill_suggestion', (e) => setSkillSuggestion(e.payload)),
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
        const { pup_name, content } = e.payload;
        // Use backend authoritative content — immune to token ordering issues.
        streamingContentRef.current = '';
        setStreamingContent('');
        setStreamingReasoningContent('');
        setStreamingPupName('');
        setStreamingSteps([]);
        setSending(false);
        if (content && activeNavRef.current !== 'channel') {
          setMessages((msgs) => [
            ...msgs,
            { id: crypto.randomUUID(), role: 'assistant', content, pup_name },
          ]);
          void loadMemoryChips();
        }
      }),
      listen<string>('stream_error', (e) => {
        setMessages((msgs) => [
          ...msgs,
          { id: crypto.randomUUID(), role: 'assistant', content: `⚠️ ${e.payload}`, pup_name: 'Alpha' },
        ]);
        setStreamingContent('');
        setStreamingReasoningContent('');
        setStreamingPupName('');
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

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [messages, streamingContent]);

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
    streamingContentRef.current = '';
    setStreamingContent('');
    setStreamingReasoningContent('');
    setStreamingSteps([]);
    setStreamingPupName('Alpha');
    setSending(true);
    const forcedPup = null; // @mention routing handled by backend
    await invoke('send_message', { input: trimmed, forcedPup }).catch((e: unknown) => {
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: 'assistant', content: `⚠️ ${String(e)}`, pup_name: 'Alpha' }]);
      setStreamingContent('');
      setStreamingReasoningContent('');
      setStreamingPupName('');
      setSending(false);
    });
  };

  const abort = async () => {
    // Reset UI immediately — don't wait for the backend stream_done event.
    streamingContentRef.current = '';
    setStreamingContent('');
    setStreamingReasoningContent('');
    setStreamingPupName('');
    setSending(false);
    await invoke('abort_message').catch(() => {});
  };

  const onKeyDown: React.KeyboardEventHandler<HTMLTextAreaElement> = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); }
  };

  const exportWorkspace = async () => {
    setExporting(true); setSettingsErr(null); setSettingsMsg(null);
    try { setSettingsMsg(`已导出到：${await invoke<string>('export_workspace')}`); }
    catch (e: unknown) { setSettingsErr(String(e)); }
    finally { setExporting(false); }
  };

  const importWorkspace = async () => {
    if (!importPath.trim()) { setSettingsErr('请填写备份文件路径'); return; }
    setImporting(true); setSettingsErr(null); setSettingsMsg(null);
    try { await invoke('import_workspace', { backup_path: importPath.trim() }); setSettingsMsg('导入成功，请重启应用。'); }
    catch (e: unknown) { setSettingsErr(String(e)); }
    finally { setImporting(false); }
  };

  if (onboardingDone === null) return (
    <div className="min-h-screen bg-stone-950 flex items-center justify-center">
      <div className="w-5 h-5 rounded-full border-2 border-amber-500 border-t-transparent animate-spin" />
    </div>
  );

  if (!onboardingDone) return (
    <Onboarding onComplete={() => {
      setOnboardingDone(true); loadMemoryChips();
      setMessages([{ id: 'welcome', role: 'assistant', content: '好了，我对你有了初步了解 🐾 这些都写在 ~/.openpup/OWNER.md 里，随时可以修改。让我们开始吧——今天想先做什么？', pup_name: 'Alpha' }]);
    }} />
  );

  const NavBtn: React.FC<{ label: string; navKey: NavItem; onClick?: () => void }> = ({ label, navKey, onClick }) => (
    <button
      onClick={() => { setActiveNav(navKey); onClick?.(); }}
      className={`w-full text-left px-3 py-1.5 rounded-md text-xs font-medium transition-all flex items-center gap-2 ${
        activeNav === navKey
          ? 'bg-stone-800 text-stone-100 border-l-2 border-amber-500 pl-[10px]'
          : 'text-stone-400 hover:text-stone-300 hover:bg-stone-900/60 border-l-2 border-transparent pl-[10px]'
      }`}
    >
      {label}
    </button>
  );

  return (
    <div className="h-screen flex flex-col bg-stone-950 text-stone-100 overflow-hidden">
      {/* Header */}
      <header className="bg-stone-900 border-b border-stone-800 px-4 py-2.5 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2.5">
          <span className="text-lg leading-none">🐾</span>
          <div className="flex flex-col leading-none">
            <span className="font-bold tracking-tight text-sm text-stone-100">OpenPup</span>
            <span className="text-[10px] text-stone-500 tracking-wide mt-0.5">"用 ChatGPT 三个月，它还是不知道你不喜欢被打断。OpenPup 记得。"</span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {/* GitHub link */}
          <button
            onClick={() => invoke('open_url', { url: 'https://github.com/openpup/openpup' })}
            className="text-stone-500 hover:text-stone-300 transition-colors cursor-pointer"
            title="GitHub"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 2C6.477 2 2 6.477 2 12c0 4.418 2.865 8.166 6.839 9.489.5.092.682-.217.682-.482 0-.237-.009-.868-.013-1.703-2.782.604-3.369-1.341-3.369-1.341-.454-1.155-1.11-1.462-1.11-1.462-.908-.62.069-.608.069-.608 1.003.07 1.531 1.03 1.531 1.03.892 1.529 2.341 1.087 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.11-4.555-4.943 0-1.091.39-1.984 1.029-2.683-.103-.253-.446-1.27.098-2.647 0 0 .84-.269 2.75 1.025A9.578 9.578 0 0 1 12 6.836a9.59 9.59 0 0 1 2.504.337c1.909-1.294 2.747-1.025 2.747-1.025.546 1.377.202 2.394.1 2.647.64.699 1.028 1.592 1.028 2.683 0 3.842-2.339 4.687-4.566 4.935.359.309.678.919.678 1.852 0 1.336-.012 2.415-.012 2.743 0 .267.18.578.688.48C19.138 20.163 22 16.418 22 12c0-5.523-4.477-10-10-10z"/>
            </svg>
          </button>
          {/* Execution mode badge */}
          <span className={`text-xs px-2.5 py-1 rounded-full font-medium ${
            execMode === 'free_run'
              ? 'bg-emerald-900/60 text-emerald-400'
              : 'bg-stone-800 text-stone-400'
          }`}>
            {execMode === 'leashed' ? t('mode_leashed', lang) : t('mode_free', lang)}
          </span>
          {/* Language toggle */}
          <button
            onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
            className="text-xs px-2 py-1 rounded bg-stone-800 text-stone-400 hover:text-stone-300 border border-stone-700 transition-colors"
          >
            {lang === 'zh' ? 'EN' : '中'}
          </button>
        </div>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <div className="w-38 border-r border-stone-800 flex flex-col px-2 py-3 shrink-0 bg-stone-900/50 overflow-y-auto" style={{ width: '152px' }}>
          {/* Pack group — collapsible pup list */}
          <button
            className="flex items-center justify-between px-3 mb-1 w-full group"
            onClick={() => setMembersExpanded((v) => !v)}
          >
            <span className="text-[10px] text-stone-600 uppercase tracking-widest font-semibold">
              {t('pup_section', lang)}
            </span>
            <span className="text-[10px] text-stone-700 group-hover:text-stone-500 transition-colors">
              {membersExpanded ? '▾' : '▸'}
            </span>
          </button>
          {membersExpanded && (
            <>
              {/* Alpha — navigate to chat, no @mention */}
              <button
                onClick={() => { setActiveNav('chat'); inputRef.current?.focus(); }}
                className={`flex items-center gap-2.5 px-3 py-1.5 rounded-lg text-xs mb-0.5 transition-all font-medium ${
                  activeNav === 'chat' && selectedPupKey === 'alpha'
                    ? 'bg-stone-800 text-stone-100 shadow-sm'
                    : 'text-stone-400 hover:text-stone-300 hover:bg-stone-800/60'
                }`}
              >
                <span className="w-2 h-2 rounded-full shrink-0 bg-emerald-500 shadow-sm shadow-emerald-500/50" />
                Alpha
              </button>
              {pups.filter((p) => p.key !== 'alpha').map((pup) => (
                <button
                  key={pup.key}
                  onClick={() => { setActiveNav('chat'); insertMention(pup.key); }}
                  className="flex items-center gap-2.5 px-3 py-1.5 rounded-lg text-xs mb-0.5 transition-all font-medium text-stone-400 hover:text-stone-300 hover:bg-stone-800/60"
                >
                  <span className={`w-2 h-2 rounded-full shrink-0 ${PUP_DOT[pup.key] ?? 'bg-violet-500'}`} />
                  <span className="truncate">{pup.display_name}</span>
                </button>
              ))}
            </>
          )}

          <div className="border-t border-stone-800 my-2.5 mx-2" />

          {/* Nav */}
          <div className="space-y-0.5">
            <NavBtn label={t('nav_pack_channel', lang)} navKey="channel" />
            <NavBtn label={t('nav_memories', lang)} navKey="memories" />
            <NavBtn label={t('nav_timeline', lang)} navKey="timeline" />
            <NavBtn label={t('nav_tasks', lang)} navKey="tasks" />
            <NavBtn label={t('nav_skills', lang)} navKey="skills" />
            <NavBtn label={t('nav_pups', lang)} navKey="pups" onClick={loadPups} />
            <NavBtn label={t('nav_mcp', lang)} navKey="mcp" />
            <NavBtn label={t('nav_settings', lang)} navKey="settings" />
          </div>
        </div>

        {/* Main content */}
        <div className="flex-1 flex flex-col overflow-hidden">

          {/* ── Chat ── */}
          {activeNav === 'chat' && (
            <>
              {/* Memory chips */}
              {memoryChips.length > 0 && (
                <div className="border-b border-stone-800/60 px-4 py-2 flex items-center gap-2 bg-stone-900/30">
                  <span className="text-[10px] text-stone-600 shrink-0 font-medium">记忆</span>
                  <div className="flex gap-1.5 flex-wrap flex-1 min-w-0">
                    {memoryChips.slice(0, 4).map((chip, i) => (
                      <span key={i} className="inline-flex items-center gap-1 text-[11px] px-2.5 py-1 rounded-full bg-stone-800/80 text-stone-300 border border-stone-700/50 hover:border-stone-600 transition-colors">
                        <span className="shrink-0">{memoryTypeIcon(chip.memory_type)}</span>
                        <span className="truncate max-w-[140px]">{chip.content}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {/* Skill suggestion */}
              {skillSuggestion && (
                <div className="border-b border-amber-900/40 bg-amber-950/30 px-4 py-2.5 flex items-start gap-3">
                  <span className="text-lg shrink-0">⚡</span>
                  <div className="flex-1 min-w-0 text-xs">
                    <span className="font-semibold text-amber-300">{skillSuggestion.skill_name}</span>
                    <span className="text-stone-400 ml-1.5">— {skillSuggestion.reason}</span>
                  </div>
                  <div className="flex gap-1.5 shrink-0">
                    <button className="px-2.5 py-1 rounded-lg bg-amber-500 text-stone-950 text-xs font-medium"
                      onClick={async () => { try { await invoke('install_skill_from_git', { repoUrl: skillSuggestion.repo_url, subdir: null }); setSkillSuggestion(null); } catch {} }}>
                      {t('skill_install_suggestion', lang)}
                    </button>
                    <button className="px-2.5 py-1 rounded-lg bg-stone-800 text-stone-400 text-xs"
                      onClick={() => setSkillSuggestion(null)}>
                      {t('skill_dismiss_suggestion', lang)}
                    </button>
                  </div>
                </div>
              )}

              {/* Messages */}
              <div className="flex-1 overflow-auto px-5 py-4 space-y-4">
                {messages.map((m) =>
                  m.role === 'user' ? (
                    <div key={m.id} className="flex justify-end">
                      <div className="max-w-sm bg-amber-700 text-white text-sm px-4 py-2.5 rounded-2xl rounded-tr-sm shadow-sm">
                        {m.content}
                      </div>
                    </div>
                  ) : (
                    <div key={m.id} className="flex flex-col gap-1.5 max-w-[32rem]">
                      {m.pup_name && (
                        <span className={`self-start text-[11px] px-2.5 py-0.5 rounded-full font-medium ${PUP_TAG[m.pup_name] ?? 'bg-stone-700 text-stone-300'}`}>
                          {m.pup_name}
                        </span>
                      )}
                      <div className="bg-stone-800 text-stone-100 text-sm px-4 py-2.5 rounded-2xl rounded-tl-sm shadow-sm prose prose-invert prose-sm max-w-none">
                        <MarkdownRenderer>{m.content}</MarkdownRenderer>
                      </div>
                    </div>
                  )
                )}
                {sending && (
                  <div className="flex flex-col gap-1.5 max-w-[32rem]">
                    <span className={`self-start text-[11px] px-2.5 py-0.5 rounded-full font-medium ${PUP_TAG[streamingPupName] ?? 'bg-emerald-900/60 text-emerald-300'}`}>
                      {streamingPupName || 'Alpha'}
                    </span>
                    {streamingSteps.length > 0 && (
                      <div className="flex flex-col gap-0.5">
                        {streamingSteps.map((step, i) => (
                          <div key={i} className="flex items-center gap-1.5 text-[11px] text-stone-500">
                            <span className="shrink-0">
                              {step.kind === 'routing' ? '→' : step.kind === 'skill' ? '⚡' : '⚙'}
                            </span>
                            <span className={step.kind === 'routing' ? 'text-stone-400' : 'font-mono text-stone-500'}>
                              {step.label}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                    {streamingReasoningContent && (
                      <div className="bg-stone-900/60 border border-stone-700/50 text-stone-400 text-xs px-3 py-2 rounded-xl italic leading-relaxed max-h-24 overflow-y-auto">
                        <span className="text-stone-500 not-italic mr-1">thinking…</span>
                        {streamingReasoningContent}
                      </div>
                    )}
                    <div className="bg-stone-800 text-stone-100 text-sm px-4 py-2.5 rounded-2xl rounded-tl-sm shadow-sm prose prose-invert prose-sm max-w-none">
                      {streamingContent
                        ? <MarkdownRenderer>{streamingContent}</MarkdownRenderer>
                        : <span className="text-stone-500 animate-pulse">{t('chat_thinking', lang)}</span>
                      }
                    </div>
                  </div>
                )}
                <div ref={messagesEndRef} />
              </div>

              {/* Input */}
              <div className="border-t border-stone-800 px-4 py-3 flex gap-2.5 shrink-0 bg-stone-900/40">
                <textarea
                  ref={inputRef}
                  className="flex-1 resize-none rounded-xl bg-stone-800 border border-stone-700 px-3.5 py-2.5 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50 transition-colors"
                  rows={2}
                  placeholder={t('chat_placeholder_alpha', lang)}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={onKeyDown}
                />
                {sending ? (
                  <button className="self-end px-4 py-2.5 rounded-xl bg-red-900/60 text-red-400 text-sm font-medium hover:bg-red-900/80 transition-colors border border-red-800/50"
                    onClick={() => void abort()}>■</button>
                ) : (
                  <button className="self-end px-4 py-2.5 rounded-xl bg-amber-500 text-stone-950 text-sm font-medium hover:bg-amber-400 disabled:opacity-40 transition-colors shadow-sm shadow-amber-500/20"
                    onClick={() => void send()} disabled={!input.trim()}>↑</button>
                )}
              </div>
            </>
          )}

          {/* ── Memories ── */}
          {activeNav === 'memories' && (
            <div className="flex-1 overflow-hidden flex flex-col px-5 py-4">
              <div className="flex items-center gap-1 mb-4 shrink-0">
                {(['long_term', 'diary'] as const).map((tab) => (
                  <button key={tab} onClick={() => setMemoriesTab(tab)}
                    className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${memoriesTab === tab ? 'bg-stone-800 text-stone-100' : 'text-stone-400 hover:text-stone-300'}`}>
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
              <PackChannel />
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
            <div className="flex-1 overflow-auto px-5 py-4"><SkillStore /></div>
          )}

          {/* ── Pups ── */}
          {activeNav === 'pups' && (
            <div className="flex-1 overflow-auto px-5 py-4"><PupManager /></div>
          )}

          {/* ── MCP ── */}
          {activeNav === 'mcp' && (
            <div className="flex-1 overflow-auto px-5 py-4"><McpSettings /></div>
          )}

          {/* ── Settings ── */}
          {activeNav === 'settings' && (
            <div className="flex-1 overflow-auto px-5 py-4 space-y-6 text-xs">

              {/* LLM Config */}
              <LlmConfigPanel />

              <div className="border-t border-stone-800" />

              <section>
                <h2 className="text-sm font-semibold mb-3 text-stone-200">{t('settings_exec', lang)}</h2>
                <p className="text-stone-500 mb-3 leading-relaxed">
                  <span className="text-stone-300 font-medium">{t('settings_leashed', lang)}</span>：{t('settings_leashed_note', lang)} &nbsp;·&nbsp;
                  <span className="text-stone-300 font-medium">{t('settings_free', lang)}</span>：{t('settings_free_note', lang)}
                </p>
                <div className="flex items-center gap-3">
                  <span className={execMode === 'leashed' ? 'text-amber-400 font-medium' : 'text-stone-500'}>
                    {t('settings_leashed', lang)}
                  </span>
                  <button className={`relative w-11 h-5.5 rounded-full transition-colors ${execMode === 'free_run' ? 'bg-emerald-500' : 'bg-stone-600'}`}
                    style={{ height: '22px', width: '44px' }}
                    onClick={async () => {
                      const next = execMode === 'leashed' ? 'free_run' : 'leashed';
                      await invoke('set_execution_mode', { mode: next }).catch(() => {});
                      setExecMode(next);
                    }}>
                    <span className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform ${execMode === 'free_run' ? 'translate-x-6' : 'translate-x-0.5'}`} />
                  </button>
                  <span className={execMode === 'free_run' ? 'text-emerald-400 font-medium' : 'text-stone-500'}>
                    {t('settings_free', lang)}
                  </span>
                </div>
              </section>

              <div className="border-t border-stone-800" />

              <section>
                <h2 className="text-sm font-semibold mb-3 text-stone-200">{t('settings_theme', lang)}</h2>
                <div className="flex gap-3">
                  {/* Dark – GitHub style */}
                  <button
                    onClick={() => setTheme('dark')}
                    className={`flex flex-col items-center gap-2 px-4 py-3 rounded-xl border text-xs font-medium transition-all ${
                      theme === 'dark'
                        ? 'border-amber-500 text-amber-400 bg-amber-500/10'
                        : 'border-stone-700 text-stone-400 hover:border-stone-600'
                    }`}
                  >
                    <span className="w-10 h-6 rounded-md overflow-hidden border border-stone-600 flex">
                      <span className="w-3 h-full bg-[#161b22]" />
                      <span className="flex-1 h-full bg-[#0d1117]" />
                    </span>
                    {lang === 'zh' ? '深色' : 'Dark'}
                  </button>

                  {/* Light – warm */}
                  <button
                    onClick={() => setTheme('light')}
                    className={`flex flex-col items-center gap-2 px-4 py-3 rounded-xl border text-xs font-medium transition-all ${
                      theme === 'light'
                        ? 'border-amber-500 text-amber-400 bg-amber-500/10'
                        : 'border-stone-700 text-stone-400 hover:border-stone-600'
                    }`}
                  >
                    <span className="w-10 h-6 rounded-md overflow-hidden border border-stone-600 flex">
                      <span className="w-3 h-full bg-[#f6f8fa]" />
                      <span className="flex-1 h-full bg-[#ffffff]" />
                    </span>
                    {lang === 'zh' ? '浅色' : 'Light'}
                  </button>
                </div>
              </section>

              <div className="border-t border-stone-800" />

              <section>
                <h2 className="text-sm font-semibold mb-3 text-stone-200">{t('settings_backup', lang)}</h2>
                <button className="px-4 py-2 rounded-lg bg-stone-800 border border-stone-700 text-stone-300 text-xs font-medium hover:bg-stone-700 disabled:opacity-40 transition-colors"
                  onClick={() => void exportWorkspace()} disabled={exporting}>
                  {exporting ? t('settings_exporting', lang) : t('settings_export', lang)}
                </button>
              </section>

              <section>
                <h2 className="text-sm font-semibold mb-3 text-stone-200">{t('settings_restore', lang)}</h2>
                <input className="w-full rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 mb-2.5 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
                  placeholder={t('settings_restore_ph', lang)} value={importPath} onChange={(e) => setImportPath(e.target.value)} />
                <button className="px-4 py-2 rounded-lg bg-emerald-600 text-white text-xs font-medium disabled:opacity-40 hover:bg-emerald-500 transition-colors"
                  onClick={() => void importWorkspace()} disabled={importing}>
                  {importing ? t('settings_importing', lang) : t('settings_import', lang)}
                </button>
              </section>

              {settingsErr && <p className="text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{settingsErr}</p>}
              {settingsMsg && <p className="text-emerald-400 bg-emerald-900/20 px-3 py-2 rounded-lg">{settingsMsg}</p>}
            </div>
          )}
        </div>
      </div>

      {permissionRequest && (
        <PermissionDialog request={permissionRequest}
          onApprove={(remember) => void handleApprove(remember)}
          onDeny={() => void handleDeny()} />
      )}
    </div>
  );
};

export const App: React.FC = () => (
  <LangProvider>
    <AppInner />
  </LangProvider>
);
