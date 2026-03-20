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

const inputStyle: React.CSSProperties = {
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '6px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
  width: '100%',
};

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
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

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
    await invoke('remove_mcp_server', { name: n }).catch((e) => setError(String(e)));
    setConfirmDelete(null);
    await load();
  };

  const toggle = async (n: string, enabled: boolean) => {
    await invoke('toggle_mcp_server', { name: n, enabled }).catch((e) => setError(String(e)));
    setServers((prev) => prev.map((s) => s.name === n ? { ...s, enabled } : s));
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, fontSize: 12 }}>
      <h3 style={{ fontSize: 14, fontWeight: 500, color: 'var(--color-text-primary)', margin: 0 }}>
        {t('mcp_title', lang)}
      </h3>
      {error && (
        <p style={{
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '6px 12px',
          borderRadius: 8,
          margin: 0,
        }}>
          {error}
        </p>
      )}

      {/* Built-in local server */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        borderRadius: 12,
        border: '1px solid var(--color-border-tertiary)',
        background: 'var(--color-background-secondary)',
        padding: '12px 16px',
      }}>
        <div>
          <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>local</span>
          <span style={{
            marginLeft: 8,
            fontSize: 10,
            padding: '1px 6px',
            borderRadius: 999,
            background: 'var(--color-background-secondary)',
            color: 'var(--color-text-tertiary)',
            border: '1px solid var(--color-border-tertiary)',
          }}>
            {t('mcp_builtin', lang)}
          </span>
          <span style={{ color: 'var(--color-text-tertiary)', marginLeft: 6 }}>
            (ping / read_file / write_file / open_browser)
          </span>
        </div>
        <span style={{
          fontSize: 11,
          fontWeight: 500,
          padding: '2px 8px',
          borderRadius: 999,
          background: 'var(--color-background-success)',
          color: 'var(--color-text-success)',
        }}>
          {t('mcp_always_on', lang)}
        </span>
      </div>

      {/* Dynamic servers */}
      {servers.map((s) => (
        <div
          key={s.name}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            borderRadius: 12,
            border: '1px solid var(--color-border-tertiary)',
            background: 'var(--color-background-secondary)',
            padding: '12px 16px',
            gap: 8,
          }}
        >
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>{s.name}</div>
            <div style={{ color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {s.base_url}{s.description ? ` — ${s.description}` : ''}
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
            <button
              onClick={() => void toggle(s.name, !s.enabled)}
              style={s.enabled ? {
                padding: '4px 8px',
                borderRadius: 8,
                background: 'var(--color-background-success)',
                color: 'var(--color-text-success)',
                border: 'none',
                cursor: 'pointer',
                fontSize: 11,
              } : {
                padding: '4px 8px',
                borderRadius: 8,
                background: 'var(--color-background-secondary)',
                color: 'var(--color-text-tertiary)',
                border: '1px solid var(--color-border-secondary)',
                cursor: 'pointer',
                fontSize: 11,
              }}
            >
              {s.enabled ? t('mcp_enabled', lang) : t('mcp_disabled', lang)}
            </button>
            {confirmDelete === s.name ? (
              <>
                <button
                  onClick={() => void remove(s.name)}
                  style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', border: 'none', cursor: 'pointer', fontSize: 11 }}
                >
                  {lang === 'zh' ? '确认删除' : 'Confirm'}
                </button>
                <button
                  onClick={() => setConfirmDelete(null)}
                  style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', border: 'none', cursor: 'pointer', fontSize: 11 }}
                >
                  {lang === 'zh' ? '取消' : 'Cancel'}
                </button>
              </>
            ) : (
              <button
                onClick={() => setConfirmDelete(s.name)}
                style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', border: 'none', cursor: 'pointer', fontSize: 11 }}
              >
                {t('mcp_delete', lang)}
              </button>
            )}
          </div>
        </div>
      ))}

      {/* Add form */}
      <div style={{
        borderRadius: 12,
        border: '1px solid var(--color-border-tertiary)',
        background: 'var(--color-background-secondary)',
        padding: '12px 16px',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
      }}>
        <div style={{ fontWeight: 500, color: 'var(--color-text-secondary)' }}>{t('mcp_add_title', lang)}</div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
          <input
            style={inputStyle}
            placeholder={t('mcp_name_ph', lang)}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            style={inputStyle}
            placeholder={t('mcp_url_ph', lang)}
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
          />
          <input
            style={inputStyle}
            placeholder={t('mcp_token_ph', lang)}
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
          <input
            style={inputStyle}
            placeholder={t('mcp_desc_ph', lang)}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>
        <button
          style={{
            padding: '8px 12px',
            borderRadius: 8,
            background: 'var(--color-text-primary)',
            color: 'var(--color-background-primary)',
            border: 'none',
            cursor: 'pointer',
            fontSize: 12,
            fontWeight: 500,
            marginTop: 4,
            opacity: (adding || !name.trim() || !baseUrl.trim()) ? 0.5 : 1,
          }}
          disabled={adding || !name.trim() || !baseUrl.trim()}
          onClick={() => void add()}
        >
          {adding ? t('mcp_adding', lang) : t('mcp_add_btn', lang)}
        </button>
      </div>

      {/* Discovered tools */}
      <div style={{ borderTop: '1px solid var(--color-border-primary)', paddingTop: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
          <span style={{ fontWeight: 500, color: 'var(--color-text-secondary)' }}>
            {t('mcp_tools_title', lang)} ({tools.length})
          </span>
          <button
            onClick={() => void refreshTools()}
            disabled={refreshing}
            style={{
              padding: '6px 12px',
              borderRadius: 8,
              background: 'var(--color-background-primary)',
              border: '1px solid var(--color-border-secondary)',
              color: 'var(--color-text-secondary)',
              fontSize: 12,
              cursor: 'pointer',
              opacity: refreshing ? 0.5 : 1,
            }}
          >
            {refreshing ? t('mcp_refreshing', lang) : t('mcp_refresh', lang)}
          </button>
        </div>
        {tools.length === 0 ? (
          <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>{t('mcp_tools_empty', lang)}</p>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4, maxHeight: 240, overflowY: 'auto' }}>
            {tools.map((tool) => (
              <div
                key={`${tool.server}:${tool.name}`}
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 8,
                  borderRadius: 8,
                  padding: '8px 12px',
                  background: 'var(--color-background-primary)',
                  border: '1px solid var(--color-border-tertiary)',
                }}
              >
                <span style={{ color: 'var(--color-text-tertiary)', flexShrink: 0, marginTop: 2 }}>[{tool.server}]</span>
                <div style={{ minWidth: 0 }}>
                  <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>{tool.name}</span>
                  {tool.description && (
                    <span style={{ color: 'var(--color-text-tertiary)', marginLeft: 6 }}>{tool.description}</span>
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
