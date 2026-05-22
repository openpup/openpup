import React, { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface McpServer {
  name: string;
  base_url: string;
  token: string;
  description: string;
  enabled: boolean;
  allowed_tools: string[];
}

interface McpToolInfo {
  server: string;
  name: string;
  description: string;
}

type McpFilter = 'all' | 'enabled' | 'disabled';

const inputStyle: React.CSSProperties = {
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '8px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
  width: '100%',
  boxSizing: 'border-box',
};

const cardStyle: React.CSSProperties = {
  borderRadius: 12,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-secondary)',
  padding: '12px 16px',
};

const ghostButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  color: 'var(--color-text-secondary)',
  cursor: 'pointer',
  fontSize: 12,
};

const textButtonStyle: React.CSSProperties = {
  padding: '4px 2px',
  borderRadius: 8,
  background: 'transparent',
  border: '1px solid transparent',
  color: 'var(--color-text-secondary)',
  cursor: 'pointer',
  fontSize: 12,
};

const dangerButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-danger)',
  color: 'var(--color-text-danger)',
  border: 'none',
  cursor: 'pointer',
  fontSize: 12,
};

const activeButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-success)',
  color: 'var(--color-text-success)',
  border: 'none',
  cursor: 'pointer',
  fontSize: 12,
};

const inactiveButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  color: 'var(--color-text-tertiary)',
  border: '1px solid var(--color-border-secondary)',
  cursor: 'pointer',
  fontSize: 12,
};

const pillStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 6,
  padding: '3px 8px',
  borderRadius: 999,
  fontSize: 11,
  fontWeight: 600,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-tertiary)',
  color: 'var(--color-text-secondary)',
};

const textareaStyle: React.CSSProperties = {
  ...inputStyle,
  minHeight: 86,
  resize: 'vertical',
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
};

function allowToolsToText(allowedTools: string[]): string {
  return allowedTools.join('\n');
}

function parseAllowedTools(text: string): string[] {
  const seen = new Set<string>();
  return text
    .split('\n')
    .map((tool) => tool.trim())
    .filter((tool) => {
      if (!tool || seen.has(tool)) return false;
      seen.add(tool);
      return true;
    });
}

