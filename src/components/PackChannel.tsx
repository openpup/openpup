import React from 'react';
import { invoke } from '@tauri-apps/api/core';

// ─── Types ────────────────────────────────────────────────────────────────────

interface ChannelMessage {
  id: string;
  sender: string;         // Pup display name, e.g. "Alpha", "Research Pup"
  content: string;
  message_type: 'text' | 'status' | 'artifact';
  artifact_name?: string;
  timestamp: number;      // unix ms
  mention?: string;       // @mention target pup name
}

interface Channel {
  id: string;
  name: string;
  task_description: string;
  status: 'active' | 'completed';
  pup_keys: string[];
  created_at: number;
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

function pupAccent(name: string): string {
  return PUP_ACCENT[name] ?? '#1D9E75';
}

// ─── Relative time ────────────────────────────────────────────────────────────

function relativeTime(ts: number): string {
  const now = Date.now();
  const diffMs = now - ts;
  const diffMin = Math.floor(diffMs / 60_000);
  const diffHour = Math.floor(diffMs / 3_600_000);

  if (diffMin < 1) return '刚刚';
  if (diffMin < 60) return `${diffMin}分钟前`;
  if (diffHour < 24) return `${diffHour}小时前`;

  const d = new Date(ts);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);

  if (d.toDateString() === yesterday.toDateString()) {
    const hh = String(d.getHours()).padStart(2, '0');
    const mm = String(d.getMinutes()).padStart(2, '0');
    return `昨天 ${hh}:${mm}`;
  }

  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${d.getMonth() + 1}/${d.getDate()} ${hh}:${mm}`;
}

// ─── @mention parser ──────────────────────────────────────────────────────────

const MENTION_RE = /@(Alpha|Dev Pup|Writer Pup|Research Pup|Ops Pup|Life Admin Pup)/g;

function renderWithMentions(content: string): React.ReactNode[] {
  const parts: React.ReactNode[] = [];
  let last = 0;
  let match: RegExpExecArray | null;

  MENTION_RE.lastIndex = 0;
  while ((match = MENTION_RE.exec(content)) !== null) {
    if (match.index > last) {
      parts.push(content.slice(last, match.index));
    }
    const pupName = match[1];
    parts.push(
      <span
        key={match.index}
        style={{
          color: pupAccent(pupName),
          fontWeight: 500,
        }}
      >
        {match[0]}
      </span>
    );
    last = match.index + match[0].length;
  }

  if (last < content.length) {
    parts.push(content.slice(last));
  }

  return parts;
}

// ─── Sub-components ───────────────────────────────────────────────────────────

interface TextMessageProps {
  msg: ChannelMessage;
}

const TextMessage: React.FC<TextMessageProps> = ({ msg }) => {
  const accent = pupAccent(msg.sender);
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
          {relativeTime(msg.timestamp)}
        </span>
      </div>
      <div style={{
        fontSize: '13px',
        lineHeight: 1.6,
        color: 'var(--color-text-primary)',
        paddingLeft: '2px',
      }}>
        {renderWithMentions(msg.content)}
      </div>
    </div>
  );
};

interface StatusMessageProps {
  msg: ChannelMessage;
}

const StatusMessage: React.FC<StatusMessageProps> = ({ msg }) => {
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
      {msg.content}
    </div>
  );
};

interface ArtifactMessageProps {
  msg: ChannelMessage;
}

const ArtifactMessage: React.FC<ArtifactMessageProps> = ({ msg }) => {
  const accent = pupAccent(msg.sender);
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
          {relativeTime(msg.timestamp)}
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

// ─── Channel selector ─────────────────────────────────────────────────────────

interface ChannelTabProps {
  channel: Channel;
  active: boolean;
  onClick: () => void;
}

const ChannelTab: React.FC<ChannelTabProps> = ({ channel, active, onClick }) => {
  return (
    <button
      onClick={onClick}
      style={{
        display: 'block', width: '100%', textAlign: 'left',
        padding: '6px 10px', borderRadius: '6px',
        background: active ? 'var(--color-background-secondary)' : 'transparent',
        border: active ? '0.5px solid var(--color-border-secondary)' : '0.5px solid transparent',
        cursor: 'pointer',
      }}
    >
      <div style={{ fontSize: '13px', fontWeight: 500, color: 'var(--color-text-primary)' }}>
        {channel.name}
      </div>
      <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginTop: '1px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {channel.task_description}
      </div>
    </button>
  );
};

// ─── Main component ───────────────────────────────────────────────────────────

export const PackChannel: React.FC = () => {
  const [channels, setChannels] = React.useState<Channel[]>([]);
  const [activeChannelId, setActiveChannelId] = React.useState<string | null>(null);
  const [messages, setMessages] = React.useState<ChannelMessage[]>([]);
  const [loading, setLoading] = React.useState(true);
  const messagesEndRef = React.useRef<HTMLDivElement>(null);
  const pollRef = React.useRef<ReturnType<typeof setInterval> | null>(null);

  const activeChannel = channels.find((c) => c.id === activeChannelId) ?? null;

  // Load channels on mount
  React.useEffect(() => {
    invoke<Channel[]>('list_channels')
      .then((list) => {
        setChannels(list);
        // Select first active channel by default
        const first = list.find((c) => c.status === 'active') ?? list[0] ?? null;
        if (first) setActiveChannelId(first.id);
      })
      .catch(() => {
        setChannels([]);
      })
      .finally(() => setLoading(false));
  }, []);

  // Load messages and start polling when active channel changes
  React.useEffect(() => {
    if (!activeChannelId) {
      setMessages([]);
      return;
    }

    const fetchMessages = () => {
      invoke<ChannelMessage[]>('get_channel_messages', { channelId: activeChannelId })
        .then((msgs) => setMessages(msgs))
        .catch(() => {});
    };

    fetchMessages();

    // Poll every 3 seconds for active channels
    if (pollRef.current) clearInterval(pollRef.current);
    const ch = channels.find((c) => c.id === activeChannelId);
    if (ch?.status === 'active') {
      pollRef.current = setInterval(fetchMessages, 3000);
    }

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [activeChannelId, channels]);

  // Scroll to bottom when messages change
  React.useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // ── Status badge ──────────────────────────────────────────────────────────

  const StatusBadge: React.FC<{ status: Channel['status'] }> = ({ status }) => (
    <span style={{
      fontSize: '11px', fontWeight: 500,
      padding: '1px 7px', borderRadius: '10px',
      background: status === 'active' ? 'rgba(29,158,117,0.12)' : 'var(--color-background-secondary)',
      color: status === 'active' ? '#1D9E75' : 'var(--color-text-tertiary)',
      border: status === 'active' ? '0.5px solid rgba(29,158,117,0.3)' : '0.5px solid var(--color-border-tertiary)',
    }}>
      {status === 'active' ? '进行中' : '已完成'}
    </span>
  );

  // ── Pup dot row ───────────────────────────────────────────────────────────

  const PupDots: React.FC<{ pupKeys: string[]; count: number }> = ({ pupKeys, count }) => (
    <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
      <div style={{ display: 'flex', gap: '3px' }}>
        {pupKeys.slice(0, 5).map((key) => (
          <span
            key={key}
            style={{
              width: '8px', height: '8px', borderRadius: '50%',
              background: pupAccent(key),
              flexShrink: 0,
            }}
          />
        ))}
      </div>
      <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
        {count} pups
      </span>
    </div>
  );

  // ── Render ────────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
        <div style={{
          padding: '12px 16px', borderBottom: '0.5px solid var(--color-border-tertiary)',
          flexShrink: 0,
        }}>
          <div style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>Pack Channel</div>
        </div>
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <span style={{ fontSize: '13px', color: 'var(--color-text-tertiary)' }}>加载中…</span>
        </div>
      </div>
    );
  }

  if (channels.length === 0) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
        <div style={{
          padding: '12px 16px', borderBottom: '0.5px solid var(--color-border-tertiary)',
          flexShrink: 0,
        }}>
          <div style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>Pack Channel</div>
          <div style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', marginTop: '2px' }}>多 Pup 异步协作</div>
        </div>
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: '8px' }}>
          <div style={{ fontSize: '28px', opacity: 0.3 }}>🐾</div>
          <div style={{ fontSize: '14px', color: 'var(--color-text-secondary)' }}>暂无活跃频道</div>
          <div style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', textAlign: 'center', maxWidth: '220px', lineHeight: 1.6 }}>
            当 Alpha 启动多 Pup 任务时，频道会自动创建并显示在这里。
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* Channel header */}
      {activeChannel && (
        <div style={{
          padding: '10px 16px 10px',
          borderBottom: '0.5px solid var(--color-border-tertiary)',
          flexShrink: 0,
        }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: '10px' }}>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '3px' }}>
                <span style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>
                  {activeChannel.name}
                </span>
                <StatusBadge status={activeChannel.status} />
              </div>
              <div style={{
                fontSize: '12px', color: 'var(--color-text-secondary)',
                overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                marginBottom: '5px',
              }}>
                {activeChannel.task_description}
              </div>
              <PupDots pupKeys={activeChannel.pup_keys} count={activeChannel.pup_keys.length} />
            </div>

            {/* Channel list — if multiple channels */}
            {channels.length > 1 && (
              <div style={{ flexShrink: 0, display: 'flex', flexDirection: 'column', gap: '2px', minWidth: '140px' }}>
                {channels.map((ch) => (
                  <ChannelTab
                    key={ch.id}
                    channel={ch}
                    active={ch.id === activeChannelId}
                    onClick={() => setActiveChannelId(ch.id)}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Message list */}
      <div style={{
        flex: 1, overflowY: 'auto',
        padding: '14px 16px',
        display: 'flex', flexDirection: 'column', gap: '10px',
      }}>
        {messages.length === 0 ? (
          <div style={{
            flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'var(--color-text-tertiary)', fontSize: '13px',
          }}>
            等待消息…
          </div>
        ) : (
          messages.map((msg) => {
            if (msg.message_type === 'status') {
              return <StatusMessage key={msg.id} msg={msg} />;
            }
            if (msg.message_type === 'artifact') {
              return <ArtifactMessage key={msg.id} msg={msg} />;
            }
            return <TextMessage key={msg.id} msg={msg} />;
          })
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Footer — read-only */}
      <div style={{
        padding: '8px 16px',
        borderTop: '0.5px solid var(--color-border-tertiary)',
        flexShrink: 0,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}>
        <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
          只读旁观 · 频道由 Alpha 管理
        </span>
      </div>

    </div>
  );
};
