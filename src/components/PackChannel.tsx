import React from 'react';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ContextInspector } from './ContextInspector';
import type { ChannelMessageRecord, ChannelRecord, DelegationPlan } from '../types/channel';
import { useLang, type Lang, t } from '../i18n';
import { formatDateTime, formatRelativeTime } from '../utils/locale';

// ─── Types ────────────────────────────────────────────────────────────────────

type ChannelMessage = ChannelMessageRecord;
type Channel = ChannelRecord;
type PlanStepState = 'waiting' | 'running' | 'done' | 'failed';

interface PlanStepView {
  id: string;
  pup: string;
  description: string;
  depends_on: string[];
  latestText: string;
  state: PlanStepState;
}

interface DagNodeLayout extends PlanStepView {
  column: number;
  row: number;
  x: number;
  y: number;
}

interface DagMergeLayout {
  id: string;
  targetId: string;
  x: number;
  y: number;
  dependsOn: string[];
}

interface DagEdgeLayout {
  id: string;
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
  colorState: PlanStepState;
}

// ─── Pup accent colors ────────────────────────────────────────────────────────

const PUP_ACCENT: Record<string, string> = {
  // by display name
  'Alpha':          '#1D9E75',
  'Dev Pup':        '#378ADD',
  'Writer Pup':     '#BA7517',
  'Research Pup':   '#7F77DD',
  'Ops Pup':        '#888780',
  'Life Admin Pup': '#DB3491',
  // by key
  alpha:      '#1D9E75',
  dev:        '#378ADD',
  writer:     '#BA7517',
  research:   '#7F77DD',
  ops:        '#888780',
  life_admin: '#DB3491',
};

function pupAccent(name: string, customPupColors: Record<string, string> = {}): string {
  return customPupColors[name] ?? PUP_ACCENT[name] ?? '#1D9E75';
}

// ─── Relative time ────────────────────────────────────────────────────────────

function relativeTime(ts: number, lang: Lang): string {
  return formatRelativeTime(ts, lang, { olderThanDay: 'date', includeYesterdayTime: true });
}

function absoluteTime(ts: number | null | undefined, lang: Lang): string {
  if (!ts) return '—';
  return formatDateTime(ts, lang);
}

function messageLabel(msg: ChannelMessage): string {
  if (msg.msg_type === 'status') {
    return (msg.status_val ?? msg.content).trim();
  }
  return msg.content.trim();
}

function statusDisplayLabel(value: string | null | undefined, lang: Lang): string {
  switch ((value ?? '').toLowerCase()) {
    case 'started':
      return t('pack_status_started', lang);
    case 'done':
    case 'completed':
      return t('pack_status_done', lang);
    case 'failed':
      return t('pack_status_failed', lang);
    case 'blocked':
      return t('pack_status_blocked', lang);
    default:
      return (value ?? '').trim();
  }
}

function latestStatusForSender(sender: string, statusMessages: ChannelMessage[]): ChannelMessage | undefined {
  const lowered = sender.toLowerCase();
  return [...statusMessages].reverse().find((msg) => msg.sender.toLowerCase() === lowered);
}

function inferMemberState(
  member: string,
  statusMessages: ChannelMessage[],
  completed: boolean,
  lang: Lang,
): { label: string; tone: 'idle' | 'running' | 'done' } {
  if (completed) return { label: t('pack_member_done', lang), tone: 'done' };
  const latest = latestStatusForSender(member, statusMessages);
  if (!latest) return { label: t('pack_member_idle', lang), tone: 'idle' };
  const text = (latest.status_val ?? '').toLowerCase();
  if (text === 'done' || text === 'completed') {
    return { label: t('pack_member_delivered', lang), tone: 'done' };
  }
  if (text === 'failed' || text === 'blocked') {
    return { label: t('pack_member_issue', lang), tone: 'idle' };
  }
  return { label: t('pack_member_running', lang), tone: 'running' };
}

function extractCurrentStage(statusMessages: ChannelMessage[], completed: boolean, lang: Lang): string {
  if (completed) return t('pack_stage_wrapped', lang);
  const latest = [...statusMessages].reverse()[0];
  if (!latest) return t('pack_stage_waiting', lang);
  const content = `${latest.sender} ${statusDisplayLabel(latest.status_val ?? latest.content, lang)}`.trim();
  return content.length > 28 ? `${content.slice(0, 28)}…` : content;
}

function isNearBottom(el: HTMLDivElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 56;
}

