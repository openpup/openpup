import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import cronstrue from 'cronstrue/i18n';
import { useLang, t } from '../i18n';
import { formatRelativeTime, formatScheduleTime } from '../utils/locale';

interface TaskItem {
  id: string;
  description: string;
  assigned_pup: string | null;
  status: string;
  created_at: number;
  completed_at: number | null;
  result: string | null;
}

interface JobStep {
  skill: string;
  input: string;
}

interface NotifyConfig {
  when: 'always' | 'on_failure' | 'never';
  channels: string[];
}

interface ScheduledJobView {
  id: string;
  name: string;
  schedule: string;
  enabled: boolean;
  created_at: number;
  mode: string;
  steps: JobStep[];
  notify: NotifyConfig;
  next_run: number | null;
  last_status: string | null;
  last_run_at: number | null;
  total_runs: number;
  fail_count: number;
}

const STATUS_COLOR: Record<string, React.CSSProperties> = {
  pending:     { background: 'var(--color-background-secondary)', color: 'var(--color-text-tertiary)' },
  in_progress: { background: 'var(--color-background-info)',      color: 'var(--color-text-info)' },
  done:        { background: 'var(--color-background-success)',   color: 'var(--color-text-success)' },
  failed:      { background: 'var(--color-background-danger)',    color: 'var(--color-text-danger)' },
};

type TabKey = 'manual' | 'scheduled';

