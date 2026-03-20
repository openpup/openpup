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

const STATUS_COLOR: Record<string, React.CSSProperties> = {
  pending:     { background: 'var(--color-background-secondary)', color: 'var(--color-text-tertiary)' },
  in_progress: { background: 'var(--color-background-info)',      color: 'var(--color-text-info)' },
  done:        { background: 'var(--color-background-success)',   color: 'var(--color-text-success)' },
  failed:      { background: 'var(--color-background-danger)',    color: 'var(--color-text-danger)' },
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
    <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', fontSize: '13px' }}>
      <h3 style={{ fontSize: '15px', fontWeight: 500, color: 'var(--color-text-primary)', margin: 0 }}>
        {t('task_title', lang)}
      </h3>
      {error && (
        <p style={{
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '8px 12px',
          borderRadius: '8px',
          margin: 0,
        }}>
          {error}
        </p>
      )}

      {/* Add task */}
      <div style={{
        borderRadius: '12px',
        border: '1px solid var(--color-border-tertiary)',
        background: 'var(--color-background-secondary)',
        padding: '12px 16px',
        display: 'flex',
        flexDirection: 'column',
        gap: '10px',
      }}>
        <div style={{ fontWeight: 500, color: 'var(--color-text-secondary)' }}>{t('task_new', lang)}</div>
        <input
          style={{
            width: '100%',
            borderRadius: '8px',
            background: 'var(--color-background-primary)',
            border: '1px solid var(--color-border-secondary)',
            padding: '8px 12px',
            fontSize: '13px',
            color: 'var(--color-text-primary)',
            outline: 'none',
            boxSizing: 'border-box',
          }}
          placeholder={t('task_desc_placeholder', lang)}
          value={newDesc}
          onChange={(e) => setNewDesc(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') void addTask(); }}
        />
        <div style={{ display: 'flex', gap: '8px' }}>
          <input
            style={{
              flex: 1,
              borderRadius: '8px',
              background: 'var(--color-background-primary)',
              border: '1px solid var(--color-border-secondary)',
              padding: '8px 12px',
              fontSize: '13px',
              color: 'var(--color-text-primary)',
              outline: 'none',
            }}
            placeholder={t('task_pup_placeholder', lang)}
            value={newPup}
            onChange={(e) => setNewPup(e.target.value)}
          />
          <button
            style={{
              padding: '8px 12px',
              borderRadius: '8px',
              background: '#1D9E75',
              color: '#ffffff',
              fontWeight: 500,
              fontSize: '13px',
              border: 'none',
              cursor: 'pointer',
              opacity: adding || !newDesc.trim() ? 0.5 : 1,
            }}
            disabled={adding || !newDesc.trim()}
            onClick={() => void addTask()}
          >
            {adding ? t('task_adding', lang) : t('task_add', lang)}
          </button>
        </div>
      </div>

      {loading ? (
        <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>加载中…</p>
      ) : tasks.length === 0 ? (
        <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>{t('task_empty', lang)}</p>
      ) : (
        <>
          {pending.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <div style={{ fontWeight: 500, color: 'var(--color-text-tertiary)' }}>
                {t('task_active', lang)} ({pending.length})
              </div>
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
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <div style={{ fontWeight: 500, color: 'var(--color-text-tertiary)' }}>
                {t('task_done_section', lang)} ({done.length})
              </div>
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
    <div style={{
      borderRadius: '12px',
      border: '1px solid var(--color-border-tertiary)',
      padding: '12px 16px',
      display: 'flex',
      flexDirection: 'column',
      gap: '6px',
      background: 'var(--color-background-secondary)',
      opacity: isDone ? 0.6 : 1,
    }}>
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: '8px' }}>
        <button
          onClick={onToggle}
          style={{ flex: 1, minWidth: 0, textAlign: 'left', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
        >
          <span style={{
            display: 'inline-flex',
            padding: '2px 8px',
            borderRadius: '9999px',
            fontSize: '11px',
            marginRight: '8px',
            fontWeight: 500,
            ...(STATUS_COLOR[task.status] ?? STATUS_COLOR.pending),
          }}>
            {statusLabel(task.status)}
          </span>
          <span style={{
            color: isDone ? 'var(--color-text-tertiary)' : 'var(--color-text-primary)',
            textDecoration: isDone ? 'line-through' : 'none',
            fontSize: '13px',
          }}>
            {task.description}
          </span>
          {task.assigned_pup && (
            <span style={{ marginLeft: '8px', color: 'var(--color-text-tertiary)', fontSize: '13px' }}>
              → {task.assigned_pup}
            </span>
          )}
        </button>
        <div style={{ display: 'flex', alignItems: 'center', gap: '4px', flexShrink: 0 }}>
          {!isDone && (
            <>
              <button
                onClick={() => onSetStatus(task.id, 'in_progress')}
                style={{
                  padding: '2px 8px',
                  borderRadius: '8px',
                  background: 'var(--color-background-secondary)',
                  color: 'var(--color-text-primary)',
                  fontSize: '11px',
                  border: '1px solid var(--color-border-secondary)',
                  cursor: 'pointer',
                }}
              >
                {t('task_start', _lang as Parameters<typeof t>[1])}
              </button>
              <button
                onClick={() => onSetStatus(task.id, 'done')}
                style={{
                  padding: '2px 8px',
                  borderRadius: '8px',
                  background: 'var(--color-background-secondary)',
                  color: 'var(--color-text-primary)',
                  fontSize: '11px',
                  border: '1px solid var(--color-border-secondary)',
                  cursor: 'pointer',
                }}
              >
                {t('task_complete', _lang as Parameters<typeof t>[1])}
              </button>
            </>
          )}
          {isDone && (
            <button
              onClick={() => onSetStatus(task.id, 'pending')}
              style={{
                padding: '2px 8px',
                borderRadius: '8px',
                background: 'var(--color-background-secondary)',
                color: 'var(--color-text-primary)',
                fontSize: '11px',
                border: '1px solid var(--color-border-secondary)',
                cursor: 'pointer',
              }}
            >
              {t('task_reopen', _lang as Parameters<typeof t>[1])}
            </button>
          )}
          <button
            onClick={() => onDelete(task.id)}
            style={{
              padding: '2px 8px',
              borderRadius: '8px',
              background: 'var(--color-background-secondary)',
              color: 'var(--color-text-danger)',
              fontSize: '11px',
              border: '1px solid var(--color-border-secondary)',
              cursor: 'pointer',
            }}
          >
            {t('task_delete', _lang as Parameters<typeof t>[1])}
          </button>
        </div>
      </div>
      {expanded && (
        <div style={{ paddingLeft: '4px', display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
          <div>{t('task_created_at', _lang as Parameters<typeof t>[1])}：{relativeTime(task.created_at)}</div>
          {task.completed_at && <div>{t('task_completed_at', _lang as Parameters<typeof t>[1])}：{relativeTime(task.completed_at)}</div>}
          {task.result && <div style={{ color: 'var(--color-text-secondary)' }}>{task.result}</div>}
        </div>
      )}
    </div>
  );
};