function layoutPlanDag(steps: PlanStepView[], lang: Lang) {
  const nodeWidth = 208;
  const nodeHeight = 102;
  const columnGap = 74;
  const rowGap = 16;
  const paddingX = 28;
  const paddingY = 24;

  const nodeIdByPup = new Map<string, string>();
  steps.forEach((step) => nodeIdByPup.set(step.pup, step.id));

  const dependencies = new Map<string, string[]>();
  const dependents = new Map<string, string[]>();
  steps.forEach((step) => {
    const deps = step.depends_on
      .map((pup) => nodeIdByPup.get(pup))
      .filter((value): value is string => Boolean(value));
    dependencies.set(step.id, deps);
    deps.forEach((depId) => {
      const next = dependents.get(depId) ?? [];
      next.push(step.id);
      dependents.set(depId, next);
    });
  });

  const memoDepth = new Map<string, number>();
  const visiting = new Set<string>();
  const depthFor = (id: string): number => {
    if (memoDepth.has(id)) return memoDepth.get(id) ?? 0;
    if (visiting.has(id)) return 0;
    visiting.add(id);
    const deps = dependencies.get(id) ?? [];
    const depth = deps.length === 0 ? 0 : Math.max(...deps.map((depId) => depthFor(depId))) + 1;
    visiting.delete(id);
    memoDepth.set(id, depth);
    return depth;
  };

  const columns = new Map<number, PlanStepView[]>();
  steps.forEach((step) => {
    const depth = depthFor(step.id);
    const bucket = columns.get(depth) ?? [];
    bucket.push(step);
    columns.set(depth, bucket);
  });

  const sortedDepths = [...columns.keys()].sort((a, b) => a - b);
  sortedDepths.forEach((depth) => {
    const bucket = columns.get(depth) ?? [];
    bucket.sort((a, b) => {
      const stateOrder: Record<PlanStepState, number> = { running: 0, waiting: 1, failed: 2, done: 3 };
      return stateOrder[a.state] - stateOrder[b.state] || a.pup.localeCompare(b.pup);
    });
  });

  const nodes: DagNodeLayout[] = [];
  sortedDepths.forEach((depth) => {
    const bucket = columns.get(depth) ?? [];
    bucket.forEach((step, row) => {
      nodes.push({
        ...step,
        column: depth,
        row,
        x: paddingX + depth * (nodeWidth + columnGap),
        y: paddingY + row * (nodeHeight + rowGap),
      });
    });
  });

  const nodeMap = new Map(nodes.map((node) => [node.id, node]));
  const merges: DagMergeLayout[] = [];
  const edges: DagEdgeLayout[] = [];

  steps.forEach((step) => {
    const target = nodeMap.get(step.id);
    if (!target) return;
    const deps = dependencies.get(step.id) ?? [];
    if (deps.length === 0) return;

    if (deps.length === 1) {
      const source = nodeMap.get(deps[0]);
      if (!source) return;
      edges.push({
        id: `${source.id}->${target.id}`,
        fromX: source.x + nodeWidth,
        fromY: source.y + nodeHeight / 2,
        toX: target.x,
        toY: target.y + nodeHeight / 2,
        colorState: target.state,
      });
      return;
    }

    const mergeX = target.x - 28;
    const mergeY = target.y + nodeHeight / 2;
    merges.push({
      id: `merge-${target.id}`,
      targetId: target.id,
      x: mergeX,
      y: mergeY,
      dependsOn: step.depends_on,
    });

    deps.forEach((depId) => {
      const source = nodeMap.get(depId);
      if (!source) return;
      edges.push({
        id: `${depId}->merge-${target.id}`,
        fromX: source.x + nodeWidth,
        fromY: source.y + nodeHeight / 2,
        toX: mergeX,
        toY: mergeY,
        colorState: target.state,
      });
    });

    edges.push({
      id: `merge-${target.id}->${target.id}`,
      fromX: mergeX,
      fromY: mergeY,
      toX: target.x,
      toY: target.y + nodeHeight / 2,
      colorState: target.state,
    });
  });

  const columnHeights = sortedDepths.map((depth) => (columns.get(depth)?.length ?? 0));
  const maxRows = Math.max(1, ...columnHeights);
  const width = paddingX * 2 + Math.max(1, sortedDepths.length) * nodeWidth + Math.max(0, sortedDepths.length - 1) * columnGap;
  const height = paddingY * 2 + maxRows * nodeHeight + Math.max(0, maxRows - 1) * rowGap;

  return {
    nodes,
    merges,
    edges,
    width,
    height,
    nodeWidth,
    nodeHeight,
    layers: sortedDepths.map((depth) => ({
      depth,
      title: depth === 0
        ? t('pack_layer_start', lang)
        : `${t('pack_layer_prefix', lang)} ${depth + 1}${t('pack_layer_suffix', lang) ? ` ${t('pack_layer_suffix', lang)}` : ''}`.trim(),
      count: columns.get(depth)?.length ?? 0,
    })),
  };
}

// ─── Sub-components ───────────────────────────────────────────────────────────

interface TextMessageProps {
  msg: ChannelMessage;
}

const TextMessage: React.FC<TextMessageProps & { customPupColors?: Record<string, string> }> = ({ msg, customPupColors = {} }) => {
  const { lang } = useLang();
  const accent = pupAccent(msg.sender, customPupColors);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
        <span style={{
          fontSize: '11px', fontWeight: 500,
          padding: '1px 7px', borderRadius: '10px',
          background: `${accent}1a`,
          color: accent,
        }}>
          {msg.sender}
        </span>
        <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>
          {relativeTime(msg.timestamp, lang)}
        </span>
      </div>
      <div style={{
        fontSize: '13px',
        lineHeight: 1.6,
        color: 'var(--color-text-primary)',
        paddingLeft: '2px',
      }}>
        <MarkdownRenderer>{msg.content}</MarkdownRenderer>
      </div>
    </div>
  );
};

interface StatusMessageProps {
  msg: ChannelMessage;
}

const StatusMessage: React.FC<StatusMessageProps> = ({ msg }) => {
  const { lang } = useLang();
  const label = statusDisplayLabel(msg.status_val ?? msg.content, lang);
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: '6px',
      fontSize: '12px', color: 'var(--color-text-tertiary)',
      fontStyle: 'italic',
      padding: '2px 0',
    }}>
      <span style={{
        width: '5px', height: '5px', borderRadius: '50%',
        background: 'var(--color-text-tertiary)',
        flexShrink: 0,
        opacity: 0.6,
      }} />
      {label}
    </div>
  );
};

interface ArtifactMessageProps {
  msg: ChannelMessage;
}

const ArtifactMessage: React.FC<ArtifactMessageProps & { customPupColors?: Record<string, string> }> = ({ msg, customPupColors = {} }) => {
  const { lang } = useLang();
  const accent = pupAccent(msg.sender, customPupColors);
  const lines = msg.content.split('\n').slice(0, 2);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
        <span style={{
          fontSize: '11px', fontWeight: 500,
          padding: '1px 7px', borderRadius: '10px',
          background: `${accent}1a`,
          color: accent,
        }}>
          {msg.sender}
        </span>
        <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>
          {relativeTime(msg.timestamp, lang)}
        </span>
      </div>
      <div style={{
        background: 'var(--color-background-secondary)',
        border: '0.5px solid var(--color-border-secondary)',
        borderLeft: `2px solid ${accent}`,
        borderRadius: '6px',
        padding: '8px 10px',
        fontFamily: 'var(--font-mono, monospace)',
        fontSize: '12px',
        color: 'var(--color-text-secondary)',
        lineHeight: 1.55,
      }}>
        {msg.artifact_name && (
          <div style={{ fontWeight: 500, color: 'var(--color-text-primary)', marginBottom: '4px', fontSize: '12px' }}>
            {msg.artifact_name}
          </div>
        )}
        {lines.map((line, i) => (
          <div key={i} style={{ whiteSpace: 'pre' }}>{line}</div>
        ))}
        {msg.content.split('\n').length > 2 && (
          <div style={{ color: 'var(--color-text-tertiary)', marginTop: '2px' }}>…</div>
        )}
      </div>
    </div>
  );
};

