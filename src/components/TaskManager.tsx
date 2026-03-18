import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface TaskItem {
  id: string;
  description: string;
  assigned_pup: string | null;
  status: string;
  created_at: number;
  completed_at: number | null;
  result: string | null;
}

function relativeTime(ts: number): string {
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return '刚刚';
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return `${Math.floor(diff / 86400)} 天前`;
}

const STATUS_COLOR: Record<string, string> = {
  pending: 'bg-stone-700 text-stone-300',
  in_progress: 'bg-amber-900/60 text-amber-300',
  done: 'bg-emerald-900/60 text-emerald-300',
  failed: 'bg-red-900/60 text-red-300',
};

export const TaskManager: React.FC = () => {
  const { lang } = useLang();
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const [newDesc, setNewDesc] = useState('');
  const [newPup, setNewPup] = useState('');
  const [adding, setAdding] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const data = await invoke<TaskItem[]>('list_tasks', { limit: 100 });
      setTasks(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void load(); }, []);

  const addTask = async () => {
    if (!newDesc.trim()) return;
    setAdding(true);
    setError(null);
    try {
      await invoke('create_task', {
        description: newDesc.trim(),
        assignedPup: newPup.trim() || null,
      });
      setNewDesc('');
      setNewPup('');
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  const setStatus = async (id: string, status: string) => {
    try {
      await invoke('update_task_status', { id, status, result: null });
      setTasks((prev) => prev.map((task) => task.id === id ? { ...task, status } : task));
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteTask = async (id: string) => {
    try {
      await invoke('delete_task', { id });
      setTasks((prev) => prev.filter((task) => task.id !== id));
    } catch (e) {
      setError(String(e));
    }
  };

  const pending = tasks.filter((task) => task.status === 'pending' || task.status === 'in_progress');
  const done = tasks.filter((task) => task.status === 'done' || task.status === 'failed');

  const statusLabel = (status: string) => {
    const map: Record<string, string> = {
      pending: t('task_pending', lang),
      in_progress: t('task_in_progress', lang),
      done: t('task_done', lang),
      failed: t('task_failed', lang),
    };
    return map[status] ?? status;
  };

  return (
    <div className="flex flex-col gap-4 text-xs">
      <h3 className="text-sm font-semibold text-stone-100">{t('task_title', lang)}</h3>
      {error && <p className="text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{error}</p>}

      {/* Add task */}
      <div className="rounded-xl border border-stone-800 bg-stone-900/60 px-4 py-3 space-y-2.5">
        <div className="font-medium text-stone-300">{t('task_new', lang)}</div>
        <input
          className="w-full rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
          placeholder={t('task_desc_placeholder', lang)}
          value={newDesc}
          onChange={(e) => setNewDesc(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') void addTask(); }}
        />
        <div className="flex gap-2">
          <input
            className="flex-1 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
            placeholder={t('task_pup_placeholder', lang)}
            value={newPup}
            onChange={(e) => setNewPup(e.target.value)}
          />
          <button
            className="px-3 py-2 rounded-lg bg-amber-500 text-stone-950 font-medium disabled:opacity-50 hover:bg-amber-400 transition-colors"
            disabled={adding || !newDesc.trim()}
            onClick={() => void addTask()}
          >
            {adding ? t('task_adding', lang) : t('task_add', lang)}
          </button>
        </div>
      </div>

      {loading ? (
        <p className="text-stone-500">加载中…</p>
      ) : tasks.length === 0 ? (
        <p className="text-stone-500">{t('task_empty', lang)}</p>
      ) : (
        <>
          {pending.length > 0 && (
            <div className="space-y-2">
              <div className="text-stone-500 font-medium">{t('task_active', lang)} ({pending.length})</div>
              {pending.map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  expanded={expanded === task.id}
                  onToggle={() => setExpanded(expanded === task.id ? null : task.id)}
                  onSetStatus={setStatus}
                  onDelete={deleteTask}
                  statusLabel={statusLabel}
                  lang={lang}
                />
              ))}
            </div>
          )}

          {done.length > 0 && (
            <div className="space-y-2">
              <div className="text-stone-500 font-medium">{t('task_done_section', lang)} ({done.length})</div>
              {done.map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  expanded={expanded === task.id}
                  onToggle={() => setExpanded(expanded === task.id ? null : task.id)}
                  onSetStatus={setStatus}
                  onDelete={deleteTask}
                  statusLabel={statusLabel}
                  lang={lang}
                />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
};

const TaskCard: React.FC<{
  task: TaskItem;
  expanded: boolean;
  onToggle: () => void;
  onSetStatus: (id: string, status: string) => void;
  onDelete: (id: string) => void;
  statusLabel: (s: string) => string;
  lang: string;
}> = ({ task, expanded, onToggle, onSetStatus, onDelete, statusLabel, lang: _lang }) => {
  const isDone = task.status === 'done' || task.status === 'failed';

  return (
    <div className={`rounded-xl border px-4 py-3 space-y-1.5 transition-colors ${isDone ? 'border-stone-800 opacity-60' : 'border-stone-700 bg-stone-900/40'}`}>
      <div className="flex items-start gap-2">
        <button onClick={onToggle} className="flex-1 min-w-0 text-left">
          <span className={`inline-flex px-2 py-0.5 rounded-full text-[10px] mr-2 font-medium ${STATUS_COLOR[task.status] ?? 'bg-stone-700 text-stone-300'}`}>
            {statusLabel(task.status)}
          </span>
          <span className={isDone ? 'line-through text-stone-500' : 'text-stone-200'}>
            {task.description}
          </span>
          {task.assigned_pup && (
            <span className="ml-2 text-stone-600">→ {task.assigned_pup}</span>
          )}
        </button>
        <div className="flex items-center gap-1 shrink-0">
          {!isDone && (
            <>
              <button
                onClick={() => onSetStatus(task.id, 'in_progress')}
                className="px-2 py-0.5 rounded-lg bg-amber-900/40 text-amber-300 text-[10px] hover:bg-amber-900/60 transition-colors"
              >
                {t('task_start', _lang as Parameters<typeof t>[1])}
              </button>
              <button
                onClick={() => onSetStatus(task.id, 'done')}
                className="px-2 py-0.5 rounded-lg bg-emerald-900/40 text-emerald-300 text-[10px] hover:bg-emerald-900/60 transition-colors"
              >
                {t('task_complete', _lang as Parameters<typeof t>[1])}
              </button>
            </>
          )}
          {isDone && (
            <button
              onClick={() => onSetStatus(task.id, 'pending')}
              className="px-2 py-0.5 rounded-lg bg-stone-700 text-stone-400 text-[10px] hover:bg-stone-600 transition-colors"
            >
              {t('task_reopen', _lang as Parameters<typeof t>[1])}
            </button>
          )}
          <button
            onClick={() => onDelete(task.id)}
            className="px-2 py-0.5 rounded-lg bg-red-900/40 text-red-400 text-[10px] hover:bg-red-900/60 transition-colors"
          >
            {t('task_delete', _lang as Parameters<typeof t>[1])}
          </button>
        </div>
      </div>
      {expanded && (
        <div className="pl-1 space-y-1 text-[11px] text-stone-500">
          <div>{t('task_created_at', _lang as Parameters<typeof t>[1])}：{relativeTime(task.created_at)}</div>
          {task.completed_at && <div>{t('task_completed_at', _lang as Parameters<typeof t>[1])}：{relativeTime(task.completed_at)}</div>}
          {task.result && <div className="text-stone-400">{task.result}</div>}
        </div>
      )}
    </div>
  );
};
