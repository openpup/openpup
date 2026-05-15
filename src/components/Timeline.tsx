import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { useLang, t } from '../i18n';
import { formatRelativeTime } from '../utils/locale';
import { pupAccentColor } from '../utils/pupVisuals';

interface TimelineEvent {
  role: string;
  pup_key: string;
  pup_name: string;
  content: string;
  timestamp: number;
}

interface SkillRunItem {
  id: string;
  skill_name: string;
  triggered_by: string;
  started_at: number;
  completed_at: number | null;
  status: string;
  output: string | null;
}

type Filter = 'all' | 'alpha' | 'you' | 'skills';

const STATUS_COLOR: Record<string, React.CSSProperties> = {
  completed: { color: 'var(--color-text-success)' },
  failed:    { color: 'var(--color-text-danger)' },
  running:   { color: 'var(--color-text-info)' },
};

export const Timeline: React.FC = () => {
  const { lang } = useLang();
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [skillRuns, setSkillRuns] = useState<SkillRunItem[]>([]);
  const [filter, setFilter] = useState<Filter>('all');
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<TimelineEvent[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [exporting, setExporting] = useState<'markdown' | 'html' | null>(null);

  const loadEvents = async () => {
    try {
      const data = await invoke<TimelineEvent[]>('list_timeline_events', { limit: 50 });
      setEvents(data);
    } catch (e) {
      console.error(e);
    }
  };

  const loadSkillRuns = async () => {
    try {
      const data = await invoke<SkillRunItem[]>('list_skill_runs', { limit: 50 });
      setSkillRuns(data);
    } catch (e) {
      console.error(e);
    }
  };

  const load = async () => {
    setLoading(true);
    await Promise.all([loadEvents(), loadSkillRuns()]);
    setLoading(false);
  };

  useEffect(() => {
    void load();

    const unlistenRun = listen('skill_run_completed', () => void loadSkillRuns());
    const unlistenHb = listen('heartbeat_completed', () => void loadSkillRuns());
    return () => {
      void unlistenRun.then((f) => f());
      void unlistenHb.then((f) => f());
    };
  }, []);

  const filteredEvents = events.filter((e) => {
    if (filter === 'alpha') return e.pup_key === 'alpha';
    if (filter === 'you') return e.pup_key === 'you';
    if (filter === 'skills') return false;
    return true;
  });

  const doSearch = async (q: string) => {
    if (!q.trim()) { setSearchResults(null); return; }
    setSearching(true);
    try {
      const rows = await invoke<{ role: string; content: string; timestamp: number }[]>(
        'search_conversations',
        { query: q.trim(), limit: 50 },
      );
      setSearchResults(
        rows.map((r) => ({
          role: r.role,
          pup_key: r.role === 'user' ? 'you' : 'alpha',
          pup_name: r.role === 'user' ? 'You' : 'Alpha',
          content: r.content,
          timestamp: r.timestamp,
        })),
      );
    } catch { /* non-critical */ }
    finally { setSearching(false); }
  };

  const tabs: { key: Filter; label: string }[] = [
    { key: 'all', label: t('tab_all', lang) },
    { key: 'alpha', label: t('tab_alpha', lang) },
    { key: 'you', label: t('tab_you', lang) },
    { key: 'skills', label: t('tab_skills_run', lang) },
  ];

  const exportableEvents = searchResults ?? filteredEvents;
  const exportSuffix = searchResults !== null ? 'search' : filter;

  const exportTimeline = async (format: 'markdown' | 'html') => {
    if (filter === 'skills') return;
    setExporting(format);
    try {
      const date = new Date().toISOString().slice(0, 10);
      const ext = format === 'html' ? 'html' : 'md';
      const defaultPath = `timeline-${exportSuffix}-${date}.${ext}`;
      const path = await save({ defaultPath });
      if (!path) return;
      const content = await invoke<string>('export_timeline_content', {
        format,
        events: exportableEvents,
      });
      await invoke('save_artifact_to_file', { path, content });
    } catch (error) {
      console.error('export timeline failed:', error);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      {/* Search bar */}
      <div style={{ display: 'flex', gap: '8px', marginBottom: '12px' }}>
        <input
          style={{
            flex: 1,
            borderRadius: '8px',
            background: 'var(--color-background-secondary)',
            border: '1px solid var(--color-border-secondary)',
            padding: '6px 12px',
            fontSize: '13px',
            color: 'var(--color-text-primary)',
            outline: 'none',
          }}
          placeholder={t('timeline_search', lang)}
          value={searchQuery}
          onChange={(e) => {
            setSearchQuery(e.target.value);
            if (!e.target.value.trim()) setSearchResults(null);
          }}
          onKeyDown={(e) => { if (e.key === 'Enter') void doSearch(searchQuery); }}
        />
        <button
          style={{
            padding: '6px 12px',
            borderRadius: '8px',
            background: 'var(--color-background-secondary)',
            border: '1px solid var(--color-border-secondary)',
            color: 'var(--color-text-primary)',
            fontSize: '13px',
            cursor: 'pointer',
            opacity: searching ? 0.5 : 1,
          }}
          disabled={searching}
          onClick={() => void doSearch(searchQuery)}
        >
          {searching ? '…' : t('timeline_search_btn', lang)}
        </button>
        {searchResults !== null && (
          <button
            style={{
              padding: '4px 8px',
              borderRadius: '8px',
              background: 'transparent',
              border: 'none',
              color: 'var(--color-text-tertiary)',
              fontSize: '13px',
              cursor: 'pointer',
            }}
            onClick={() => { setSearchResults(null); setSearchQuery(''); }}
          >
            {t('timeline_clear', lang)}
          </button>
        )}
      </div>

      {/* Filter tabs — hidden while search is active */}
      {searchResults === null && (
        <div style={{ display: 'flex', gap: '6px', marginBottom: '16px' }}>
          {tabs.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setFilter(tab.key)}
              style={{
                padding: '6px 12px',
                borderRadius: '8px',
                fontSize: '13px',
                fontWeight: 500,
                cursor: 'pointer',
                background: 'transparent',
                border: filter === tab.key
                  ? '0.5px solid var(--color-border-secondary)'
                  : '0.5px solid transparent',
                color: filter === tab.key
                  ? 'var(--color-text-primary)'
                  : 'var(--color-text-tertiary)',
              }}
            >
              {tab.label}
            </button>
          ))}
          <button
            style={{
              marginLeft: 'auto',
              fontSize: '13px',
              background: 'transparent',
              border: 'none',
              color: filter === 'skills' ? 'var(--color-text-quaternary)' : 'var(--color-text-tertiary)',
              cursor: exporting || filter === 'skills' ? 'not-allowed' : 'pointer',
              opacity: exporting || filter === 'skills' ? 0.5 : 1,
            }}
            disabled={!!exporting || filter === 'skills'}
            onClick={() => void exportTimeline('markdown')}
          >
            {exporting === 'markdown' ? '…' : (lang === 'zh' ? '导出 Markdown' : 'Export Markdown')}
          </button>
          <button
            style={{
              fontSize: '13px',
              background: 'transparent',
              border: 'none',
              color: filter === 'skills' ? 'var(--color-text-quaternary)' : 'var(--color-text-tertiary)',
              cursor: exporting || filter === 'skills' ? 'not-allowed' : 'pointer',
              opacity: exporting || filter === 'skills' ? 0.5 : 1,
            }}
            disabled={!!exporting || filter === 'skills'}
            onClick={() => void exportTimeline('html')}
          >
            {exporting === 'html' ? '…' : (lang === 'zh' ? '导出 HTML' : 'Export HTML')}
          </button>
          <button
            style={{
              fontSize: '13px',
              background: 'transparent',
              border: 'none',
              color: 'var(--color-text-tertiary)',
              cursor: 'pointer',
            }}
            onClick={() => void load()}
          >
            {t('timeline_refresh', lang)}
          </button>
        </div>
      )}

      {/* Search results */}
      {searchResults !== null ? (
        searchResults.length === 0 ? (
          <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)' }}>{t('timeline_no_results', lang)}</div>
        ) : (
          <div style={{ flex: 1, overflowY: 'auto' }}>
            {searchResults.map((evt, i) => (
              <div
                key={i}
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: '12px',
                  padding: '12px 0',
                  borderBottom: '1px solid var(--color-border-tertiary)',
                }}
              >
                <div style={{
                  width: '8px',
                  height: '8px',
                  borderRadius: '50%',
                  marginTop: '6px',
                  flexShrink: 0,
                  background: pupAccentColor(evt.pup_key),
                }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginBottom: '2px' }}>
                    {evt.pup_name} · {formatRelativeTime(evt.timestamp, lang)}
                  </div>
                  <div style={{ fontSize: '12.5px', color: 'var(--color-text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {evt.content}
                  </div>
                </div>
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0, alignSelf: 'center' }}>
                  ~{Math.ceil(evt.content.length / 4)} tokens
                </div>
              </div>
            ))}
          </div>
        )
      ) : loading ? (
        <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)' }}>加载中…</div>
      ) : filter === 'skills' ? (
        /* ── Skill runs ── */
        skillRuns.length === 0 ? (
          <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)' }}>{t('timeline_empty', lang)}</div>
        ) : (
          <div style={{ flex: 1, overflowY: 'auto' }}>
            {skillRuns.map((run) => (
              <div
                key={run.id}
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: '12px',
                  padding: '12px 0',
                  borderBottom: '1px solid var(--color-border-tertiary)',
                }}
              >
                <div style={{
                  width: '8px',
                  height: '8px',
                  borderRadius: '50%',
                  marginTop: '6px',
                  flexShrink: 0,
                  background: '#BA7517',
                }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', marginBottom: '2px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <span>⚡ {run.skill_name}</span>
                    <span style={{ color: 'var(--color-text-tertiary)' }}>·</span>
                    <span style={STATUS_COLOR[run.status] ?? { color: 'var(--color-text-secondary)' }}>{run.status}</span>
                    <span style={{ color: 'var(--color-text-tertiary)' }}>·</span>
                    <span>{run.triggered_by}</span>
                    <span style={{ color: 'var(--color-text-tertiary)' }}>·</span>
                    <span>{formatRelativeTime(run.started_at, lang)}</span>
                  </div>
                  {run.output && (
                    <div style={{ fontSize: '13px', color: 'var(--color-text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {run.output.slice(0, 120)}
                    </div>
                  )}
                </div>
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0, alignSelf: 'center' }}>⚡ 技能</div>
              </div>
            ))}
          </div>
        )
      ) : (
        /* ── Conversation events ── */
        filteredEvents.length === 0 ? (
          <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)' }}>{t('timeline_empty', lang)}</div>
        ) : (
          <div style={{ flex: 1, overflowY: 'auto' }}>
            {filteredEvents.map((evt, i) => (
              <div
                key={i}
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: '12px',
                  padding: '12px 0',
                  borderBottom: '1px solid var(--color-border-tertiary)',
                }}
              >
                <div style={{
                  width: '8px',
                  height: '8px',
                  borderRadius: '50%',
                  marginTop: '6px',
                  flexShrink: 0,
                  background: pupAccentColor(evt.pup_key),
                }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginBottom: '2px' }}>
                    {evt.pup_name} · {formatRelativeTime(evt.timestamp, lang)}
                  </div>
                  <div style={{ fontSize: '12.5px', color: 'var(--color-text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {evt.content}
                  </div>
                </div>
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0, alignSelf: 'center' }}>
                  ~{Math.ceil(evt.content.length / 4)} tokens
                </div>
              </div>
            ))}
          </div>
        )
      )}
    </div>
  );
};