// ─── Main component ───────────────────────────────────────────────────────────

interface PackChannelProps {
  channels?: Channel[];
  activeChannelId?: string | null;
  messages?: ChannelMessage[];
  error?: string | null;
  loading?: boolean;
  plan?: DelegationPlan | null;
  isCompleted?: boolean;
  detailMode?: boolean;
  onSelectChannel?: (id: string) => void;
  onBackToList?: () => void;
  onOpenFinalReply?: () => void;
  onClearCompleted?: () => void;
  onClearStale?: () => void;
  customPupColors?: Record<string, string>;
}

export const PackChannel: React.FC<PackChannelProps> = ({
  channels = [],
  activeChannelId = null,
  messages = [],
  error = null,
  loading = false,
  plan = null,
  isCompleted = false,
  detailMode = false,
  onSelectChannel,
  onBackToList,
  onOpenFinalReply,
  onClearCompleted,
  onClearStale,
  customPupColors = {},
}) => {
  const messagesEndRef = React.useRef<HTMLDivElement>(null);
  const timelineRef = React.useRef<HTMLDivElement>(null);
  const [autoFollow, setAutoFollow] = React.useState(true);
  const { lang } = useLang();

  const activeChannel = channels.find((c) => c.id === activeChannelId) ?? null;
  const activePlan = plan && plan.channel_id === activeChannelId ? plan : null;
  const activeMessages = messages
    .filter((msg) => msg.channel_id === activeChannelId)
    .filter((msg) => {
      if (msg.msg_type === 'artifact') return true;
      return messageLabel(msg).length > 0;
    });
  const artifactMessages = activeMessages.filter((msg) => msg.msg_type === 'artifact');
  const statusMessages = activeMessages.filter((msg) => msg.msg_type === 'status');
  const sortedChannels = [...channels].sort((a, b) => {
    if (a.status === 'active' && b.status !== 'active') return -1;
    if (a.status !== 'active' && b.status === 'active') return 1;
    return b.updated_at - a.updated_at;
  });
  const activeChannels = sortedChannels.filter((channel) => channel.status === 'active');
  const completedToday = sortedChannels.filter((channel) => {
    if (!channel.completed_at) return false;
    const today = new Date();
    const d = new Date((channel.completed_at ?? 0) < 1_000_000_000_000 ? (channel.completed_at ?? 0) * 1000 : (channel.completed_at ?? 0));
    return d.toDateString() === today.toDateString();
  }).length;
  const historyChannels = sortedChannels.filter((channel) => channel.status !== 'active');
  const memberStates = (activeChannel?.members ?? []).map((member) => ({
    member,
    ...inferMemberState(member, statusMessages, isCompleted, lang),
  }));
  const planStates: PlanStepView[] = (activePlan?.subtasks ?? []).map((subtask, index) => {
    const related = latestStatusForSender(subtask.pup, statusMessages);
    const rawStatus = (related?.status_val ?? '').toLowerCase();
    const latestText = related ? statusDisplayLabel(related.status_val ?? related.content, lang) : '';
    const state: PlanStepState = rawStatus === 'done' || rawStatus === 'completed'
      ? 'done'
      : rawStatus === 'failed' || rawStatus === 'blocked'
        ? 'failed'
        : rawStatus === 'started'
          ? 'running'
          : 'waiting';
    return { id: `${subtask.pup}-${index}`, ...subtask, latestText, state };
  });
  const totalSubtasks = planStates.length;
  const completedSubtasks = planStates.filter((subtask) => subtask.state === 'done').length;
  const runningSubtasks = planStates.filter((subtask) => subtask.state === 'running').length;
  const failedSubtasks = planStates.filter((subtask) => subtask.state === 'failed').length;
  const progressRatio = totalSubtasks > 0 ? Math.min(1, completedSubtasks / totalSubtasks) : 0;
  const currentStage = extractCurrentStage(statusMessages, isCompleted, lang);
  const completedHeaderMode = isCompleted || activeChannel?.status === 'completed';
  const detailStats = [
    { label: t('pack_detail_status', lang), value: activeChannel?.status === 'active' ? t('pack_badge_running', lang) : t('pack_selected_completed', lang) },
    { label: t('pack_detail_progress', lang), value: totalSubtasks > 0 ? `${completedSubtasks}/${totalSubtasks}` : '—', meter: progressRatio },
    { label: t('pack_detail_members', lang), value: `${activeChannel?.members?.length ?? 0}` },
    { label: t('pack_detail_stage', lang), value: currentStage },
  ];
  const completedDetailStats = [
    { label: t('pack_detail_progress', lang), value: totalSubtasks > 0 ? `${completedSubtasks}/${totalSubtasks}` : '—', meter: progressRatio },
    { label: t('pack_detail_members', lang), value: `${activeChannel?.members?.length ?? 0}` },
  ];
  const selectedSummary = activeChannel
    ? activeChannel.status === 'active'
      ? currentStage
      : t('pack_selected_completed', lang)
    : t('pack_selected_none', lang);
  const dagLayout = React.useMemo(() => layoutPlanDag(planStates, lang), [lang, planStates]);

  // Scroll to bottom when messages change
  React.useEffect(() => {
    if (!autoFollow) return;
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeMessages, autoFollow]);

  const handleTimelineScroll = React.useCallback(() => {
    if (!timelineRef.current) return;
    setAutoFollow(isNearBottom(timelineRef.current));
  }, []);

  const StatusBadge: React.FC<{ status: Channel['status'] }> = ({ status }) => (
    <span style={{
      fontSize: '11px', fontWeight: 600,
      padding: '3px 9px', borderRadius: '999px',
      background: status === 'active' ? 'rgba(29,158,117,0.12)' : 'var(--color-background-secondary)',
      color: status === 'active' ? '#1D9E75' : 'var(--color-text-tertiary)',
      border: status === 'active' ? '0.5px solid rgba(29,158,117,0.3)' : '0.5px solid var(--color-border-tertiary)',
      letterSpacing: '0.04em',
      textTransform: 'uppercase',
    }}>
      {status === 'active' ? t('pack_badge_running', lang) : t('pack_badge_complete', lang)}
    </span>
  );

  const PupDots: React.FC<{ pupKeys: string[]; count?: number }> = ({ pupKeys, count = pupKeys.length }) => (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px', minWidth: 0 }}>
      <div style={{ display: 'flex', gap: '5px', flexShrink: 0 }}>
        {pupKeys.slice(0, 6).map((key) => (
          <span
            key={key}
            style={{
              width: '9px',
              height: '9px',
              borderRadius: '50%',
              background: pupAccent(key, customPupColors),
              boxShadow: `0 0 0 2px color-mix(in srgb, ${pupAccent(key, customPupColors)} 12%, transparent)`,
            }}
          />
        ))}
      </div>
      <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
        {count} {t('pack_pups_suffix', lang)}
      </span>
    </div>
  );

  const EventCard: React.FC<{ msg: ChannelMessage }> = ({ msg }) => {
    const accent = pupAccent(msg.sender, customPupColors);
    const isStatus = msg.msg_type === 'status';
    const isArtifact = msg.msg_type === 'artifact';
    return (
      <div style={{
        display: 'grid',
        gridTemplateColumns: '116px minmax(0, 1fr)',
        gap: '14px',
        alignItems: 'start',
      }}>
        <div style={{ paddingTop: '2px' }}>
          <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginBottom: '6px' }}>
            {relativeTime(msg.timestamp, lang)}
          </div>
          <div style={{ display: 'inline-flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <span style={{
              width: '8px',
              height: '8px',
              borderRadius: '50%',
              background: accent,
              boxShadow: `0 0 0 4px color-mix(in srgb, ${accent} 16%, transparent)`,
            }} />
            <span style={{ fontSize: '11px', color: 'var(--color-text-secondary)', fontWeight: 600 }}>
              {msg.sender}
            </span>
          </div>
        </div>
        <div style={{
          position: 'relative',
          borderRadius: isStatus ? '18px' : '22px',
          padding: isArtifact ? '16px' : isStatus ? '14px 16px' : '16px 18px',
          border: '0.5px solid var(--color-border-tertiary)',
          background: isStatus
            ? 'color-mix(in srgb, var(--color-background-primary) 68%, var(--color-background-secondary) 32%)'
            : isArtifact
              ? 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 92%, rgba(186,117,23,0.05) 8%), var(--color-background-primary))'
              : 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 94%, rgba(29,158,117,0.04) 6%), var(--color-background-primary))',
          overflow: 'hidden',
        }}>
          <div style={{
            position: 'absolute',
            inset: '0 auto 0 0',
            width: '3px',
            background: isStatus ? 'var(--color-border-secondary)' : accent,
            opacity: isStatus ? 0.55 : 0.95,
          }} />
          {isStatus ? <StatusMessage msg={msg} /> : null}
          {isArtifact ? <ArtifactMessage msg={msg} customPupColors={customPupColors} /> : null}
          {!isStatus && !isArtifact ? <TextMessage msg={msg} customPupColors={customPupColors} /> : null}
        </div>
      </div>
    );
  };

  if (channels.length === 0) {
    return (
      <div style={{
        height: '100%',
        padding: '32px',
        background: `
          radial-gradient(circle at top left, color-mix(in srgb, #1D9E75 10%, transparent) 0%, transparent 32%),
          linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 90%, var(--color-background-secondary) 10%), var(--color-background-primary))
        `,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}>
        <div style={{
          width: 'min(720px, 100%)',
          borderRadius: '32px',
          padding: '40px 38px',
          border: '0.5px solid var(--color-border-tertiary)',
          background: 'color-mix(in srgb, var(--color-background-primary) 90%, rgba(255,255,255,0.06) 10%)',
          boxShadow: '0 24px 80px rgba(0,0,0,0.06)',
          textAlign: 'center',
        }}>
          <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.12em', marginBottom: '12px' }}>
            {t('pack_channels_title', lang)}
          </div>
          <div style={{ fontSize: '34px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.05 }}>
            {loading ? t('pack_channels_loading', lang)
              : error ? t('pack_channels_error', lang)
                : t('pack_channels_empty', lang)}
          </div>
          <div style={{ fontSize: '14px', color: 'var(--color-text-secondary)', lineHeight: 1.8, marginTop: '16px', maxWidth: '520px', marginInline: 'auto' }}>
            {error ? error : t('pack_channels_empty_desc', lang)}
          </div>
        </div>
      </div>
    );
  }

  if (!detailMode) {
    return (
      <div style={{
        height: '100%',
        overflowY: 'auto',
        padding: '24px 24px 34px',
        background: `
          radial-gradient(circle at top left, color-mix(in srgb, #1D9E75 12%, transparent) 0%, transparent 24%),
          radial-gradient(circle at 85% 8%, color-mix(in srgb, #BA7517 8%, transparent) 0%, transparent 18%),
          linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 95%, var(--color-background-secondary) 5%), var(--color-background-primary))
        `,
      }}>
        <div style={{ width: 'min(1100px, 100%)', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: '16px' }}>
          <div style={{
            borderRadius: '24px',
            padding: '16px 18px',
            border: '0.5px solid var(--color-border-tertiary)',
            background: 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 92%, rgba(29,158,117,0.05) 8%), var(--color-background-primary))',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '16px',
            flexWrap: 'wrap',
          }}>
            <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', gap: '6px' }}>
              <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.12em' }}>
                {t('pack_mission_control', lang)}
              </div>
              <div style={{ display: 'flex', alignItems: 'baseline', gap: '10px', flexWrap: 'wrap' }}>
                <div style={{ fontSize: '24px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.05, letterSpacing: '-0.02em' }}>
                  {t('pack_channels_list', lang)}
                </div>
                <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)' }}>
                  {t('pack_focus', lang)}: {selectedSummary}
                </div>
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'stretch', gap: '10px', flexWrap: 'wrap' }}>
              {[
                { label: t('pack_stat_total', lang), value: String(channels.length), tone: 'var(--color-text-primary)' },
                { label: t('pack_stat_active', lang), value: String(activeChannels.length), tone: '#1D9E75' },
                { label: t('pack_stat_done_today', lang), value: String(completedToday), tone: '#BA7517' },
              ].map((item) => (
                <div
                  key={item.label}
                  style={{
                    minWidth: '92px',
                    borderRadius: '16px',
                    padding: '10px 12px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: 'color-mix(in srgb, var(--color-background-primary) 96%, rgba(255,255,255,0.03) 4%)',
                  }}
                >
                  <div style={{ fontSize: '18px', fontWeight: 700, color: item.tone, lineHeight: 1.1 }}>
                    {item.value}
                  </div>
                  <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginTop: '5px' }}>
                    {item.label}
                  </div>
                </div>
              ))}
            </div>
          </div>

          {activeChannels.length > 0 && (
            <div style={{
              borderRadius: '24px',
              padding: '14px 18px',
              border: '0.5px solid color-mix(in srgb, #1D9E75 22%, var(--color-border-tertiary) 78%)',
              background: 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 84%, rgba(29,158,117,0.10) 16%), var(--color-background-primary))',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: '18px',
            }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.12em', color: '#1D9E75', marginBottom: '8px' }}>
                  {t('pack_active_mission', lang)}
                </div>
                <div style={{ fontSize: '18px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.1 }}>
                  {activeChannels[0].title}
                </div>
                <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)', marginTop: '8px' }}>
                  {activeChannels.length} {t('pack_active_channels_desc_suffix', lang)}
                </div>
              </div>
              <button
                onClick={() => onSelectChannel?.(activeChannels[0].id)}
                style={{
                  padding: '10px 16px',
                  borderRadius: '999px',
                  border: 'none',
                  background: '#1D9E75',
                  color: '#fff',
                  cursor: 'pointer',
                  fontSize: '12px',
                  fontWeight: 700,
                  flexShrink: 0,
                }}
              >
                {t('pack_continue', lang)}
              </button>
            </div>
          )}

          {activeChannels.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px' }}>
                <div style={{ fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'var(--color-text-tertiary)' }}>
                  {t('pack_running_section', lang)}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                  <div style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
                    {activeChannels.length} {t('pack_channel_count_suffix', lang)}
                  </div>
                  {onClearStale && (
                    <button
                      onClick={(event) => {
                        event.stopPropagation();
                        onClearStale();
                      }}
                      style={{
                        padding: '6px 10px',
                        borderRadius: '999px',
                        border: '0.5px solid var(--color-border-tertiary)',
                        background: 'var(--color-background-primary)',
                        color: 'var(--color-text-secondary)',
                        cursor: 'pointer',
                        fontSize: '12px',
                        fontWeight: 600,
                      }}
                    >
                      {t('pack_clear_stuck', lang)}
                    </button>
                  )}
                </div>
              </div>
              {activeChannels.map((channel) => {
                const isRunning = channel.status === 'active';
                return (
                  <button
                    key={channel.id}
                    onClick={() => onSelectChannel?.(channel.id)}
                    style={{
                      textAlign: 'left',
                      width: '100%',
                      borderRadius: '18px',
                      padding: '16px 18px',
                      border: '0.5px solid var(--color-border-tertiary)',
                      background: 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 78%, rgba(29,158,117,0.10) 22%), var(--color-background-primary))',
                      cursor: 'pointer',
                      boxShadow: '0 10px 28px rgba(29,158,117,0.08)',
                    }}
                  >
                    <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1.6fr) minmax(140px, 0.55fr) minmax(200px, 0.8fr) minmax(140px, 0.55fr)', gap: '16px', alignItems: 'center' }}>
                      <div style={{ minWidth: 0 }}>
                        <div style={{ fontSize: '15px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.25 }}>
                          {channel.title}
                        </div>
                        <div style={{
                          fontSize: '12px',
                          color: 'var(--color-text-secondary)',
                          lineHeight: 1.55,
                          marginTop: '6px',
                          display: '-webkit-box',
                          WebkitLineClamp: 2,
                          WebkitBoxOrient: 'vertical',
                          overflow: 'hidden',
                        }}>
                          {channel.task_id}
                        </div>
                      </div>
                      <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                        <span style={{
                          fontSize: '10px',
                          fontWeight: 700,
                          letterSpacing: '0.08em',
                          textTransform: 'uppercase',
                          color: isRunning ? '#1D9E75' : 'var(--color-text-tertiary)',
                          flexShrink: 0,
                          padding: '4px 8px',
                          borderRadius: '999px',
                          background: isRunning ? 'rgba(29,158,117,0.10)' : 'var(--color-background-secondary)',
                        }}>
                        {t('pack_badge_running', lang)}
                        </span>
                      </div>
                      <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                        <PupDots pupKeys={channel.members ?? []} />
                      </div>
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: '10px' }}>
                        <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>
                          {relativeTime(channel.updated_at, lang)}
                        </span>
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          )}

          <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px' }}>
              <div style={{ fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'var(--color-text-tertiary)' }}>
                  {t('pack_history', lang)}
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                <div style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
                    {historyChannels.length} {t('pack_channel_count_suffix', lang)}
                </div>
                {historyChannels.length > 0 && onClearCompleted && (
                  <button
                    onClick={(event) => {
                      event.stopPropagation();
                      onClearCompleted();
                    }}
                    style={{
                      padding: '6px 10px',
                      borderRadius: '999px',
                      border: '0.5px solid var(--color-border-tertiary)',
                      background: 'var(--color-background-primary)',
                      color: 'var(--color-text-secondary)',
                      cursor: 'pointer',
                      fontSize: '12px',
                      fontWeight: 600,
                    }}
                  >
                    {t('pack_clear_completed', lang)}
                  </button>
                )}
              </div>
            </div>
            {historyChannels.map((channel) => {
              const isRunning = channel.status === 'active';
              return (
                <button
                  key={channel.id}
                  onClick={() => onSelectChannel?.(channel.id)}
                  style={{
                    textAlign: 'left',
                    width: '100%',
                    borderRadius: '18px',
                    padding: '16px 18px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: isRunning
                      ? 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 78%, rgba(29,158,117,0.10) 22%), var(--color-background-primary))'
                      : 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 94%, rgba(255,255,255,0.02) 6%), var(--color-background-primary))',
                    cursor: 'pointer',
                    boxShadow: isRunning ? '0 10px 28px rgba(29,158,117,0.08)' : 'none',
                  }}
                >
                  <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1.6fr) minmax(140px, 0.55fr) minmax(200px, 0.8fr) minmax(140px, 0.55fr)', gap: '16px', alignItems: 'center' }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: '15px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.25 }}>
                        {channel.title}
                      </div>
                      <div style={{
                        fontSize: '12px',
                        color: 'var(--color-text-secondary)',
                        lineHeight: 1.55,
                        marginTop: '6px',
                        display: '-webkit-box',
                        WebkitLineClamp: 2,
                        WebkitBoxOrient: 'vertical',
                        overflow: 'hidden',
                      }}>
                        {channel.task_id}
                      </div>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                      <span style={{
                        fontSize: '10px',
                        fontWeight: 700,
                        letterSpacing: '0.08em',
                        textTransform: 'uppercase',
                        color: isRunning ? '#1D9E75' : 'var(--color-text-tertiary)',
                        flexShrink: 0,
                        padding: '4px 8px',
                        borderRadius: '999px',
                        background: isRunning ? 'rgba(29,158,117,0.10)' : 'var(--color-background-secondary)',
                      }}>
                        {isRunning ? t('pack_badge_running', lang) : t('pack_status_done', lang)}
                      </span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                      <PupDots pupKeys={channel.members ?? []} />
                    </div>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: '10px' }}>
                      <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>
                        {relativeTime(channel.updated_at, lang)}
                      </span>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', height: '100%', minWidth: 0 }}>
      <div style={{
        flex: 1,
        minWidth: 0,
        display: 'flex',
        flexDirection: 'column',
        background: `
          radial-gradient(circle at top left, color-mix(in srgb, #1D9E75 9%, transparent) 0%, transparent 24%),
          linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 94%, var(--color-background-secondary) 6%), var(--color-background-primary))
        `,
      }}>
        {activeChannel && (
          <>
            <div style={{
              padding: '10px 20px 10px',
              borderBottom: '0.5px solid var(--color-border-tertiary)',
              background: 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 86%, rgba(29,158,117,0.06) 14%), color-mix(in srgb, var(--color-background-primary) 96%, transparent 4%))',
              flexShrink: 0,
            }}>
              {onBackToList && (
                <button
                  onClick={onBackToList}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: '6px',
                    padding: 0,
                    border: 'none',
                    background: 'none',
                    color: 'var(--color-text-secondary)',
                    cursor: 'pointer',
                    fontSize: '12px',
                    fontWeight: 650,
                    marginBottom: '8px',
                  }}
                >
                  <span style={{ fontSize: '14px', lineHeight: 1 }}>←</span>
                  {t('pack_back_to_channels', lang)}
                </button>
              )}
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: completedHeaderMode ? '12px' : '16px', alignItems: 'flex-start' }}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '7px', marginBottom: completedHeaderMode ? '3px' : '5px' }}>
                    <div style={{ fontSize: completedHeaderMode ? '18px' : '21px', fontWeight: 650, color: 'var(--color-text-primary)', lineHeight: 1.04, letterSpacing: '-0.02em' }}>
                      {activeChannel.title}
                    </div>
                    <StatusBadge status={activeChannel.status} />
                    {!completedHeaderMode && (
                      <span style={{
                        fontSize: '10px',
                        color: 'var(--color-text-tertiary)',
                        textTransform: 'uppercase',
                        letterSpacing: '0.08em',
                        padding: '3px 7px',
                        borderRadius: '999px',
                        background: 'var(--color-background-secondary)',
                      }}>
                        {currentStage}
                      </span>
                    )}
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: completedHeaderMode ? '8px' : '10px', minWidth: 0 }}>
                    <PupDots pupKeys={activeChannel.members ?? []} count={(activeChannel.members ?? []).length} />
                    <span style={{
                      fontSize: completedHeaderMode ? '11px' : '12px',
                      color: completedHeaderMode ? 'var(--color-text-tertiary)' : 'var(--color-text-secondary)',
                      minWidth: 0,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      flex: '1 1 280px',
                    }}>
                      {activeChannel.task_id}
                    </span>
                    {!completedHeaderMode && (
                      <>
                        <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>{t('task_created_at', lang)} {absoluteTime(activeChannel.created_at, lang)}</span>
                        <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>{t('pack_updated', lang)} {absoluteTime(activeChannel.updated_at, lang)}</span>
                      </>
                    )}
                    {completedHeaderMode && (
                      <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>
                        {t('pack_completed_at_prefix', lang)} {absoluteTime(activeChannel.updated_at, lang)}
                      </span>
                    )}
                  </div>
                </div>
                <div style={{ display: 'flex', flexDirection: completedHeaderMode ? 'row' : 'column', alignItems: 'flex-end', gap: '8px', flexShrink: 0, width: completedHeaderMode ? 'auto' : 'min(340px, 34vw)' }}>
                  <div style={{
                    display: 'flex',
                    justifyContent: 'flex-end',
                    flexWrap: 'wrap',
                    gap: '8px',
                    width: completedHeaderMode ? 'auto' : '100%',
                  }}>
                    {(completedHeaderMode ? completedDetailStats : detailStats).map((item) => (
                      <div
                        key={item.label}
                        style={{
                          borderRadius: '999px',
                          border: '0.5px solid var(--color-border-tertiary)',
                          background: 'color-mix(in srgb, var(--color-background-primary) 96%, rgba(255,255,255,0.04) 4%)',
                          padding: item.label === t('pack_detail_progress', lang) ? '7px 10px' : '7px 10px 7px 9px',
                          minWidth: 0,
                          display: 'inline-flex',
                          alignItems: 'center',
                          gap: '8px',
                          maxWidth: item.label === t('pack_detail_stage', lang) ? '100%' : 'none',
                        }}
                      >
                        <div style={{ fontSize: '10px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.08em', flexShrink: 0 }}>
                          {item.label}
                        </div>
                        <div style={{
                          fontSize: item.label === t('pack_detail_stage', lang) ? '11px' : '13px',
                          fontWeight: 650,
                          color: 'var(--color-text-primary)',
                          whiteSpace: item.label === t('pack_detail_stage', lang) ? 'nowrap' : 'normal',
                          overflow: item.label === t('pack_detail_stage', lang) ? 'hidden' : 'visible',
                          textOverflow: item.label === t('pack_detail_stage', lang) ? 'ellipsis' : 'clip',
                          minWidth: 0,
                        }}>
                          {item.value}
                        </div>
                        {'meter' in item && (
                          <div style={{ width: completedHeaderMode ? '42px' : '54px', height: '4px', borderRadius: '999px', background: 'var(--color-background-secondary)', overflow: 'hidden', flexShrink: 0 }}>
                            <div style={{ width: `${Math.max(8, Math.round((item.meter ?? 0) * 100))}%`, height: '100%', background: '#1D9E75', borderRadius: '999px' }} />
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                  {isCompleted && onOpenFinalReply && (
                    <button
                      onClick={onOpenFinalReply}
                      style={{
                        padding: completedHeaderMode ? '7px 11px' : '9px 14px',
                        borderRadius: '999px',
                        border: 'none',
                        background: '#BA7517',
                        color: '#fff',
                        cursor: 'pointer',
                        fontSize: '12px',
                        fontWeight: 600,
                        alignSelf: 'flex-end',
                      }}
                    >
                      {t('pack_final_reply', lang)}
                    </button>
                  )}
                </div>
              </div>
            </div>

            <div
              ref={timelineRef}
              onScroll={handleTimelineScroll}
              style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: '16px 20px 20px' }}
            >
              <div style={{ maxWidth: '960px', display: 'flex', flexDirection: 'column', gap: '14px' }}>
                {planStates.length > 0 && (
                  <div style={{
                    borderRadius: '20px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: 'color-mix(in srgb, var(--color-background-primary) 94%, rgba(29,158,117,0.03) 6%)',
                    padding: '16px 16px 14px',
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px', marginBottom: '12px', flexWrap: 'wrap' }}>
                      <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                        {t('pack_dag_title', lang)}
                      </div>
                      <div style={{ display: 'flex', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
                        {[
                          { label: t('pack_dag_running', lang), value: runningSubtasks, color: '#1D9E75' },
                          { label: t('pack_dag_pending', lang), value: Math.max(0, totalSubtasks - completedSubtasks - runningSubtasks - failedSubtasks), color: 'var(--color-text-tertiary)' },
                          { label: t('pack_dag_done', lang), value: completedSubtasks, color: '#BA7517' },
                          { label: t('pack_dag_issues', lang), value: failedSubtasks, color: '#C65A5A' },
                        ].map((item) => (
                          <span
                            key={item.label}
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                              gap: '6px',
                              fontSize: '11px',
                              color: 'var(--color-text-secondary)',
                              padding: '5px 9px',
                              borderRadius: '999px',
                              background: 'var(--color-background-primary)',
                              border: '0.5px solid var(--color-border-tertiary)',
                            }}
                          >
                            <span style={{ width: '7px', height: '7px', borderRadius: '50%', background: item.color, flexShrink: 0 }} />
                            {item.label} {item.value}
                          </span>
                        ))}
                      </div>
                    </div>
                    <div style={{ display: 'flex', gap: '10px', marginBottom: '12px', flexWrap: 'wrap' }}>
                      {dagLayout.layers.map((layer) => (
                        <div
                          key={layer.depth}
                          style={{
                            minWidth: '120px',
                            borderRadius: '14px',
                            padding: '8px 10px',
                            border: '0.5px solid var(--color-border-tertiary)',
                            background: 'color-mix(in srgb, var(--color-background-primary) 97%, rgba(255,255,255,0.03) 3%)',
                          }}
                        >
                          <div style={{ fontSize: '11px', color: 'var(--color-text-primary)', fontWeight: 650 }}>
                            {layer.title}
                          </div>
                          <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginTop: '4px' }}>
                            {layer.count} {t('pack_nodes_suffix', lang)}
                          </div>
                        </div>
                      ))}
                    </div>
                    <div style={{ overflowX: 'auto', paddingBottom: '4px' }}>
                      <div
                        style={{
                          position: 'relative',
                          width: `${dagLayout.width}px`,
                          height: `${dagLayout.height}px`,
                          minWidth: '100%',
                          borderRadius: '18px',
                          border: '0.5px solid var(--color-border-tertiary)',
                          background: 'linear-gradient(180deg, color-mix(in srgb, var(--color-background-primary) 97%, rgba(255,255,255,0.03) 3%), var(--color-background-primary))',
                          overflow: 'hidden',
                        }}
                      >
                        <svg
                          width={dagLayout.width}
                          height={dagLayout.height}
                          viewBox={`0 0 ${dagLayout.width} ${dagLayout.height}`}
                          style={{ position: 'absolute', inset: 0 }}
                        >
                          <defs>
                            <marker
                              id="pack-channel-dag-arrow"
                              markerWidth="8"
                              markerHeight="8"
                              refX="6"
                              refY="4"
                              orient="auto"
                              markerUnits="strokeWidth"
                            >
                              <path d="M 0 0 L 8 4 L 0 8 z" fill="rgba(128,128,128,0.72)" />
                            </marker>
                          </defs>
                          {dagLayout.edges.map((edge) => {
                            const controlOffset = Math.max(18, (edge.toX - edge.fromX) / 2);
                            const path = `M ${edge.fromX} ${edge.fromY} C ${edge.fromX + controlOffset} ${edge.fromY}, ${edge.toX - controlOffset} ${edge.toY}, ${edge.toX} ${edge.toY}`;
                            const edgeColor = edge.colorState === 'failed'
                              ? '#C65A5A'
                              : edge.colorState === 'done'
                                ? '#BA7517'
                                : edge.colorState === 'running'
                                  ? '#1D9E75'
                                  : 'rgba(128,128,128,0.55)';
                            return (
                              <path
                                key={edge.id}
                                d={path}
                                fill="none"
                                stroke={edgeColor}
                                strokeWidth="2"
                                strokeOpacity="0.42"
                                strokeLinecap="round"
                                markerEnd="url(#pack-channel-dag-arrow)"
                              />
                            );
                          })}
                        </svg>
                        {dagLayout.merges.map((merge) => (
                          <div
                            key={merge.id}
                            style={{
                              position: 'absolute',
                              left: `${merge.x - 10}px`,
                              top: `${merge.y - 10}px`,
                              width: '20px',
                              height: '20px',
                              borderRadius: '999px',
                              border: '0.5px solid var(--color-border-tertiary)',
                              background: 'color-mix(in srgb, var(--color-background-primary) 94%, rgba(29,158,117,0.08) 6%)',
                              color: 'var(--color-text-secondary)',
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'center',
                              fontSize: '10px',
                              fontWeight: 700,
                              boxShadow: '0 6px 12px rgba(0,0,0,0.05)',
                            }}
                            title={`${t('pack_merge_title_prefix', lang)} ${merge.dependsOn.join(' + ')} -> ${merge.targetId}`}
                          >
                            {t('pack_merge_badge', lang)}
                          </div>
                        ))}
                        {dagLayout.nodes.map((node) => {
                          const tone = node.state === 'done'
                            ? { color: '#BA7517', background: 'rgba(186,117,23,0.12)' }
                            : node.state === 'failed'
                              ? { color: '#C65A5A', background: 'rgba(198,90,90,0.12)' }
                              : node.state === 'running'
                                ? { color: '#1D9E75', background: 'rgba(29,158,117,0.12)' }
                                : { color: 'var(--color-text-tertiary)', background: 'var(--color-background-secondary)' };
                          return (
                            <div
                              key={node.id}
                              style={{
                                position: 'absolute',
                                left: `${node.x}px`,
                                top: `${node.y}px`,
                                width: `${dagLayout.nodeWidth}px`,
                                minHeight: `${dagLayout.nodeHeight}px`,
                                borderRadius: '14px',
                                border: `0.5px solid ${node.state === 'running' ? 'rgba(29,158,117,0.28)' : 'var(--color-border-tertiary)'}`,
                                background: 'color-mix(in srgb, var(--color-background-primary) 96%, rgba(255,255,255,0.03) 4%)',
                                boxShadow: node.state === 'running' ? '0 10px 18px rgba(29,158,117,0.08)' : 'none',
                                padding: '10px 10px 9px',
                                display: 'flex',
                                flexDirection: 'column',
                                justifyContent: 'space-between',
                                gap: '6px',
                              }}
                            >
                              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                                <span style={{ fontSize: '12px', fontWeight: 700, color: pupAccent(node.pup, customPupColors) }}>
                                  {node.pup}
                                </span>
                                <span style={{
                                  fontSize: '10px',
                                  fontWeight: 700,
                                  padding: '3px 7px',
                                  borderRadius: '999px',
                                  color: tone.color,
                                  background: tone.background,
                                  textTransform: 'uppercase',
                                  letterSpacing: '0.06em',
                                }}>
                                  {node.state}
                                </span>
                              </div>
                              <div style={{
                                fontSize: '11px',
                                color: 'var(--color-text-secondary)',
                                lineHeight: 1.4,
                                display: '-webkit-box',
                                WebkitLineClamp: 2,
                                WebkitBoxOrient: 'vertical',
                                overflow: 'hidden',
                              }}>
                                {node.description}
                              </div>
                              <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px' }}>
                                  {node.depends_on.length > 0 ? node.depends_on.map((dep) => (
                                    <span
                                      key={`${node.id}-${dep}`}
                                      style={{
                                        display: 'inline-flex',
                                        alignItems: 'center',
                                        gap: '4px',
                                        padding: '2px 6px',
                                        borderRadius: '999px',
                                        fontSize: '10px',
                                        color: 'var(--color-text-secondary)',
                                        background: 'var(--color-background-secondary)',
                                        border: '0.5px solid var(--color-border-tertiary)',
                                      }}
                                    >
                                      <span style={{ color: 'var(--color-text-tertiary)' }}>←</span>
                                      {dep}
                                    </span>
                                  )) : (
                                    <span style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>
                                      {t('ctx_no_upstream', lang)}
                                    </span>
                                  )}
                                </div>
                                {node.latestText && (
                                  <div style={{
                                    fontSize: '10px',
                                    color: 'var(--color-text-tertiary)',
                                    lineHeight: 1.35,
                                    display: '-webkit-box',
                                    WebkitLineClamp: 2,
                                    WebkitBoxOrient: 'vertical',
                                    overflow: 'hidden',
                                  }}>
                                    {node.latestText}
                                  </div>
                                )}
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  </div>
                )}
                {activeMessages.length === 0 ? (
                  <div style={{
                    borderRadius: '20px',
                    border: '0.5px dashed var(--color-border-secondary)',
                    padding: '28px 24px',
                    color: 'var(--color-text-tertiary)',
                    fontSize: '13px',
                    textAlign: 'center',
                    background: 'color-mix(in srgb, var(--color-background-primary) 76%, var(--color-background-secondary) 24%)',
                  }}>
                    {error ? `${t('pack_messages_load_failed_prefix', lang)}${error}` : t('pack_stage_waiting', lang)}
                  </div>
                ) : (
                  activeMessages.map((msg) => <EventCard key={msg.id} msg={msg} />)
                )}
                <div ref={messagesEndRef} />
              </div>
              {!autoFollow && activeMessages.length > 0 && (
                <button
                  onClick={() => {
                    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
                    setAutoFollow(true);
                  }}
                  style={{
                    position: 'sticky',
                    bottom: '14px',
                    marginLeft: 'auto',
                    display: 'block',
                    padding: '8px 12px',
                    borderRadius: '999px',
                    border: '0.5px solid rgba(29,158,117,0.22)',
                    background: 'color-mix(in srgb, var(--color-background-primary) 88%, rgba(29,158,117,0.10) 12%)',
                    color: '#1D9E75',
                    fontSize: '12px',
                    fontWeight: 700,
                    cursor: 'pointer',
                    marginTop: '12px',
                  }}
                >
                  {t('pack_jump_latest', lang)}
                </button>
              )}
            </div>
          </>
        )}
      </div>

      <ContextInspector
        plan={activePlan}
        members={memberStates}
        artifacts={artifactMessages}
        getPupAccent={(name) => pupAccent(name, customPupColors)}
        formatRelativeTime={(timestamp) => relativeTime(timestamp, lang)}
      />
    </div>
  );
};
