import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useLang, t } from '../i18n';

interface TimelineEvent {
  role: string;
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

function relativeTime(ts: number): string {
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return '刚刚';
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return `${Math.floor(diff / 86400)} 天前`;
}

const STATUS_COLOR: Record<string, string> = {
  completed: 'text-emerald-400',
  failed: 'text-red-400',
  running: 'text-yellow-400',
};

const PUP_COLORS: Record<string, string> = {
  Alpha: 'bg-emerald-500',
  You: 'bg-amber-500',
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
    if (filter === 'alpha') return e.pup_name === 'Alpha';
    if (filter === 'you') return e.pup_name === 'You';
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

  return (
    <div className="h-full flex flex-col">
      {/* Search bar */}
      <div className="flex gap-2 mb-3">
        <input
          className="flex-1 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
          placeholder={t('timeline_search', lang)}
          value={searchQuery}
          onChange={(e) => {
            setSearchQuery(e.target.value);
            if (!e.target.value.trim()) setSearchResults(null);
          }}
          onKeyDown={(e) => { if (e.key === 'Enter') void doSearch(searchQuery); }}
        />
        <button
          className="px-3 py-2 rounded-lg bg-stone-800 border border-stone-700 text-stone-300 text-xs hover:bg-stone-700 disabled:opacity-50 transition-colors"
          disabled={searching}
          onClick={() => void doSearch(searchQuery)}
        >
          {searching ? '…' : t('timeline_search_btn', lang)}
        </button>
        {searchResults !== null && (
          <button
            className="px-2 py-1 rounded-lg text-stone-500 text-xs hover:text-stone-300 transition-colors"
            onClick={() => { setSearchResults(null); setSearchQuery(''); }}
          >
            {t('timeline_clear', lang)}
          </button>
        )}
      </div>

      {/* Filter tabs — hidden while search is active */}
      {searchResults === null && (
        <div className="flex gap-1.5 mb-4">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setFilter(tab.key)}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                filter === tab.key
                  ? 'bg-stone-800 text-stone-100'
                  : 'text-stone-400 hover:text-stone-300'
              }`}
            >
              {tab.label}
            </button>
          ))}
          <button
            className="ml-auto text-xs text-stone-500 hover:text-stone-400 transition-colors"
            onClick={() => void load()}
          >
            {t('timeline_refresh', lang)}
          </button>
        </div>
      )}

      {/* Search results */}
      {searchResults !== null ? (
        searchResults.length === 0 ? (
          <div className="text-xs text-stone-500">{t('timeline_no_results', lang)}</div>
        ) : (
          <div className="flex-1 overflow-auto space-y-0 divide-y divide-stone-800">
            {searchResults.map((evt, i) => (
              <div key={i} className="flex items-start gap-3 py-3">
                <div className={`w-2 h-2 rounded-full mt-1.5 shrink-0 ${PUP_COLORS[evt.pup_name] ?? 'bg-stone-500'}`} />
                <div className="flex-1 min-w-0">
                  <div className="text-xs text-stone-500 mb-0.5">{evt.pup_name} · {relativeTime(evt.timestamp)}</div>
                  <div className="text-sm text-stone-200 truncate">{evt.content}</div>
                </div>
                <div className="text-[10px] text-stone-600 shrink-0 self-center">~{Math.ceil(evt.content.length / 4)} tokens</div>
              </div>
            ))}
          </div>
        )
      ) : loading ? (
        <div className="text-xs text-stone-500">加载中…</div>
      ) : filter === 'skills' ? (
        /* ── Skill runs ── */
        skillRuns.length === 0 ? (
          <div className="text-xs text-stone-500">{t('timeline_empty', lang)}</div>
        ) : (
          <div className="flex-1 overflow-auto space-y-0 divide-y divide-stone-800">
            {skillRuns.map((run) => (
              <div key={run.id} className="flex items-start gap-3 py-3">
                <div className="w-2 h-2 rounded-full mt-1.5 shrink-0 bg-amber-500" />
                <div className="flex-1 min-w-0">
                  <div className="text-xs text-stone-500 mb-0.5 flex items-center gap-2">
                    <span>⚡ {run.skill_name}</span>
                    <span className="text-stone-600">·</span>
                    <span className={STATUS_COLOR[run.status] ?? 'text-stone-400'}>{run.status}</span>
                    <span className="text-stone-600">·</span>
                    <span>{run.triggered_by}</span>
                    <span className="text-stone-600">·</span>
                    <span>{relativeTime(run.started_at)}</span>
                  </div>
                  {run.output && (
                    <div className="text-xs text-stone-400 truncate">{run.output.slice(0, 120)}</div>
                  )}
                </div>
                <div className="text-[10px] text-stone-600 shrink-0 self-center">⚡ 技能</div>
              </div>
            ))}
          </div>
        )
      ) : (
        /* ── Conversation events ── */
        filteredEvents.length === 0 ? (
          <div className="text-xs text-stone-500">{t('timeline_empty', lang)}</div>
        ) : (
          <div className="flex-1 overflow-auto space-y-0 divide-y divide-stone-800">
            {filteredEvents.map((evt, i) => (
              <div key={i} className="flex items-start gap-3 py-3">
                <div className={`w-2 h-2 rounded-full mt-1.5 shrink-0 ${PUP_COLORS[evt.pup_name] ?? 'bg-stone-500'}`} />
                <div className="flex-1 min-w-0">
                  <div className="text-xs text-stone-500 mb-0.5">{evt.pup_name} · {relativeTime(evt.timestamp)}</div>
                  <div className="text-sm text-stone-200 truncate">{evt.content}</div>
                </div>
                <div className="text-[10px] text-stone-600 shrink-0 self-center">~{Math.ceil(evt.content.length / 4)} tokens</div>
              </div>
            ))}
          </div>
        )
      )}
    </div>
  );
};
