import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { useLang, t } from '../i18n';

export interface MessageActionsProps {
  messageId: string;
  channelId?: string;
  content: string;
  isArtifact?: boolean;
  artifactName?: string;
}

/* Inline SVG icons — 14×14, stroke-based, matching the minimal UI tone */
const IconThumbUp = ({ filled }: { filled?: boolean }) => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill={filled ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M7 22V11l3-7a2 2 0 0 1 2-2h.5a1.5 1.5 0 0 1 1.5 1.5V9h5.17a2 2 0 0 1 1.98 2.27l-1.17 7A2 2 0 0 1 17 20H7z" />
    <path d="M7 11H4a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h3" />
  </svg>
);

const IconThumbDown = ({ filled }: { filled?: boolean }) => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill={filled ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M17 2V13l-3 7a2 2 0 0 1-2 2h-.5a1.5 1.5 0 0 1-1.5-1.5V15H4.83a2 2 0 0 1-1.98-2.27l1.17-7A2 2 0 0 1 7 4h10z" />
    <path d="M17 13h3a1 1 0 0 0 1-1V3a1 1 0 0 0-1-1h-3" />
  </svg>
);

const IconCopy = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="9" y="9" width="13" height="13" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

const IconCheck = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="20 6 9 17 4 12" />
  </svg>
);

const IconDownload = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <polyline points="7 10 12 15 17 10" />
    <line x1="12" y1="15" x2="12" y2="3" />
  </svg>
);

export const MessageActions: React.FC<MessageActionsProps> = ({
  messageId,
  channelId,
  content,
  isArtifact = false,
  artifactName,
}) => {
  const { lang } = useLang();
  const [feedback, setFeedback] = useState<'up' | 'down' | null>(null);
  const [copied, setCopied] = useState(false);

  const handleFeedback = async (value: 'up' | 'down') => {
    const next = feedback === value ? null : value;
    setFeedback(next);
    try {
      await invoke('submit_message_feedback', {
        messageId,
        channelId: channelId ?? null,
        feedback: next,
      });
    } catch {
      // silently ignore
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // fallback ignored
    }
  };

  const handleDownload = async () => {
    try {
      const path = await save({
        defaultPath: artifactName || 'artifact.txt',
      });
      if (path) {
        await invoke('save_artifact_to_file', { path, content });
      }
    } catch {
      // cancelled or error
    }
  };

  return (
    <div
      className="msg-actions"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '1px',
        marginTop: '4px',
        opacity: 0,
        transition: 'opacity 0.15s',
      }}
    >
      <ActionBtn
        active={feedback === 'up'}
        onClick={() => void handleFeedback('up')}
        title={t('msg_feedback_up', lang)}
      >
        <IconThumbUp filled={feedback === 'up'} />
      </ActionBtn>

      <ActionBtn
        active={feedback === 'down'}
        onClick={() => void handleFeedback('down')}
        title={t('msg_feedback_down', lang)}
      >
        <IconThumbDown filled={feedback === 'down'} />
      </ActionBtn>

      <ActionBtn
        onClick={handleCopy}
        title={t(copied ? 'msg_copied' : 'msg_copy', lang)}
      >
        {copied ? <IconCheck /> : <IconCopy />}
      </ActionBtn>

      {isArtifact && (
        <ActionBtn
          onClick={() => void handleDownload()}
          title={t('msg_download', lang)}
        >
          <IconDownload />
        </ActionBtn>
      )}
    </div>
  );
};

const ActionBtn: React.FC<{
  active?: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}> = ({ active, onClick, title, children }) => {
  const [hover, setHover] = useState(false);
  return (
    <button
      onClick={onClick}
      title={title}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: '26px',
        height: '22px',
        background: hover ? 'var(--color-background-secondary)' : 'none',
        border: 'none',
        borderRadius: '4px',
        cursor: 'pointer',
        color: active ? 'var(--color-text-info)' : hover ? 'var(--color-text-secondary)' : 'var(--color-text-tertiary)',
        transition: 'color 0.15s, background 0.15s',
        padding: 0,
      }}
    >
      {children}
    </button>
  );
};
