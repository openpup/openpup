import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import { useLang, t } from '../i18n';

export const DiaryViewer: React.FC = () => {
  const { lang } = useLang();
  const [dates, setDates] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string>('');
  const [loading, setLoading] = useState(false);

  useEffect(() => { invoke<string[]>('list_diary_dates').then(setDates).catch(() => {}); }, []);

  const openDate = async (date: string) => {
    setSelected(date); setLoading(true);
    try { setContent(await invoke<string>('read_diary_entry', { date })); }
    catch { setContent('（无法读取日记）'); }
    finally { setLoading(false); }
  };

  return (
    <div className="flex gap-3 h-full min-h-0">
      <div className="w-32 shrink-0 space-y-0.5">
        {dates.length === 0 && <p className="text-xs text-stone-500 px-2">{t('diary_empty', lang)}</p>}
        {dates.map((d) => (
          <button key={d} onClick={() => void openDate(d)}
            className={`w-full text-left px-3 py-1.5 rounded-lg text-xs transition-colors font-mono ${
              selected === d ? 'bg-amber-500/20 text-amber-400 border border-amber-500/30' : 'text-stone-400 hover:bg-stone-800 hover:text-stone-200'
            }`}>
            {d}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-auto text-xs text-stone-200 min-w-0">
        {!selected && <p className="text-stone-500">{t('diary_select', lang)}</p>}
        {loading && <p className="text-stone-500">{t('diary_loading', lang)}</p>}
        {selected && !loading && (
          <div className="prose prose-invert prose-sm max-w-none prose-p:text-stone-300 prose-headings:text-stone-100">
            <ReactMarkdown>{content}</ReactMarkdown>
          </div>
        )}
      </div>
    </div>
  );
};