export const McpSettings: React.FC = () => {
  const { lang } = useLang();
  const [servers, setServers] = useState<McpServer[]>([]);
  const [tools, setTools] = useState<McpToolInfo[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState('');
  const [mcpUrl, setMcpUrl] = useState('');
  const [token, setToken] = useState('');
  const [description, setDescription] = useState('');
  const [allowedTools, setAllowedTools] = useState('');
  const [adding, setAdding] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [editUrl, setEditUrl] = useState('');
  const [editToken, setEditToken] = useState('');
  const [editDescription, setEditDescription] = useState('');
  const [editAllowedTools, setEditAllowedTools] = useState('');
  const [savingEdit, setSavingEdit] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<McpFilter>('all');
  const deferredSearch = useDeferredValue(search.trim().toLowerCase());

  const load = async () => {
    try {
      setServers(await invoke<McpServer[]>('list_mcp_servers'));
    } catch (e) {
      setError(String(e));
    }
  };

  const loadTools = async () => {
    try {
      setTools(await invoke<McpToolInfo[]>('list_mcp_tools'));
    } catch {
      // Ignore tool discovery failures on initial load.
    }
  };

  useEffect(() => {
    void load();
    void loadTools();
  }, []);

  const refreshTools = async () => {
    setRefreshing(true);
    setError(null);
    try {
      setTools(await invoke<McpToolInfo[]>('refresh_mcp_tools'));
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  };

  const add = async () => {
    if (!name.trim() || !mcpUrl.trim()) return;
    setAdding(true);
    setError(null);
    try {
      await invoke('add_mcp_server', {
        entry: {
          name: name.trim(),
          base_url: mcpUrl.trim(),
          token: token.trim(),
          description: description.trim(),
          allowed_tools: parseAllowedTools(allowedTools),
        },
      });
      setName('');
      setMcpUrl('');
      setToken('');
      setDescription('');
      setAllowedTools('');
      setAddOpen(false);
      await load();
      await loadTools();
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  const startCreate = () => {
    cancelEdit();
    setName('');
    setMcpUrl('');
    setToken('');
    setDescription('');
    setAllowedTools('');
    setAddOpen(true);
  };

  const startEdit = (server: McpServer) => {
    setAddOpen(false);
    setEditingName(server.name);
    setEditName(server.name);
    setEditUrl(server.base_url);
    setEditToken(server.token);
    setEditDescription(server.description);
    setEditAllowedTools(allowToolsToText(server.allowed_tools));
  };

  const cancelEdit = () => {
    setEditingName(null);
    setEditName('');
    setEditUrl('');
    setEditToken('');
    setEditDescription('');
    setEditAllowedTools('');
  };

  const saveEdit = async () => {
    if (!editingName || !editName.trim() || !editUrl.trim()) return;
    setSavingEdit(true);
    setError(null);
    try {
      await invoke('update_mcp_server', {
        entry: {
          original_name: editingName,
          name: editName.trim(),
          base_url: editUrl.trim(),
          token: editToken.trim(),
          description: editDescription.trim(),
          enabled: servers.find((server) => server.name === editingName)?.enabled ?? true,
          allowed_tools: parseAllowedTools(editAllowedTools),
        },
      });
      cancelEdit();
      await load();
      await loadTools();
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingEdit(false);
    }
  };

  const remove = async (serverName: string) => {
    await invoke('remove_mcp_server', { name: serverName }).catch((e) => setError(String(e)));
    setConfirmDelete(null);
    await load();
    await loadTools();
  };

  const toggle = async (serverName: string, enabled: boolean) => {
    await invoke('toggle_mcp_server', { name: serverName, enabled }).catch((e) => setError(String(e)));
    setServers((prev) => prev.map((server) => server.name === serverName ? { ...server, enabled } : server));
    await loadTools();
  };

  const toolsByServer = useMemo(() => {
    const grouped = new Map<string, McpToolInfo[]>();
    for (const tool of tools) {
      const list = grouped.get(tool.server) ?? [];
      list.push(tool);
      grouped.set(tool.server, list);
    }
    return grouped;
  }, [tools]);

  const filteredServers = useMemo(() => {
    return servers.filter((server) => {
      if (filter === 'enabled' && !server.enabled) return false;
      if (filter === 'disabled' && server.enabled) return false;
      if (!deferredSearch) return true;
      const haystack = `${server.name} ${server.base_url} ${server.description}`.toLowerCase();
      const toolHaystack = (toolsByServer.get(server.name) ?? [])
        .map((tool) => `${tool.name} ${tool.description}`)
        .join(' ')
        .toLowerCase();
      return haystack.includes(deferredSearch) || toolHaystack.includes(deferredSearch);
    });
  }, [servers, filter, deferredSearch, toolsByServer]);

  const filteredTools = useMemo(() => {
    return tools.filter((tool) => {
      if (!deferredSearch) return true;
      return `${tool.server} ${tool.name} ${tool.description}`.toLowerCase().includes(deferredSearch);
    });
  }, [tools, deferredSearch]);

  const enabledCount = servers.filter((server) => server.enabled).length + 1;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, fontSize: 12 }}>
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

      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        <span style={pillStyle}>{t('mcp_stat_servers', lang)} · {servers.length + 1}</span>
        <span style={pillStyle}>{t('mcp_stat_enabled', lang)} · {enabledCount}</span>
        <span style={pillStyle}>{t('mcp_stat_tools', lang)} · {tools.length}</span>
      </div>

      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        <input
          style={{ ...inputStyle, flex: '1 1 260px' }}
          placeholder={t('mcp_search_ph', lang)}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <button onClick={() => setFilter('all')} style={filter === 'all' ? activeButtonStyle : inactiveButtonStyle}>{t('tab_all', lang)}</button>
        <button onClick={() => setFilter('enabled')} style={filter === 'enabled' ? activeButtonStyle : inactiveButtonStyle}>{t('mcp_enabled', lang)}</button>
        <button onClick={() => setFilter('disabled')} style={filter === 'disabled' ? activeButtonStyle : inactiveButtonStyle}>{t('mcp_disabled', lang)}</button>
        <button
          onClick={() => void refreshTools()}
          disabled={refreshing}
          style={{ ...ghostButtonStyle, opacity: refreshing ? 0.5 : 1 }}
        >
          {refreshing ? t('mcp_refreshing', lang) : t('mcp_refresh', lang)}
        </button>
      </div>

      <div style={{ display: 'grid', gap: 8 }}>
        {!addOpen && (
          <button
            onClick={startCreate}
            style={{
              ...ghostButtonStyle,
              width: 'fit-content',
            }}
          >
            {t('mcp_add_title', lang)}
          </button>
        )}

        {addOpen && (
          <div style={{ ...cardStyle, background: 'color-mix(in srgb, var(--color-background-primary) 86%, var(--color-background-secondary) 14%)' }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8, marginBottom: 8 }}>
              <div>
                <div style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>{t('mcp_add_title', lang)}</div>
                <div style={{ color: 'var(--color-text-tertiary)', marginTop: 4 }}>{t('mcp_add_hint', lang)}</div>
              </div>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 8 }}>
              <input style={inputStyle} placeholder={t('mcp_name_ph', lang)} value={name} onChange={(e) => setName(e.target.value)} />
              <input style={inputStyle} placeholder={t('mcp_url_ph', lang)} value={mcpUrl} onChange={(e) => setMcpUrl(e.target.value)} />
              <input style={inputStyle} placeholder={t('mcp_token_ph', lang)} type="password" value={token} onChange={(e) => setToken(e.target.value)} />
              <input style={inputStyle} placeholder={t('mcp_desc_ph', lang)} value={description} onChange={(e) => setDescription(e.target.value)} />
            </div>
            <div style={{ marginTop: 8, display: 'grid', gap: 6 }}>
              <textarea
                style={textareaStyle}
                placeholder={t('mcp_allowed_tools_ph', lang)}
                value={allowedTools}
                onChange={(e) => setAllowedTools(e.target.value)}
              />
              <div style={{ color: 'var(--color-text-tertiary)', fontSize: 11 }}>
                {t('mcp_allowed_tools_hint', lang)}
              </div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 10 }}>
              <button onClick={() => setAddOpen(false)} style={ghostButtonStyle}>
                {t('common_cancel', lang)}
              </button>
              <button
                style={{ ...ghostButtonStyle, background: 'var(--color-text-primary)', color: 'var(--color-background-primary)', opacity: (adding || !name.trim() || !mcpUrl.trim()) ? 0.5 : 1 }}
                disabled={adding || !name.trim() || !mcpUrl.trim()}
                onClick={() => void add()}
              >
                {adding ? t('mcp_adding', lang) : t('mcp_add_btn', lang)}
              </button>
            </div>
          </div>
        )}

        <div style={{ ...cardStyle, padding: 0, overflow: 'hidden', background: 'color-mix(in srgb, var(--color-background-secondary) 92%, var(--color-background-primary) 8%)' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, padding: '10px 12px', borderBottom: '1px solid var(--color-border-tertiary)', background: 'color-mix(in srgb, var(--color-background-primary) 70%, var(--color-background-secondary) 30%)' }}>
            <div style={{ minWidth: 0 }}>
              <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>local</span>
              <span style={{ marginLeft: 8, ...pillStyle, background: 'var(--color-background-info)', color: 'var(--color-text-info)', border: 'none' }}>
                {t('mcp_builtin', lang)}
              </span>
              <span style={{ marginLeft: 8, color: 'var(--color-text-tertiary)', fontSize: 12 }}>
                ping / read_file / write_file / open_browser
              </span>
            </div>
            <span style={{ color: 'var(--color-text-tertiary)', fontSize: 11, flexShrink: 0 }}>
              {t('mcp_always_on', lang)}
            </span>
          </div>

          {filteredServers.map((server, index) => {
            const serverTools = toolsByServer.get(server.name) ?? [];
            const showDivider = index < filteredServers.length - 1;
            return (
              <div
                key={server.name}
                style={{
                  padding: '10px 12px',
                  background: editingName === server.name ? 'color-mix(in srgb, var(--color-background-primary) 78%, var(--color-background-secondary) 22%)' : 'transparent',
                  borderBottom: showDivider ? '1px solid var(--color-border-tertiary)' : 'none',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>{server.name}</span>
                    <span
                      style={{
                          width: 6,
                          height: 6,
                          borderRadius: '50%',
                          background: server.enabled ? 'var(--color-text-success)' : 'var(--color-text-tertiary)',
                          flexShrink: 0,
                        }}
                      />
                      <span style={{ fontSize: 11, color: server.enabled ? 'var(--color-text-success)' : 'var(--color-text-tertiary)' }}>
                        {server.enabled ? t('mcp_enabled', lang) : t('mcp_disabled', lang)}
                      </span>
                    <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                      {serverTools.length} {t('mcp_tool_count_suffix', lang)}
                    </span>
                    <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                      · {server.allowed_tools.length > 0
                        ? `${server.allowed_tools.length} ${t('mcp_allowlisted_suffix', lang)}`
                        : t('mcp_all_tools_allowed', lang)}
                    </span>
                    {server.description && (
                      <span
                        style={{
                          color: 'var(--color-text-secondary)',
                          whiteSpace: 'nowrap',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          minWidth: 0,
                          maxWidth: '100%',
                        }}
                        title={server.description}
                      >
                        · {server.description}
                      </span>
                    )}
                  </div>
                  <div
                    style={{
                      color: 'var(--color-text-tertiary)',
                      marginTop: 3,
                      whiteSpace: 'nowrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                    }}
                    title={server.base_url}
                  >
                    {server.base_url}
                  </div>
                </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
                    <button onClick={() => startEdit(server)} style={textButtonStyle}>{t('pup_edit', lang)}</button>
                    <button
                      onClick={() => void toggle(server.name, !server.enabled)}
                      style={server.enabled ? activeButtonStyle : inactiveButtonStyle}
                    >
                      {server.enabled ? t('mcp_action_disable', lang) : t('mcp_action_enable', lang)}
                    </button>
                    {confirmDelete === server.name ? (
                      <>
                        <button onClick={() => void remove(server.name)} style={dangerButtonStyle}>{t('common_confirm', lang)}</button>
                        <button onClick={() => setConfirmDelete(null)} style={ghostButtonStyle}>{t('common_cancel', lang)}</button>
                      </>
                    ) : (
                      <button onClick={() => setConfirmDelete(server.name)} style={dangerButtonStyle}>{t('mcp_delete', lang)}</button>
                    )}
                  </div>
                </div>
                {editingName === server.name && (
                  <div style={{ marginTop: 10, paddingTop: 10, borderTop: '1px solid var(--color-border-tertiary)', display: 'grid', gap: 8 }}>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 8 }}>
                      <input style={inputStyle} placeholder={t('mcp_name_ph', lang)} value={editName} onChange={(e) => setEditName(e.target.value)} />
                      <input style={inputStyle} placeholder={t('mcp_url_ph', lang)} value={editUrl} onChange={(e) => setEditUrl(e.target.value)} />
                      <input style={inputStyle} placeholder={t('mcp_token_ph', lang)} type="password" value={editToken} onChange={(e) => setEditToken(e.target.value)} />
                      <input style={inputStyle} placeholder={t('mcp_desc_ph', lang)} value={editDescription} onChange={(e) => setEditDescription(e.target.value)} />
                    </div>
                    <div style={{ display: 'grid', gap: 6 }}>
                      <textarea
                        style={textareaStyle}
                        placeholder={t('mcp_allowed_tools_ph', lang)}
                        value={editAllowedTools}
                        onChange={(e) => setEditAllowedTools(e.target.value)}
                      />
                      <div style={{ color: 'var(--color-text-tertiary)', fontSize: 11 }}>
                        {t('mcp_allowed_tools_hint', lang)}
                      </div>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
                      <button onClick={cancelEdit} style={ghostButtonStyle}>{t('common_cancel', lang)}</button>
                      <button
                        onClick={() => void saveEdit()}
                        disabled={savingEdit || !editName.trim() || !editUrl.trim()}
                        style={{
                          ...ghostButtonStyle,
                          background: 'var(--color-text-primary)',
                          color: 'var(--color-background-primary)',
                          opacity: (savingEdit || !editName.trim() || !editUrl.trim()) ? 0.5 : 1,
                        }}
                      >
                        {savingEdit ? t('mcp_adding', lang) : t('pup_save', lang)}
                      </button>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {filteredServers.length === 0 && (
          <div style={{ ...cardStyle, color: 'var(--color-text-tertiary)', textAlign: 'center' }}>
            {servers.length === 0 ? t('mcp_empty_body', lang) : t('mcp_no_server_match_body', lang)}
          </div>
        )}
      </div>

      <div style={cardStyle}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8, gap: 8 }}>
          <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>
            {t('mcp_tools_title', lang)} ({filteredTools.length})
          </span>
          <span style={{ color: 'var(--color-text-tertiary)' }}>{t('mcp_tools_hint', lang)}</span>
        </div>
        {filteredTools.length === 0 ? (
          <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>
            {tools.length === 0 ? t('mcp_tools_empty', lang) : t('mcp_no_tool_match_body', lang)}
          </p>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 280, overflowY: 'auto' }}>
            {filteredTools.map((tool) => (
              <div
                key={`${tool.server}:${tool.name}`}
                style={{
                  borderRadius: 8,
                  padding: '8px 12px',
                  background: 'var(--color-background-primary)',
                  border: '1px solid var(--color-border-tertiary)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 8,
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