export const TaskManager: React.FC = () => {
  const { lang } = useLang();
  const [tab, setTab] = useState<TabKey>('manual');
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const [newDesc, setNewDesc] = useState('');
  const [newPup, setNewPup] = useState('');
  const [adding, setAdding] = useState(false);

  // Scheduled jobs state
  const [jobs, setJobs] = useState<ScheduledJobView[]>([]);
  const [jobsLoading, setJobsLoading] = useState(true);

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

  const loadJobs = useCallback(async () => {
    setJobsLoading(true);
    try {
      const data = await invoke<ScheduledJobView[]>('list_scheduled_jobs');
      setJobs(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setJobsLoading(false);
    }
  }, []);

  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (tab === 'scheduled') void loadJobs();
  }, [tab, loadJobs]);

  // Listen for job completion events to auto-refresh
  useEffect(() => {
    const unlisten = listen('scheduled_job_completed', () => {
      if (tab === 'scheduled') void loadJobs();
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, [tab, loadJobs]);

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

  const toggleJob = async (id: string, enabled: boolean) => {
    try {
      await invoke('toggle_scheduled_job', { id, enabled });
      setJobs((prev) => prev.map((j) => j.id === id ? { ...j, enabled } : j));
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteJob = async (id: string) => {
    try {
      await invoke('delete_scheduled_job', { id });
      setJobs((prev) => prev.filter((j) => j.id !== id));
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

  const tabStyle = (active: boolean): React.CSSProperties => ({
    padding: '6px 16px',
    borderRadius: '8px',
    fontSize: '13px',
    fontWeight: 500,
    border: 'none',
    cursor: 'pointer',
    background: active ? 'var(--color-background-secondary)' : 'transparent',
    color: active ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)',
  });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', fontSize: '13px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <h3 style={{ fontSize: '15px', fontWeight: 500, color: 'var(--color-text-primary)', margin: 0, marginRight: '8px' }}>
          {t('task_title', lang)}
        </h3>
        <button style={tabStyle(tab === 'manual')} onClick={() => setTab('manual')}>
          手动
        </button>
        <button style={tabStyle(tab === 'scheduled')} onClick={() => setTab('scheduled')}>
          定时
        </button>
      </div>

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

      {tab === 'manual' && (
        <>
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
        </>
      )}

      {tab === 'scheduled' && (
        <>
          {jobsLoading ? (
            <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>加载中…</p>
          ) : jobs.length === 0 ? (
            <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>暂无定时任务</p>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              {jobs.map((job) => (
                <ScheduledJobCard
                  key={job.id}
                  job={job}
                  onToggle={toggleJob}
                  onDelete={deleteJob}
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

// ─── Scheduled Job Card ────────────────────────────────────────────────────────

const cronHumanLabel = (schedule: string, lang: string): string => {
  try {
    const locale = lang.startsWith('zh') ? 'zh_CN' : 'en';
    return cronstrue.toString(schedule, { locale, use24HourTimeFormat: true });
  } catch {
    return schedule;
  }
};

const ScheduledJobCard: React.FC<{
  job: ScheduledJobView;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  lang: string;
}> = ({ job, onToggle, onDelete, lang }) => {
  const successCount = job.total_runs - job.fail_count;
  const successRate = job.total_runs > 0 ? Math.round((successCount / job.total_runs) * 100) : 0;

  const lastStatusStyle: React.CSSProperties = (() => {
    if (!job.last_status) return { background: 'var(--color-background-secondary)', color: 'var(--color-text-tertiary)' };
    if (job.last_status === 'completed') return { background: 'var(--color-background-success)', color: 'var(--color-text-success)' };
    if (job.last_status === 'failed') return { background: 'var(--color-background-danger)', color: 'var(--color-text-danger)' };
    return { background: 'var(--color-background-info)', color: 'var(--color-text-info)' };
  })();

  const lastStatusLabel = (() => {
    if (!job.last_status) return '从未执行';
    if (job.last_status === 'completed') return '成功';
    if (job.last_status === 'failed') return '失败';
    return job.last_status;
  })();

  return (
    <div style={{
      borderRadius: '12px',
      border: '1px solid var(--color-border-tertiary)',
      padding: '12px 16px',
      display: 'flex',
      flexDirection: 'column',
      gap: '8px',
      background: 'var(--color-background-secondary)',
      opacity: job.enabled ? 1 : 0.6,
    }}>
      {/* Header row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <span style={{ flex: 1, fontWeight: 500, color: 'var(--color-text-primary)', fontSize: '13px' }}>
          {job.name}
        </span>
        <span style={{
          display: 'inline-flex',
          padding: '2px 8px',
          borderRadius: '9999px',
          fontSize: '11px',
          fontWeight: 500,
          ...lastStatusStyle,
        }}>
          {lastStatusLabel}
        </span>
      </div>

      {/* Details row */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '12px', fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
        <span title={job.schedule}>
          {cronHumanLabel(job.schedule, lang)}
        </span>
        {job.next_run && (
          <span>
            下次: {formatScheduleTime(job.next_run, lang as Parameters<typeof formatScheduleTime>[1])}
          </span>
        )}
        {job.last_run_at && (
          <span>
            上次: {formatRelativeTime(job.last_run_at, lang as Parameters<typeof formatRelativeTime>[1])}
          </span>
        )}
        {job.total_runs > 0 && (
          <span>
            {successCount}/{job.total_runs} ({successRate}%)
          </span>
        )}
        {job.notify && job.notify.when !== 'never' && (
          <span>
            {job.notify.when === 'always' ? '🔔 始终通知' : '🔔 失败通知'}
            {job.notify.channels.length > 0 && ` · ${job.notify.channels.map(c => c === 'weixin' ? '微信' : c === 'qqbot' ? 'QQ' : c).join(', ')}`}
          </span>
        )}
      </div>

      {/* Actions row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
        <button
          onClick={() => onToggle(job.id, !job.enabled)}
          style={{
            padding: '2px 8px',
            borderRadius: '8px',
            background: 'var(--color-background-secondary)',
            color: job.enabled ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)',
            fontSize: '11px',
            border: '1px solid var(--color-border-secondary)',
            cursor: 'pointer',
          }}
        >
          {job.enabled ? '暂停' : '启用'}
        </button>
        <button
          onClick={() => onDelete(job.id)}
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
          {t('task_delete', lang as Parameters<typeof t>[1])}
        </button>
      </div>
    </div>
  );
};

// ─── Task Card ─────────────────────────────────────────────────────────────────

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
          <div>{t('task_created_at', _lang as Parameters<typeof t>[1])}：{formatRelativeTime(task.created_at, _lang as Parameters<typeof t>[1])}</div>
          {task.completed_at && <div>{t('task_completed_at', _lang as Parameters<typeof t>[1])}：{formatRelativeTime(task.completed_at, _lang as Parameters<typeof t>[1])}</div>}
          {task.result && <div style={{ color: 'var(--color-text-secondary)' }}>{task.result}</div>}
        </div>
      )}
    </div>
  );
};
