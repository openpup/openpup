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
    <div style={{ display: 'flex', gap: '12px', height: '100%', minHeight: 0 }}>
      <div style={{ width: '128px', flexShrink: 0 }}>
        {dates.length === 0 && (
          <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', padding: '0 8px' }}>
            {t('diary_empty', lang)}
          </p>
        )}
        {dates.map((d) => (
          <button
            key={d}
            onClick={() => void openDate(d)}
            style={{
              display: 'block',
              width: '100%',
              textAlign: 'left',
              padding: '6px 12px',
              borderRadius: '8px',
              fontSize: '13px',
              fontFamily: 'var(--font-mono)',
              cursor: 'pointer',
              border: selected === d ? '1px solid var(--color-border-secondary)' : '1px solid transparent',
              background: selected === d ? 'var(--color-background-secondary)' : 'transparent',
              color: selected === d ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)',
              marginBottom: '2px',
              transition: 'background 0.15s, color 0.15s',
            }}
          >
            {d}
          </button>
        ))}
      </div>
      <div style={{ flex: 1, overflow: 'auto', fontSize: '13px', color: 'var(--color-text-primary)', minWidth: 0 }}>
        {!selected && (
          <p style={{ color: 'var(--color-text-tertiary)' }}>{t('diary_select', lang)}</p>
        )}
        {loading && (
          <p style={{ color: 'var(--color-text-tertiary)' }}>{t('diary_loading', lang)}</p>
        )}
        {selected && !loading && (
          <div className="prose prose-sm max-w-none">
            <ReactMarkdown>{content}</ReactMarkdown>
          </div>
        )}
      </div>
    </div>
  );
};
