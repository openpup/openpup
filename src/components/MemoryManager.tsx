import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface LongTermMemoryItem {
  id: string; content: string; memory_type: string; importance: number; created_at: number;
}

const PAGE_SIZE = 20;
const INPUT = 'w-full rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50';

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

  const load = async () => {
    setLoading(true); setError(null);
    try {
      setItems(await invoke<LongTermMemoryItem[]>('list_long_term_memories', {
        offset: page * PAGE_SIZE, limit: PAGE_SIZE, query: query.trim() || null,
      }));
    } catch (e: unknown) { setError(String(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => { void load(); }, [page]);

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

  return (
    <div className="flex flex-col gap-3 text-xs">
      <div className="flex gap-2">
        <input className={INPUT + ' flex-1'} placeholder={t('mem_search', lang)}
          value={query} onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { setPage(0); void load(); } }} />
        <button className="px-3 py-2 rounded-lg bg-stone-800 border border-stone-700 text-stone-300 hover:bg-stone-700 transition-colors disabled:opacity-50"
          onClick={() => { setPage(0); void load(); }} disabled={loading}>
          {t('mem_search_btn', lang)}
        </button>
      </div>

      {error && <div className="text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{error}</div>}

      <div className="flex items-center justify-between text-stone-500">
        <span>Page {page + 1} · {PAGE_SIZE}/page</span>
        <div className="flex gap-2">
          <button className="px-2.5 py-1 rounded-lg bg-stone-800 border border-stone-700 text-stone-400 hover:text-stone-300 disabled:opacity-40"
            disabled={page === 0 || loading} onClick={() => setPage((p) => Math.max(0, p - 1))}>
            {t('mem_prev', lang)}
          </button>
          <button className="px-2.5 py-1 rounded-lg bg-stone-800 border border-stone-700 text-stone-400 hover:text-stone-300 disabled:opacity-40"
            disabled={items.length < PAGE_SIZE || loading} onClick={() => setPage((p) => p + 1)}>
            {t('mem_next', lang)}
          </button>
        </div>
      </div>

      <div className="space-y-2">
        {items.length === 0 && !loading && <div className="text-stone-500 text-center py-6">{t('mem_empty', lang)}</div>}
        {items.map((item) => (
          <div key={item.id} className="rounded-xl border border-stone-800 bg-stone-900/60 px-4 py-3 flex flex-col gap-1.5 hover:border-stone-700 transition-colors">
            <div className="flex justify-between items-center">
              <span className="font-medium text-stone-300 text-[11px] px-2 py-0.5 rounded-full bg-stone-800">
                {item.memory_type || 'general'}
              </span>
              <span className="text-stone-600">{t('mem_importance', lang)}: {item.importance.toFixed(2)}</span>
            </div>
            <div className="text-stone-200 whitespace-pre-wrap break-words leading-relaxed">{item.content}</div>
            <div className="flex justify-between items-center mt-0.5">
              <span className="text-stone-600">{new Date(item.created_at * 1000).toLocaleDateString()}</span>
              <div className="flex gap-1.5">
                <button className="px-2.5 py-1 rounded-lg bg-stone-800 text-stone-400 hover:text-stone-200 transition-colors border border-stone-700"
                  onClick={() => { setEditing(item); setEditContent(item.content); setEditType(item.memory_type || ''); setEditImportance(item.importance ?? 0.5); }}>
                  {t('mem_edit', lang)}
                </button>
                <button className="px-2.5 py-1 rounded-lg bg-red-900/40 text-red-400 hover:bg-red-900/60 transition-colors"
                  onClick={() => void deleteItem(item)}>
                  {t('mem_delete', lang)}
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>

      {editing && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="w-full max-w-lg rounded-2xl bg-stone-900 border border-stone-700 p-5 space-y-3 shadow-2xl">
            <h2 className="text-sm font-semibold text-stone-100">{t('mem_edit_title', lang)}</h2>
            <textarea className="w-full h-32 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 focus:outline-none focus:ring-1 focus:ring-amber-500/50 resize-y"
              value={editContent} onChange={(e) => setEditContent(e.target.value)} />
            <div className="flex gap-2">
              <input className="flex-1 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
                placeholder={t('mem_type_placeholder', lang)} value={editType} onChange={(e) => setEditType(e.target.value)} />
              <input className="w-28 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
                type="number" min={0} max={1} step={0.05} value={editImportance}
                onChange={(e) => setEditImportance(Number(e.target.value))} />
            </div>
            <div className="flex justify-end gap-2">
              <button className="px-3 py-1.5 rounded-lg bg-stone-800 text-stone-400 hover:text-stone-300 border border-stone-700 transition-colors"
                onClick={() => setEditing(null)}>{t('mem_cancel', lang)}</button>
              <button className="px-3 py-1.5 rounded-lg bg-amber-500 text-stone-950 font-semibold hover:bg-amber-400 transition-colors"
                onClick={() => void saveEdit()}>{t('mem_save', lang)}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
