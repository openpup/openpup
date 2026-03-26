import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';
import { formatDateOnly } from '../utils/locale';

interface LongTermMemoryItem {
  id: string; content: string; memory_type: string; importance: number; created_at: number;
  superseded_by: string | null;
}

const PAGE_SIZE = 20;

const inputStyle: React.CSSProperties = {
  width: '100%',
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '6px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
};

type TabKey = 'active' | 'history';

export const MemoryManager: React.FC = () => {
  const { lang } = useLang();
  const [items, setItems] = useState<LongTermMemoryItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [editing, setEditing] = useState<LongTermMemoryItem | null>(null);
  const [editContent, setEditContent] = useState('');
  const [editType, setEditType] = useState('');
  const [editImportance, setEditImportance] = useState(0.5);
  const [tab, setTab] = useState<TabKey>('active');

  const load = async () => {
    setLoading(true); setError(null);
    try {
      const all = await invoke<LongTermMemoryItem[]>('list_long_term_memories', {
        offset: page * PAGE_SIZE, limit: PAGE_SIZE, query: query.trim() || null,
      });
      if (tab === 'active') {
        // Active: exclude invalidated, sort rules first
        const active = all.filter((m) => m.memory_type !== 'invalidated');
        active.sort((a, b) => {
          if (a.memory_type === 'rule' && b.memory_type !== 'rule') return -1;
          if (a.memory_type !== 'rule' && b.memory_type === 'rule') return 1;
          return 0;
        });
        setItems(active);
      } else {
        // History: only invalidated / superseded
        setItems(all.filter((m) => m.memory_type === 'invalidated' || m.superseded_by != null));
      }
    } catch (e: unknown) { setError(String(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => { void load(); }, [page, tab]);

  const saveEdit = async () => {
    if (!editing) return;
    try {
      await invoke('update_long_term_memory', { id: editing.id, content: editContent, memoryType: editType, importance: editImportance });
      setEditing(null); await load();
    } catch (e: unknown) { setError(String(e)); }
  };

  const deleteItem = async (item: LongTermMemoryItem) => {
    if (!window.confirm(t('mem_confirm_delete', lang))) return;
    try { await invoke('delete_long_term_memory', { id: item.id }); await load(); }
    catch (e: unknown) { setError(String(e)); }
  };

  const typeLabel = (mt: string): string => {
    switch (mt) {
      case 'rule': return '🔒 规则';
      case 'preference': return '偏好';
      case 'fact': return '事实';
      case 'experience': return '经历';
      case 'invalidated': return '已失效';
      default: return mt || 'general';
    }
  };

  const typeBadgeStyle = (mt: string): React.CSSProperties => {
    const base: React.CSSProperties = {
      fontWeight: 500,
      fontSize: 11,
      padding: '2px 8px',
      borderRadius: 999,
      border: '1px solid var(--color-border-tertiary)',
    };
    if (mt === 'rule') {
      return { ...base, background: 'rgba(29, 158, 117, 0.15)', color: '#1D9E75', borderColor: '#1D9E75' };
    }
    if (mt === 'invalidated') {
      return { ...base, background: 'var(--color-background-danger)', color: 'var(--color-text-danger)' };
    }
    return { ...base, background: 'var(--color-background-secondary)', color: 'var(--color-text-tertiary)' };
  };

  const tabStyle = (active: boolean): React.CSSProperties => ({
    padding: '6px 14px',
    borderRadius: 8,
    background: active ? 'var(--color-text-primary)' : 'var(--color-background-primary)',
    color: active ? 'var(--color-background-primary)' : 'var(--color-text-secondary)',
    border: active ? 'none' : '1px solid var(--color-border-secondary)',
    cursor: 'pointer',
    fontSize: 12,
    fontWeight: active ? 600 : 400,
  });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, fontSize: 12 }}>
      {/* Tab bar */}
      <div style={{ display: 'flex', gap: 6 }}>
        <button style={tabStyle(tab === 'active')} onClick={() => { setTab('active'); setPage(0); }}>
          {lang === 'zh' ? '当前记忆' : 'Active'}
        </button>
        <button style={tabStyle(tab === 'history')} onClick={() => { setTab('history'); setPage(0); }}>
          {lang === 'zh' ? '历史记录' : 'History'}
        </button>
      </div>

      <div style={{ display: 'flex', gap: 8 }}>
        <input
          style={{ ...inputStyle, flex: 1 }}
          placeholder={t('mem_search', lang)}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { setPage(0); void load(); } }}
        />
        <button
          style={{
            padding: '6px 12px',
            borderRadius: 8,
            background: 'var(--color-background-primary)',
            border: '1px solid var(--color-border-secondary)',
            color: 'var(--color-text-secondary)',
            cursor: 'pointer',
            fontSize: 12,
            opacity: loading ? 0.5 : 1,
          }}
          onClick={() => { setPage(0); void load(); }}
          disabled={loading}
        >
          {t('mem_search_btn', lang)}
        </button>
      </div>

      {error && (
        <div style={{
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '6px 12px',
          borderRadius: 8,
        }}>
          {error}
        </div>
      )}

      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--color-text-tertiary)' }}>
        <span>Page {page + 1} · {PAGE_SIZE}/page</span>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            style={{
              padding: '4px 10px',
              borderRadius: 8,
              background: 'var(--color-background-primary)',
              border: '1px solid var(--color-border-secondary)',
              color: 'var(--color-text-secondary)',
              cursor: 'pointer',
              fontSize: 12,
              opacity: (page === 0 || loading) ? 0.4 : 1,
            }}
            disabled={page === 0 || loading}
            onClick={() => setPage((p) => Math.max(0, p - 1))}
          >
            {t('mem_prev', lang)}
          </button>
          <button
            style={{
              padding: '4px 10px',
              borderRadius: 8,
              background: 'var(--color-background-primary)',
              border: '1px solid var(--color-border-secondary)',
              color: 'var(--color-text-secondary)',
              cursor: 'pointer',
              fontSize: 12,
              opacity: (items.length < PAGE_SIZE || loading) ? 0.4 : 1,
            }}
            disabled={items.length < PAGE_SIZE || loading}
            onClick={() => setPage((p) => p + 1)}
          >
            {t('mem_next', lang)}
          </button>
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {items.length === 0 && !loading && (
          <div style={{ color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '24px 0' }}>
            {tab === 'history'
              ? (lang === 'zh' ? '暂无历史记忆' : 'No memory history')
              : t('mem_empty', lang)}
          </div>
        )}
        {items.map((item) => (
          <div
            key={item.id}
            style={{
              borderRadius: 12,
              border: item.memory_type === 'rule'
                ? '1px solid rgba(29, 158, 117, 0.4)'
                : '1px solid var(--color-border-primary)',
              background: 'var(--color-background-secondary)',
              padding: '12px 16px',
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
              opacity: item.memory_type === 'invalidated' ? 0.6 : 1,
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={typeBadgeStyle(item.memory_type)}>
                {typeLabel(item.memory_type)}
              </span>
              <span style={{ color: 'var(--color-text-tertiary)' }}>
                {t('mem_importance', lang)}: {item.importance.toFixed(2)}
              </span>
            </div>
            <div style={{ marginTop: 2, marginBottom: 2, height: 4, borderRadius: 2, background: 'var(--color-background-secondary)', border: '1px solid var(--color-border-tertiary)', overflow: 'hidden' }}>
              <div style={{ height: '100%', width: `${item.importance * 100}%`, background: item.memory_type === 'rule' ? '#1D9E75' : '#1D9E75', borderRadius: 2 }} />
            </div>
            <div style={{ color: 'var(--color-text-primary)', whiteSpace: 'pre-wrap', wordBreak: 'break-word', lineHeight: 1.6 }}>
              {item.content}
            </div>
            {item.superseded_by && (
              <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', fontStyle: 'italic' }}>
                {lang === 'zh' ? '已被替代 →' : 'Superseded by →'} {item.superseded_by.slice(0, 8)}…
              </div>
            )}
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 2 }}>
              <span style={{ color: 'var(--color-text-tertiary)' }}>
                {formatDateOnly(item.created_at, lang)}
              </span>
              {tab === 'active' && (
                <div style={{ display: 'flex', gap: 6 }}>
                  <button
                    style={{
                      padding: '4px 10px',
                      borderRadius: 8,
                      background: 'var(--color-background-primary)',
                      border: '1px solid var(--color-border-secondary)',
                      color: 'var(--color-text-tertiary)',
                      cursor: 'pointer',
                      fontSize: 12,
                    }}
                    onClick={() => { setEditing(item); setEditContent(item.content); setEditType(item.memory_type || ''); setEditImportance(item.importance ?? 0.5); }}
                  >
                    {t('mem_edit', lang)}
                  </button>
                  <button
                    style={{
                      padding: '4px 10px',
                      borderRadius: 8,
                      background: 'var(--color-background-danger)',
                      color: 'var(--color-text-danger)',
                      border: 'none',
                      cursor: 'pointer',
                      fontSize: 12,
                    }}
                    onClick={() => void deleteItem(item)}
                  >
                    {t('mem_delete', lang)}
                  </button>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      {editing && (
        <div style={{
          position: 'fixed',
          inset: 0,
          background: 'rgba(0,0,0,0.6)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: 50,
        }}>
          <div style={{
            width: '100%',
            maxWidth: 512,
            borderRadius: 16,
            background: 'var(--color-background-primary)',
            border: '1px solid var(--color-border-secondary)',
            padding: 20,
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
          }}>
            <h2 style={{ fontSize: 14, fontWeight: 500, color: 'var(--color-text-primary)', margin: 0 }}>
              {t('mem_edit_title', lang)}
            </h2>
            <textarea
              style={{
                ...inputStyle,
                height: 128,
                resize: 'vertical',
                fontFamily: 'inherit',
              }}
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
            />
            <div style={{ display: 'flex', gap: 8 }}>
              <input
                style={{ ...inputStyle, flex: 1 }}
                placeholder={t('mem_type_placeholder', lang)}
                value={editType}
                onChange={(e) => setEditType(e.target.value)}
              />
              <input
                style={{ ...inputStyle, width: 112 }}
                type="number"
                min={0}
                max={1}
                step={0.05}
                value={editImportance}
                onChange={(e) => setEditImportance(Number(e.target.value))}
              />
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <button
                style={{
                  padding: '6px 12px',
                  borderRadius: 8,
                  background: 'var(--color-background-primary)',
                  border: '1px solid var(--color-border-secondary)',
                  color: 'var(--color-text-tertiary)',
                  cursor: 'pointer',
                  fontSize: 12,
                }}
                onClick={() => setEditing(null)}
              >
                {t('mem_cancel', lang)}
              </button>
              <button
                style={{
                  padding: '6px 12px',
                  borderRadius: 8,
                  background: 'var(--color-text-primary)',
                  color: 'var(--color-background-primary)',
                  border: 'none',
                  cursor: 'pointer',
                  fontSize: 12,
                  fontWeight: 500,
                }}
                onClick={() => void saveEdit()}
              >
                {t('mem_save', lang)}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
