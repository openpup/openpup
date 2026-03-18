import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface McpServer {
  name: string;
  base_url: string;
  description: string;
  enabled: boolean;
}

interface McpToolInfo {
  server: string;
  name: string;
  description: string;
}

export const McpSettings: React.FC = () => {
  const { lang } = useLang();
  const [servers, setServers] = useState<McpServer[]>([]);
  const [tools, setTools] = useState<McpToolInfo[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [token, setToken] = useState('');
  const [description, setDescription] = useState('');
  const [adding, setAdding] = useState(false);

  const INPUT = 'rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50';

  const load = () =>
    invoke<McpServer[]>('list_mcp_servers').then(setServers).catch((e) => setError(String(e)));

  const loadTools = () =>
    invoke<McpToolInfo[]>('list_mcp_tools').then(setTools).catch(() => {});

  useEffect(() => {
    void load();
    void loadTools();
  }, []);

  const refreshTools = async () => {
    setRefreshing(true);
    setError(null);
    try {
      const toolList = await invoke<McpToolInfo[]>('refresh_mcp_tools');
      setTools(toolList);
    } catch (e) { setError(String(e)); }
    finally { setRefreshing(false); }
  };

  const add = async () => {
    if (!name.trim() || !baseUrl.trim()) return;
    setAdding(true);
    setError(null);
    try {
      await invoke('add_mcp_server', {
        entry: { name: name.trim(), base_url: baseUrl.trim(), token: token.trim(), description: description.trim() },
      });
      setName(''); setBaseUrl(''); setToken(''); setDescription('');
      await load();
    } catch (e) { setError(String(e)); }
    finally { setAdding(false); }
  };

  const remove = async (n: string) => {
    if (!window.confirm(`删除 MCP 服务器 "${n}"？`)) return;
    await invoke('remove_mcp_server', { name: n }).catch((e) => setError(String(e)));
    await load();
  };

  const toggle = async (n: string, enabled: boolean) => {
    await invoke('toggle_mcp_server', { name: n, enabled }).catch((e) => setError(String(e)));
    setServers((prev) => prev.map((s) => s.name === n ? { ...s, enabled } : s));
  };

  return (
    <div className="space-y-4 text-xs">
      <h3 className="text-sm font-semibold text-stone-100">{t('mcp_title', lang)}</h3>
      {error && <p className="text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{error}</p>}

      {/* Built-in local server */}
      <div className="flex items-center justify-between rounded-xl border border-stone-800 bg-stone-900/40 px-4 py-3">
        <div>
          <span className="font-medium text-stone-200">local</span>
          <span className="ml-2 text-stone-500">{t('mcp_builtin', lang)} (ping / read_file / write_file / open_browser)</span>
        </div>
        <span className="text-emerald-400 text-[11px] font-medium">{t('mcp_always_on', lang)}</span>
      </div>

      {/* Dynamic servers */}
      {servers.map((s) => (
        <div key={s.name} className="flex items-center justify-between rounded-xl border border-stone-800 bg-stone-900/40 px-4 py-3 gap-2 hover:border-stone-700 transition-colors">
          <div className="min-w-0 flex-1">
            <div className="font-medium text-stone-200">{s.name}</div>
            <div className="text-stone-500 truncate">{s.base_url}{s.description ? ` — ${s.description}` : ''}</div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              onClick={() => void toggle(s.name, !s.enabled)}
              className={`px-2 py-1 rounded-lg text-[11px] transition-colors ${s.enabled ? 'bg-emerald-900/60 text-emerald-300 hover:bg-emerald-900/80' : 'bg-stone-700 text-stone-300 hover:bg-stone-600'}`}
            >
              {s.enabled ? t('mcp_enabled', lang) : t('mcp_disabled', lang)}
            </button>
            <button onClick={() => void remove(s.name)} className="px-2 py-1 rounded-lg bg-red-900/40 text-red-400 text-[11px] hover:bg-red-900/60 transition-colors">
              {t('mcp_delete', lang)}
            </button>
          </div>
        </div>
      ))}

      {/* Add form */}
      <div className="rounded-xl border border-stone-700 bg-stone-900/40 px-4 py-3 space-y-2.5">
        <div className="font-medium text-stone-300 mb-1">{t('mcp_add_title', lang)}</div>
        <div className="grid grid-cols-2 gap-2">
          <input
            className={INPUT}
            placeholder={t('mcp_name_ph', lang)}
            value={name} onChange={(e) => setName(e.target.value)}
          />
          <input
            className={INPUT}
            placeholder={t('mcp_url_ph', lang)}
            value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)}
          />
          <input
            className={INPUT}
            placeholder={t('mcp_token_ph', lang)}
            type="password"
            value={token} onChange={(e) => setToken(e.target.value)}
          />
          <input
            className={INPUT}
            placeholder={t('mcp_desc_ph', lang)}
            value={description} onChange={(e) => setDescription(e.target.value)}
          />
        </div>
        <button
          className="px-3 py-2 rounded-lg bg-amber-500 text-stone-950 text-xs font-medium disabled:opacity-50 hover:bg-amber-400 transition-colors mt-1"
          disabled={adding || !name.trim() || !baseUrl.trim()}
          onClick={() => void add()}
        >
          {adding ? t('mcp_adding', lang) : t('mcp_add_btn', lang)}
        </button>
      </div>

      {/* Discovered tools */}
      <div className="border-t border-stone-800 pt-3">
        <div className="flex items-center justify-between mb-2">
          <span className="font-semibold text-stone-200">{t('mcp_tools_title', lang)} ({tools.length})</span>
          <button
            onClick={() => void refreshTools()}
            disabled={refreshing}
            className="px-3 py-1.5 rounded-lg bg-stone-800 border border-stone-700 text-stone-300 text-xs hover:bg-stone-700 disabled:opacity-50 transition-colors"
          >
            {refreshing ? t('mcp_refreshing', lang) : t('mcp_refresh', lang)}
          </button>
        </div>
        {tools.length === 0 ? (
          <p className="text-stone-600">{t('mcp_tools_empty', lang)}</p>
        ) : (
          <div className="space-y-1 max-h-60 overflow-auto">
            {tools.map((tool) => (
              <div
                key={`${tool.server}:${tool.name}`}
                className="flex items-start gap-2 rounded-lg px-3 py-2 bg-stone-800/50"
              >
                <span className="text-stone-500 shrink-0 mt-0.5">[{tool.server}]</span>
                <div className="min-w-0">
                  <span className="font-medium text-stone-200">{tool.name}</span>
                  {tool.description && (
                    <span className="text-stone-500 ml-1.5">{tool.description}</span>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
