import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { type CSSProperties, type KeyboardEvent, useEffect, useMemo, useRef, useState } from 'react';
import { MarkdownRenderer } from './MarkdownRenderer';
import { t, useLang } from '../i18n';

type ConversationKind = 'group';
type TransportKind = 'local' | 'xmtp' | 'bridge' | 'relay' | 'lan_p2p';

type ConversationSpace = {
  id: string;
  kind: ConversationKind;
  title: string;
  description: string;
  owner_identity_id: string;
  created_at: number;
  updated_at: number;
  accent: string;
  invite_code: string;
  routing_mode: string;
  member_count: number;
  unread: number;
  transports: TransportBinding[];
};

type TransportBinding = {
  kind: TransportKind;
  label: string;
  status: 'active' | 'planned' | 'paused' | 'failed';
  transport_ref?: string | null;
};

type ConversationMessage = {
  id: string;
  conversation_id: string;
  sender_identity_id: string;
  sender_route_id?: string | null;
  sender_name: string;
  sender_kind: 'human' | 'agent' | 'system';
  route_label?: string | null;
  content: string;
  created_at: number;
  network_kind?: string | null;
  transport_inbox_id?: string | null;
  client_kind?: string | null;
  client_instance_id?: string | null;
  client_display_name?: string | null;
  actor_kind?: string | null;
  actor_id?: string | null;
  actor_display_name?: string | null;
  via_kind?: string | null;
  via_label?: string | null;
};

type Member = {
  id: string;
  conversation_id: string;
  identity_id: string;
  display_name: string;
  mention_key: string;
  role: 'owner' | 'admin' | 'member' | 'agent';
  status: string;
  route_label: string;
  online: boolean;
  accent: string;
  joined_at: number;
  last_seen_at?: number | null;
  network_kind?: string | null;
  transport_inbox_id?: string | null;
  client_kind?: string | null;
  client_instance_id?: string | null;
  client_display_name?: string | null;
  actor_kind?: string | null;
  actor_id?: string | null;
  actor_display_name?: string | null;
  via_kind?: string | null;
  via_label?: string | null;
};

type ConversationMessageCreatedPayload = {
  conversation_id: string;
  message: ConversationMessage;
};

type ConversationMembersChangedPayload = {
  conversation_id: string;
  members: Member[];
  member_count: number;
};

type ConversationSpacesChangedPayload = {
  spaces: ConversationSpace[];
};

type XmtpConversationBinding = {
  conversationId: string;
  transportRef: string;
  status: string;
};

type XmtpIdentity = {
  inboxId: string;
  env: string;
};

type XmtpStreamStatus = {
  status: 'started' | 'stopped' | 'reconnecting' | 'failed' | string;
  target?: string;
  mode?: string;
  error?: string;
};

const ACTIVE_SPACE_STORAGE_KEY = 'openpup_active_conversation_space';
const RIGHT_PANEL_STORAGE_KEY = 'openpup_group_right_open';

const dialogInputStyle: CSSProperties = {
  width: '100%',
  border: '0.5px solid var(--color-border-secondary)',
  borderRadius: '7px',
  background: 'var(--color-background-secondary)',
  color: 'var(--color-text-primary)',
  padding: '8px 9px',
  fontSize: '13px',
  outline: 'none',
};

function formatChatTime(ts: number) {
  const ms = ts < 10_000_000_000 ? ts * 1000 : ts;
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function initials(name: string) {
  if (name === '我') return '我';
  if (name.includes('.')) return name.split('.').map((part) => part[0]).join('').slice(0, 2).toUpperCase();
  return name.slice(0, 2).toUpperCase();
}

function memberGroup(member: Member): 'agents' | 'people' {
  return member.role === 'agent' ? 'agents' : 'people';
}

function canRemoveMember(member: Member) {
  return member.identity_id !== 'owner' && member.identity_id !== 'agent_alpha';
}

function localizeGroupRole(role: Member['role'], lang: 'zh' | 'en') {
  if (lang === 'zh') {
    if (role === 'agent') return 'Agent';
    if (role === 'owner') return 'Owner';
    if (role === 'admin') return 'Admin';
    return '成员';
  }
  if (role === 'agent') return 'Agent';
  if (role === 'owner') return 'Owner';
  if (role === 'admin') return 'Admin';
  return 'Member';
}

function localizeTransportStatus(status: TransportBinding['status'], lang: 'zh' | 'en') {
  if (lang === 'zh') {
    if (status === 'active') return '已启用';
    if (status === 'planned') return '计划中';
    if (status === 'paused') return '已暂停';
    if (status === 'failed') return '失败';
  }
  if (status === 'active') return 'Active';
  if (status === 'planned') return 'Planned';
  if (status === 'paused') return 'Paused';
  return 'Failed';
}

function localizeXmtpStreamStatus(status: XmtpStreamStatus | null, lang: 'zh' | 'en') {
  if (!status) return lang === 'zh' ? '监听未启动' : 'Stream not started';
  if (status.status === 'started') return lang === 'zh' ? '实时监听中' : 'Live stream active';
  if (status.status === 'reconnecting') return lang === 'zh' ? '正在重连' : 'Reconnecting';
  if (status.status === 'stopped') return lang === 'zh' ? '监听已停止' : 'Stream stopped';
  if (status.status === 'failed') return lang === 'zh' ? '监听失败' : 'Stream failed';
  return status.status;
}

function xmtpStreamTone(status: XmtpStreamStatus | null) {
  if (!status) return 'var(--color-text-tertiary)';
  if (status.status === 'started') return 'var(--color-text-success)';
  if (status.status === 'reconnecting') return '#B7791F';
  if (status.status === 'failed') return 'var(--color-text-danger)';
  return 'var(--color-text-tertiary)';
}

function extractMentionTokens(content: string) {
  return content
    .split(/\s+/)
    .map((token) => token.trim())
    .map((token) => token.replace(/^[^\S\r\n]*/, ''))
    .filter(Boolean)
    .map((token) => token.replace(/[，。,.:：;；)）(（]+$/g, ''))
    .filter((token) => token.startsWith('@') || token.startsWith('＠'))
    .map((token) => token.slice(1).toLowerCase())
    .map((token) => token.match(/^[a-z0-9._/-]+/)?.[0] ?? '')
    .filter(Boolean);
}

function messageMentionChips(message: ConversationMessage, members: Member[]) {
  const mentionSet = new Set(extractMentionTokens(message.content));
  if (!mentionSet.size) return [];
  return members.filter((member) => mentionSet.has(member.mention_key));
}

function transportTone(binding: TransportBinding) {
  if (binding.status === 'active') return 'var(--color-text-success)';
  if (binding.status === 'failed') return 'var(--color-text-danger)';
  if (binding.kind === 'xmtp') return '#378ADD';
  return 'var(--color-text-tertiary)';
}

function IconButton({
  label,
  children,
  onClick,
}: {
  label: string;
  children: React.ReactNode;
  onClick?: () => void;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      style={{
        width: '30px',
        height: '30px',
        borderRadius: '7px',
        border: '0.5px solid var(--color-border-tertiary)',
        background: 'var(--color-background-primary)',
        color: 'var(--color-text-secondary)',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        flexShrink: 0,
      }}
    >
      {children}
    </button>
  );
}

function PlusIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );
}

export function GroupChat() {
  const { lang } = useLang();
  const [spaces, setSpaces] = useState<ConversationSpace[]>([]);
  const [activeSpaceId, setActiveSpaceId] = useState(() => localStorage.getItem(ACTIVE_SPACE_STORAGE_KEY) || '');
  const [messagesBySpace, setMessagesBySpace] = useState<Record<string, ConversationMessage[]>>({});
  const [membersBySpace, setMembersBySpace] = useState<Record<string, Member[]>>({});
  const [draft, setDraft] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [createTitle, setCreateTitle] = useState('');
  const [addMemberOpen, setAddMemberOpen] = useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [removeMemberTarget, setRemoveMemberTarget] = useState<Member | null>(null);
  const [xmtpBusy, setXmtpBusy] = useState(false);
  const [xmtpIdentity, setXmtpIdentity] = useState<XmtpIdentity | null>(null);
  const [xmtpMemberId, setXmtpMemberId] = useState('');
  const [xmtpStreamStatus, setXmtpStreamStatus] = useState<XmtpStreamStatus | null>(null);
  const [rightOpen, setRightOpen] = useState(() => localStorage.getItem(RIGHT_PANEL_STORAGE_KEY) === 'true');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const imeComposingRef = useRef(false);
  const activeSpaceIdRef = useRef(activeSpaceId);
  const unlistensRef = useRef<UnlistenFn[]>([]);

  const activeSpace = useMemo(
    () => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0],
    [activeSpaceId, spaces],
  );
  const activeMessages = activeSpace ? messagesBySpace[activeSpace.id] ?? [] : [];
  const activeMembers = activeSpace ? membersBySpace[activeSpace.id] ?? [] : [];
  const activeXmtp = activeSpace?.transports.find((binding) => binding.kind === 'xmtp' && binding.status === 'active');

  const loadSpaces = async (preferredId?: string) => {
    setError(null);
    const nextSpaces = await invoke<ConversationSpace[]>('list_conversation_spaces');
    setSpaces(nextSpaces);
    const nextActive = preferredId || activeSpaceId || nextSpaces[0]?.id || '';
    if (nextActive && nextSpaces.some((space) => space.id === nextActive)) {
      setActiveSpaceId(nextActive);
    } else {
      setActiveSpaceId(nextSpaces[0]?.id || '');
    }
    return nextSpaces;
  };

  useEffect(() => {
    let cancelled = false;
    const boot = async () => {
      setLoading(true);
      try {
        const nextSpaces = await invoke<ConversationSpace[]>('list_conversation_spaces');
        if (cancelled) return;
        setSpaces(nextSpaces);
        const storedId = localStorage.getItem(ACTIVE_SPACE_STORAGE_KEY) || '';
        const nextActive = nextSpaces.some((space) => space.id === storedId) ? storedId : nextSpaces[0]?.id || '';
        setActiveSpaceId(nextActive);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void boot();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (activeSpaceId) localStorage.setItem(ACTIVE_SPACE_STORAGE_KEY, activeSpaceId);
    activeSpaceIdRef.current = activeSpaceId;
  }, [activeSpaceId]);

  useEffect(() => {
    localStorage.setItem(RIGHT_PANEL_STORAGE_KEY, String(rightOpen));
  }, [rightOpen]);

  useEffect(() => {
    if (!activeSpaceId) return;
    let cancelled = false;
    const loadActive = async () => {
      try {
        const [messages, members] = await Promise.all([
          invoke<ConversationMessage[]>('get_conversation_messages', { conversationId: activeSpaceId, limit: 200 }),
          invoke<Member[]>('get_conversation_members', { conversationId: activeSpaceId }),
        ]);
        if (cancelled) return;
        setMessagesBySpace((prev) => ({ ...prev, [activeSpaceId]: messages }));
        setMembersBySpace((prev) => ({ ...prev, [activeSpaceId]: members }));
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };
    void loadActive();
    return () => {
      cancelled = true;
    };
  }, [activeSpaceId]);

  useEffect(() => {
    let cancelled = false;
    const loadIdentity = async () => {
      if (!activeXmtp) {
        setXmtpIdentity(null);
        setXmtpStreamStatus(null);
        return;
      }
      try {
        const identity = await invoke<XmtpIdentity>('get_xmtp_identity');
        if (!cancelled) setXmtpIdentity(identity);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };
    void loadIdentity();
    return () => {
      cancelled = true;
    };
  }, [activeXmtp?.transport_ref]);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      listen<ConversationMessageCreatedPayload>('conversation_message_created', ({ payload }) => {
        setMessagesBySpace((prev) => {
          const current = prev[payload.conversation_id] ?? [];
          if (current.some((message) => message.id === payload.message.id)) return prev;
          return {
            ...prev,
            [payload.conversation_id]: [...current, payload.message],
          };
        });
      }),
      listen<ConversationMembersChangedPayload>('conversation_members_changed', ({ payload }) => {
        setMembersBySpace((prev) => ({ ...prev, [payload.conversation_id]: payload.members }));
        setSpaces((prev) => prev.map((space) => (
          space.id === payload.conversation_id
            ? { ...space, member_count: payload.member_count }
            : space
        )));
      }),
      listen<ConversationSpacesChangedPayload>('conversation_spaces_changed', ({ payload }) => {
        setSpaces(payload.spaces);
        const activeId = activeSpaceIdRef.current;
        if (activeId && payload.spaces.some((space) => space.id === activeId)) return;
        setActiveSpaceId(payload.spaces[0]?.id || '');
      }),
      listen<XmtpStreamStatus>('xmtp_stream_status', ({ payload }) => {
        setXmtpStreamStatus(payload);
      }),
    ]).then((unlistens) => {
      if (cancelled) {
        unlistens.forEach((unlisten) => unlisten());
      } else {
        unlistensRef.current.push(...unlistens);
      }
    });

    return () => {
      cancelled = true;
      unlistensRef.current.forEach((unlisten) => unlisten());
      unlistensRef.current = [];
    };
  }, []);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [activeSpaceId, activeMessages.length]);

  useEffect(() => {
    if (!pickerOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (pickerRef.current?.contains(event.target as Node)) return;
      setPickerOpen(false);
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') {
        setPickerOpen(false);
        setRightOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [pickerOpen]);

  useEffect(() => {
    if (!rightOpen) return;
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') setRightOpen(false);
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [rightOpen]);

  const openCreateDialog = () => {
    setCreateTitle('');
    setCreateOpen(true);
    setPickerOpen(false);
  };

  const createSpace = async () => {
    const cleanTitle = createTitle.trim();
    if (!cleanTitle) return;
    try {
      const nextSpace = await invoke<ConversationSpace>('create_conversation_space', { title: cleanTitle });
      setSpaces((prev) => [nextSpace, ...prev.filter((space) => space.id !== nextSpace.id)]);
      setActiveSpaceId(nextSpace.id);
      setPickerOpen(false);
      setCreateOpen(false);
      setCreateTitle('');
      await loadSpaces(nextSpace.id);
      requestAnimationFrame(() => inputRef.current?.focus());
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteActiveSpace = async () => {
    if (!activeSpace) return;
    try {
      if (activeXmtp) {
        await invoke('leave_xmtp_conversation', { conversationId: activeSpace.id });
      }
      await invoke('delete_conversation_space', { conversationId: activeSpace.id });
      setMessagesBySpace((prev) => {
        const next = { ...prev };
        delete next[activeSpace.id];
        return next;
      });
      setMembersBySpace((prev) => {
        const next = { ...prev };
        delete next[activeSpace.id];
        return next;
      });
      const nextSpaces = await loadSpaces();
      const nextActive = nextSpaces.find((space) => space.id !== activeSpace.id)?.id || nextSpaces[0]?.id || '';
      setActiveSpaceId(nextActive);
      setDeleteConfirmOpen(false);
    } catch (e) {
      setError(String(e));
    }
  };

  const enableXmtp = async () => {
    if (!activeSpace || xmtpBusy) return;
    setXmtpBusy(true);
    setError(null);
    try {
      await invoke<XmtpConversationBinding>('enable_xmtp_for_conversation', {
        conversationId: activeSpace.id,
      });
      const identity = await invoke<XmtpIdentity>('get_xmtp_identity');
      setXmtpIdentity(identity);
      await loadSpaces(activeSpace.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setXmtpBusy(false);
    }
  };

  const copyXmtpId = async () => {
    if (!xmtpIdentity?.inboxId) return;
    try {
      await navigator.clipboard.writeText(xmtpIdentity.inboxId);
    } catch {
      setError(t('group_xmtp_copy_failed', lang));
    }
  };

  const removeMember = async () => {
    if (!activeSpace || !removeMemberTarget) return;
    try {
      if (activeXmtp && removeMemberTarget.transport_inbox_id) {
        await invoke('remove_xmtp_conversation_member', {
          conversationId: activeSpace.id,
          inboxId: removeMemberTarget.transport_inbox_id,
        });
      }
      const localTargets = removeMemberTarget.transport_inbox_id
        ? activeMembers.filter((member) => (
          member.transport_inbox_id === removeMemberTarget.transport_inbox_id && canRemoveMember(member)
        ))
        : [removeMemberTarget];
      for (const member of localTargets) {
        await invoke('remove_conversation_member', {
          conversationId: activeSpace.id,
          identityId: member.identity_id,
        });
      }
      const [messages, members, nextSpaces] = await Promise.all([
        invoke<ConversationMessage[]>('get_conversation_messages', { conversationId: activeSpace.id, limit: 200 }),
        invoke<Member[]>('get_conversation_members', { conversationId: activeSpace.id }),
        invoke<ConversationSpace[]>('list_conversation_spaces'),
      ]);
      setMessagesBySpace((prev) => ({ ...prev, [activeSpace.id]: messages }));
      setMembersBySpace((prev) => ({ ...prev, [activeSpace.id]: members }));
      setSpaces(nextSpaces);
      setRemoveMemberTarget(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const addXmtpMember = async () => {
    if (!activeSpace || xmtpBusy) return;
    const inboxId = xmtpMemberId.trim();
    if (!inboxId) return;
    setXmtpBusy(true);
    setError(null);
    try {
      await invoke('add_xmtp_conversation_member', {
        conversationId: activeSpace.id,
        inboxId,
      });
      setXmtpMemberId('');
      setAddMemberOpen(false);
      await Promise.all([
        loadSpaces(activeSpace.id),
        invoke<Member[]>('get_conversation_members', { conversationId: activeSpace.id }).then((members) => {
          setMembersBySpace((prev) => ({ ...prev, [activeSpace.id]: members }));
        }),
      ]);
    } catch (e) {
      setError(String(e));
    } finally {
      setXmtpBusy(false);
    }
  };

  const syncXmtpGroups = async () => {
    if (xmtpBusy) return;
    setXmtpBusy(true);
    setError(null);
    try {
      await invoke('sync_xmtp_groups');
      await loadSpaces(activeSpace?.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setXmtpBusy(false);
    }
  };

  const send = async () => {
    const content = draft.trim();
    if (!content || !activeSpace || sending) return;
    setSending(true);
    setDraft('');
    if (inputRef.current) inputRef.current.style.height = 'auto';
    try {
      const next = await invoke<ConversationMessage>('post_conversation_message', {
        conversationId: activeSpace.id,
        content,
      });
      setMessagesBySpace((prev) => {
        const current = prev[activeSpace.id] ?? [];
        if (current.some((message) => message.id === next.id)) return prev;
        return {
          ...prev,
          [activeSpace.id]: [...current, next],
        };
      });
    } catch (e) {
      setDraft(content);
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  const insertMention = (mentionKey: string) => {
    const mention = `@${mentionKey}`;
    setDraft((prev) => {
      const base = prev.trimEnd();
      if (!base) return `${mention} `;
      if (base.endsWith(mention) || base.includes(`${mention} `)) return `${base} `;
      return `${base} ${mention} `;
    });
    requestAnimationFrame(() => {
      inputRef.current?.focus();
      const value = inputRef.current?.value ?? '';
      const length = value.length;
      inputRef.current?.setSelectionRange(length, length);
      if (inputRef.current) {
        inputRef.current.style.height = 'auto';
        inputRef.current.style.height = `${Math.min(inputRef.current.scrollHeight, 150)}px`;
      }
    });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== 'Enter' || e.shiftKey) return;
    const ne = e.nativeEvent;
    if (imeComposingRef.current || ne.isComposing || ne.keyCode === 229) return;
    e.preventDefault();
    void send();
  };

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center" style={{ color: 'var(--color-text-tertiary)', background: 'var(--color-background-primary)', fontSize: '13px' }}>
        {t('group_loading', lang)}
      </div>
    );
  }

  if (!activeSpace) {
    return (
      <div className="flex-1 flex items-center justify-center" style={{ background: 'var(--color-background-primary)', color: 'var(--color-text-secondary)' }}>
        <div style={{ display: 'grid', gap: '12px', justifyItems: 'center' }}>
          <div style={{ fontSize: '14px', fontWeight: 650, color: 'var(--color-text-primary)' }}>{t('group_empty', lang)}</div>
          <button onClick={openCreateDialog} style={{ display: 'inline-flex', alignItems: 'center', gap: '8px', padding: '8px 12px', border: 'none', borderRadius: '7px', background: 'var(--color-text-primary)', color: 'var(--color-background-primary)', cursor: 'pointer', fontSize: '13px' }}>
            <PlusIcon /> {t('group_create', lang)}
          </button>
          {error && <div style={{ fontSize: '12px', color: 'var(--color-text-danger)' }}>{error}</div>}
          {createOpen && (
            <InlineDialog
              title={t('group_create', lang)}
              primaryLabel={t('group_create', lang)}
              primaryDisabled={!createTitle.trim()}
              onCancel={() => setCreateOpen(false)}
              onPrimary={() => void createSpace()}
            >
              <input
                autoFocus
                value={createTitle}
                placeholder={t('group_name_ph', lang)}
                onChange={(e) => setCreateTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void createSpace();
                }}
                style={dialogInputStyle}
              />
            </InlineDialog>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-hidden flex" style={{ background: 'var(--color-background-primary)', position: 'relative' }}>
      <main style={{ minWidth: 0, flex: 1, display: 'flex', flexDirection: 'column' }}>
        <header style={{ minHeight: '60px', flexShrink: 0, borderBottom: '0.5px solid var(--color-border-tertiary)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '0 16px', background: 'var(--color-background-primary)', position: 'relative', zIndex: 3 }}>
          <div ref={pickerRef} style={{ minWidth: 0, position: 'relative' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <button
                onClick={() => setPickerOpen((value) => !value)}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '9px',
                  border: 'none',
                  background: 'transparent',
                  color: 'var(--color-text-primary)',
                  cursor: 'pointer',
                  padding: '4px 6px 4px 0',
                  borderRadius: '7px',
                }}
              >
                <span style={{ width: '28px', height: '28px', borderRadius: '7px', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', color: activeSpace.accent, background: `${activeSpace.accent}1f`, fontWeight: 750, border: `0.5px solid ${activeSpace.accent}33` }}>#</span>
                <span style={{ fontSize: '15px', fontWeight: 700 }}>{activeSpace.title}</span>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ color: 'var(--color-text-tertiary)' }}>
                  <path d="M6 9l6 6 6-6" />
                </svg>
              </button>
              <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{activeSpace.description}</span>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px', marginTop: '4px', flexWrap: 'wrap' }}>
              {[activeSpace.invite_code, activeSpace.routing_mode, t('group_members_count', lang).replace('{count}', String(activeSpace.member_count))].map((item) => (
                <span key={item} style={{ fontSize: '10px', color: 'var(--color-text-tertiary)', padding: '2px 6px', borderRadius: '999px', background: 'var(--color-background-secondary)' }}>{item}</span>
              ))}
              {activeSpace.transports.map((binding) => (
                <span key={`${binding.kind}-${binding.status}-${binding.transport_ref || ''}`} style={{ fontSize: '10px', color: transportTone(binding), padding: '2px 6px', borderRadius: '999px', background: 'var(--color-background-secondary)' }}>
                  {binding.label}{binding.status === 'planned' ? ` · ${localizeTransportStatus(binding.status, lang)}` : ''}
                </span>
              ))}
            </div>
            {pickerOpen && (
              <div style={{ position: 'absolute', top: '52px', left: 0, width: '320px', padding: '8px', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', boxShadow: '0 18px 48px rgba(0,0,0,0.18)', zIndex: 10 }}>
                {spaces.map((space) => {
                  const active = space.id === activeSpace.id;
                  const latest = (messagesBySpace[space.id] ?? []).filter((m) => m.sender_kind !== 'system').at(-1);
                  return (
                    <button
                      key={space.id}
                      onClick={() => {
                        setActiveSpaceId(space.id);
                        setPickerOpen(false);
                        inputRef.current?.focus();
                      }}
                      style={{
                        width: '100%',
                        display: 'flex',
                        gap: '10px',
                        alignItems: 'center',
                        padding: '9px',
                        border: 'none',
                        borderRadius: '7px',
                        background: active ? 'var(--color-background-secondary)' : 'transparent',
                        color: active ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                        cursor: 'pointer',
                        textAlign: 'left',
                      }}
                    >
                      <span style={{ width: '32px', height: '32px', borderRadius: '8px', flexShrink: 0, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', background: `${space.accent}1f`, color: space.accent, fontSize: '14px', fontWeight: 700, border: `0.5px solid ${space.accent}33` }}>#</span>
                      <span style={{ minWidth: 0, flex: 1 }}>
                        <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px' }}>
                          <span style={{ fontSize: '13px', fontWeight: active ? 650 : 550, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{space.title}</span>
                          {space.unread ? <span style={{ minWidth: '18px', height: '18px', padding: '0 6px', borderRadius: '999px', background: space.accent, color: '#fff', fontSize: '10px', lineHeight: '18px', textAlign: 'center', fontWeight: 650 }}>{space.unread}</span> : null}
                        </span>
                        <span style={{ display: 'block', marginTop: '3px', fontSize: '11px', color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {latest ? `${latest.sender_name}: ${latest.content}` : space.description}
                        </span>
                      </span>
                    </button>
                  );
                })}
                <div style={{ height: '0.5px', background: 'var(--color-border-tertiary)', margin: '6px 2px' }} />
                <button onClick={openCreateDialog} style={{ width: '100%', display: 'flex', alignItems: 'center', gap: '8px', padding: '8px 9px', border: 'none', borderRadius: '7px', background: 'transparent', color: 'var(--color-text-secondary)', cursor: 'pointer', fontSize: '12px', textAlign: 'left' }}>
                  <PlusIcon /> {t('group_create', lang)}
                </button>
              </div>
            )}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            {error && <span style={{ maxWidth: '240px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--color-text-danger)', fontSize: '11px' }}>{error}</span>}
            {activeXmtp && (
              <IconButton label={t('group_add_member', lang)} onClick={() => setAddMemberOpen(true)}>
                <PlusIcon />
              </IconButton>
            )}
            <IconButton label={t('group_leave', lang)} onClick={() => setDeleteConfirmOpen(true)}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </IconButton>
            <IconButton label={rightOpen ? t('group_members_close', lang) : t('group_members_open', lang)} onClick={() => setRightOpen((value) => !value)}><UserIcon /></IconButton>
          </div>
        </header>

        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 18px', background: 'var(--color-background-primary)' }}>
          <div style={{ maxWidth: '800px', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: '6px' }}>
            {activeMessages.map((message) => (
              <ConversationMessageRow
                key={message.id}
                message={message}
                accent={activeSpace.accent}
                members={activeMembers}
                onMentionClick={insertMention}
              />
            ))}
            <div ref={messagesEndRef} />
          </div>
        </div>

        <div style={{ flexShrink: 0, padding: '12px 18px 14px', borderTop: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
          <div style={{ maxWidth: '800px', margin: '0 auto', width: '100%' }}>
            <div style={{ display: 'flex', alignItems: 'flex-end', gap: '9px', padding: '8px', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-secondary)' }}>
              <textarea
                ref={inputRef}
                rows={1}
                placeholder={activeXmtp ? t('group_composer_placeholder_xmtp', lang).replace('{title}', activeSpace.title) : t('group_composer_placeholder', lang).replace('{title}', activeSpace.title)}
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                  const el = e.target;
                  el.style.height = 'auto';
                  el.style.height = `${Math.min(el.scrollHeight, 150)}px`;
                }}
                onCompositionStart={() => { imeComposingRef.current = true; }}
                onCompositionEnd={() => { imeComposingRef.current = false; }}
                onKeyDown={onKeyDown}
                style={{ flex: 1, resize: 'none', outline: 'none', fontSize: '13px', padding: '5px 3px', border: 'none', background: 'transparent', color: 'var(--color-text-primary)', fontFamily: 'inherit', lineHeight: '1.5', overflowY: 'auto' }}
              />
              <button onClick={() => void send()} disabled={!draft.trim() || sending} style={{ width: '30px', height: '30px', borderRadius: '7px', border: 'none', background: activeSpace.accent, color: '#fff', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: draft.trim() && !sending ? 'pointer' : 'not-allowed', opacity: draft.trim() && !sending ? 1 : 0.35, flexShrink: 0 }}>
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M12 19V5M5 12l7-7 7 7" />
                </svg>
              </button>
            </div>
          </div>
        </div>
      </main>

      {rightOpen && (
        <>
          <button
            aria-label={t('group_members_close', lang)}
            onClick={() => setRightOpen(false)}
            style={{
              position: 'absolute',
              inset: '0 0 0 0',
              border: 'none',
              background: 'rgba(0,0,0,0.05)',
              cursor: 'default',
              zIndex: 5,
            }}
          />
          <aside style={{ position: 'absolute', top: 0, right: 0, bottom: 0, width: '268px', borderLeft: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-secondary)', overflowY: 'auto', boxShadow: '-20px 0 50px rgba(0,0,0,0.16)', zIndex: 6 }}>
            <div style={{ padding: '14px 12px 12px', borderBottom: '0.5px solid var(--color-border-tertiary)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
              <div>
                <div style={{ fontSize: '13px', fontWeight: 650, color: 'var(--color-text-primary)' }}>{t('group_members', lang)}</div>
                <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', marginTop: '3px' }}>{t('group_members_count', lang).replace('{count}', String(activeSpace.member_count))}</div>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                {activeXmtp && (
                  <IconButton label={t('group_add_member', lang)} onClick={() => setAddMemberOpen(true)}>
                    <PlusIcon />
                  </IconButton>
                )}
                <IconButton label={t('group_members_close', lang)} onClick={() => setRightOpen(false)}>
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                    <path d="M18 6L6 18M6 6l12 12" />
                  </svg>
                </IconButton>
              </div>
            </div>

            <MemberSection title={t('group_agents', lang)} members={activeMembers.filter((member) => memberGroup(member) === 'agents')} onRemove={(member) => setRemoveMemberTarget(member)} removeLabel={t('group_kick_member', lang)} />
            <MemberSection title={t('group_people', lang)} members={activeMembers.filter((member) => memberGroup(member) === 'people')} onRemove={(member) => setRemoveMemberTarget(member)} removeLabel={t('group_kick_member', lang)} />

            <div style={{ margin: '10px 12px 12px', padding: '10px', borderRadius: '8px', border: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
              <div style={{ fontSize: '11px', fontWeight: 650, color: 'var(--color-text-secondary)' }}>{t('group_transport', lang)}</div>
              <div style={{ marginTop: '6px', display: 'grid', gap: '5px' }}>
                {activeSpace.transports.map((binding) => (
                  <div key={`${binding.kind}-${binding.transport_ref || ''}`} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px', fontSize: '11px', color: 'var(--color-text-tertiary)' }}>
                    <span>{binding.label}</span>
                    <span style={{ color: transportTone(binding) }}>{localizeTransportStatus(binding.status, lang)}</span>
                  </div>
                ))}
              </div>
              <div style={{ marginTop: '9px', display: 'grid', gap: '7px' }}>
                <button
                  onClick={() => void enableXmtp()}
                  disabled={!!activeXmtp || xmtpBusy}
                  style={{ border: 'none', borderRadius: '7px', background: activeXmtp ? 'var(--color-background-secondary)' : activeSpace.accent, color: activeXmtp ? 'var(--color-text-tertiary)' : '#fff', padding: '7px 9px', fontSize: '11px', cursor: activeXmtp || xmtpBusy ? 'not-allowed' : 'pointer', textAlign: 'center' }}
                >
                  {activeXmtp ? t('group_xmtp_enabled', lang) : xmtpBusy ? t('group_connecting', lang) : t('group_enable_xmtp', lang)}
                </button>
                {activeXmtp && (
                  <>
                    <div style={{ display: 'grid', gap: '4px' }}>
                      <div style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>{t('group_id', lang)}</div>
                      <div style={{ fontSize: '10px', color: 'var(--color-text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{activeXmtp.transport_ref}</div>
                    </div>
                    <div style={{ display: 'grid', gap: '4px' }}>
                      <div style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>{t('group_xmtp_stream', lang)}</div>
                      <div style={{ display: 'flex', alignItems: 'center', gap: '6px', minWidth: 0 }}>
                        <span style={{ width: '6px', height: '6px', borderRadius: '999px', background: xmtpStreamTone(xmtpStreamStatus), flexShrink: 0 }} />
                        <span style={{ fontSize: '10px', color: xmtpStreamTone(xmtpStreamStatus), overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {localizeXmtpStreamStatus(xmtpStreamStatus, lang)}
                        </span>
                      </div>
                      {xmtpStreamStatus?.status === 'reconnecting' && xmtpStreamStatus.error && (
                        <div style={{ fontSize: '10px', color: 'var(--color-text-tertiary)', lineHeight: 1.4, wordBreak: 'break-word' }}>
                          {xmtpStreamStatus.error}
                        </div>
                      )}
                    </div>
                    {xmtpIdentity?.inboxId && (
                      <div style={{ display: 'grid', gap: '5px' }}>
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                          <span style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>{t('group_my_xmtp_id', lang)}</span>
                          <button
                            onClick={() => void copyXmtpId()}
                            style={{ border: 'none', borderRadius: '6px', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', padding: '4px 7px', fontSize: '10px', cursor: 'pointer' }}
                          >
                            {t('msg_copy', lang)}
                          </button>
                        </div>
                        <div style={{ fontSize: '10px', color: 'var(--color-text-secondary)', lineHeight: 1.45, wordBreak: 'break-all' }}>
                          {xmtpIdentity.inboxId}
                        </div>
                      </div>
                    )}
                    <button
                      onClick={() => void syncXmtpGroups()}
                      disabled={xmtpBusy}
                      style={{ border: '0.5px solid var(--color-border-tertiary)', borderRadius: '7px', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', padding: '7px 9px', fontSize: '11px', cursor: xmtpBusy ? 'not-allowed' : 'pointer', textAlign: 'center', opacity: xmtpBusy ? 0.55 : 1 }}
                    >
                      {t('group_sync_xmtp', lang)}
                    </button>
                  </>
                )}
                {!activeXmtp && (
                  <button
                    onClick={() => void syncXmtpGroups()}
                    disabled={xmtpBusy}
                    style={{ border: '0.5px solid var(--color-border-tertiary)', borderRadius: '7px', background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', padding: '7px 9px', fontSize: '11px', cursor: xmtpBusy ? 'not-allowed' : 'pointer', textAlign: 'center', opacity: xmtpBusy ? 0.55 : 1 }}
                  >
                    {t('group_sync_xmtp', lang)}
                  </button>
                )}
              </div>
            </div>

            <div style={{ margin: '10px 12px 12px', padding: '10px', borderRadius: '8px', border: '0.5px solid var(--color-border-tertiary)', background: 'var(--color-background-primary)' }}>
              <div style={{ fontSize: '11px', fontWeight: 650, color: 'var(--color-text-secondary)' }}>{t('nav_bridge', lang)}</div>
              <div style={{ marginTop: '6px', display: 'grid', gap: '5px' }}>
                {[t('group_bridge_personal', lang), `/g ${activeSpace.title} ${t('group_bridge_content', lang)}`, `/use ${activeSpace.title}`].map((line) => (
                  <div key={line} style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{line}</div>
                ))}
              </div>
            </div>
          </aside>
        </>
      )}

      {createOpen && (
        <InlineDialog
          title={t('group_create', lang)}
          primaryLabel={t('group_create', lang)}
          primaryDisabled={!createTitle.trim()}
          onCancel={() => setCreateOpen(false)}
          onPrimary={() => void createSpace()}
        >
          <input
            autoFocus
            value={createTitle}
            placeholder={t('group_name_ph', lang)}
            onChange={(e) => setCreateTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void createSpace();
            }}
            style={dialogInputStyle}
          />
        </InlineDialog>
      )}

      {deleteConfirmOpen && activeSpace && (
        <InlineDialog
          title={t('group_leave', lang)}
          primaryLabel={t('group_leave_confirm', lang)}
          onCancel={() => setDeleteConfirmOpen(false)}
          onPrimary={() => void deleteActiveSpace()}
        >
          <div style={{ display: 'grid', gap: '8px', fontSize: '12px', color: 'var(--color-text-secondary)' }}>
            <div>{t('group_leave_confirm_body', lang).replace('{title}', activeSpace.title)}</div>
            <div style={{ color: 'var(--color-text-tertiary)' }}>{t('group_leave_confirm_hint', lang)}</div>
          </div>
        </InlineDialog>
      )}

      {addMemberOpen && activeSpace && activeXmtp && (
        <InlineDialog
          title={t('group_add_member', lang)}
          primaryLabel={t('group_add_member', lang)}
          primaryDisabled={xmtpBusy || !xmtpMemberId.trim()}
          onCancel={() => {
            setAddMemberOpen(false);
            setXmtpMemberId('');
          }}
          onPrimary={() => void addXmtpMember()}
        >
          <div style={{ display: 'grid', gap: '8px' }}>
            <input
              autoFocus
              value={xmtpMemberId}
              placeholder={t('group_other_xmtp_id', lang)}
              onChange={(e) => setXmtpMemberId(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void addXmtpMember();
              }}
              style={dialogInputStyle}
            />
            <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', lineHeight: 1.45 }}>
              {t('group_add_member_hint', lang)}
            </div>
          </div>
        </InlineDialog>
      )}

      {removeMemberTarget && activeSpace && (
        <InlineDialog
          title={t('group_kick_member', lang)}
          primaryLabel={t('group_kick_confirm', lang)}
          onCancel={() => setRemoveMemberTarget(null)}
          onPrimary={() => void removeMember()}
        >
          <div style={{ display: 'grid', gap: '8px', fontSize: '12px', color: 'var(--color-text-secondary)' }}>
            <div>{t('group_kick_confirm_body', lang).replace('{name}', removeMemberTarget.display_name)}</div>
            <div style={{ color: 'var(--color-text-tertiary)' }}>{t('group_kick_confirm_hint', lang)}</div>
          </div>
        </InlineDialog>
      )}
    </div>
  );
}

function InlineDialog({
  title,
  children,
  primaryLabel,
  primaryDisabled,
  onCancel,
  onPrimary,
}: {
  title: string;
  children: React.ReactNode;
  primaryLabel: string;
  primaryDisabled?: boolean;
  onCancel: () => void;
  onPrimary: () => void;
}) {
  const { lang } = useLang();
  return (
    <div style={{ position: 'fixed', inset: 0, zIndex: 80, background: 'rgba(0,0,0,0.32)', display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }}>
      <div style={{ width: 'min(360px, 100%)', borderRadius: '8px', border: '0.5px solid var(--color-border-secondary)', background: 'var(--color-background-primary)', boxShadow: '0 24px 70px rgba(0,0,0,0.28)', padding: '14px' }}>
        <div style={{ fontSize: '14px', fontWeight: 700, color: 'var(--color-text-primary)', marginBottom: '12px' }}>{title}</div>
        {children}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px', marginTop: '14px' }}>
          <button onClick={onCancel} style={{ border: '0.5px solid var(--color-border-secondary)', borderRadius: '7px', background: 'transparent', color: 'var(--color-text-secondary)', padding: '7px 10px', fontSize: '12px', cursor: 'pointer' }}>{t('common_cancel', lang)}</button>
          <button disabled={primaryDisabled} onClick={onPrimary} style={{ border: 'none', borderRadius: '7px', background: 'var(--color-text-primary)', color: 'var(--color-background-primary)', padding: '7px 12px', fontSize: '12px', cursor: primaryDisabled ? 'not-allowed' : 'pointer', opacity: primaryDisabled ? 0.45 : 1 }}>{primaryLabel}</button>
        </div>
      </div>
    </div>
  );
}

function MemberSection({
  title,
  members,
  onRemove,
  removeLabel,
}: {
  title: string;
  members: Member[];
  onRemove: (member: Member) => void;
  removeLabel: string;
}) {
  const { lang } = useLang();
  return (
    <div style={{ padding: '12px 12px 2px' }}>
      <div style={{ fontSize: '10px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', fontWeight: 700, marginBottom: '7px' }}>{title}</div>
      {members.map((member) => (
        <div key={member.id} style={{ display: 'flex', gap: '9px', alignItems: 'center', padding: '7px 0' }}>
          <Avatar name={member.display_name} accent={member.accent} online={member.online} />
          <span style={{ minWidth: 0, flex: 1 }}>
            <span style={{ display: 'block', fontSize: '12px', color: 'var(--color-text-primary)', fontWeight: 550 }}>{member.display_name}</span>
            <span style={{ display: 'block', fontSize: '10px', color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {member.client_display_name || member.actor_kind || member.via_label
                ? [
                    member.client_display_name,
                    member.actor_kind || localizeGroupRole(member.role, lang),
                    member.via_label,
                  ].filter(Boolean).join(' · ')
                : `@${member.mention_key} · ${localizeGroupRole(member.role, lang)} · ${member.route_label}`}
            </span>
          </span>
          {canRemoveMember(member) && (
            <button
              onClick={() => onRemove(member)}
              title={removeLabel}
              style={{ border: 'none', borderRadius: '6px', background: 'var(--color-background-primary)', color: 'var(--color-text-tertiary)', width: '24px', height: '24px', cursor: 'pointer', flexShrink: 0 }}
            >
              ×
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

function Avatar({ name, accent, online }: { name: string; accent: string; online?: boolean }) {
  return (
    <span style={{ position: 'relative', width: '30px', height: '30px', borderRadius: '8px', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', background: `${accent}1f`, color: accent, border: `0.5px solid ${accent}33`, fontSize: '11px', fontWeight: 750, flexShrink: 0 }}>
      {initials(name)}
      {online !== undefined && (
        <span style={{ position: 'absolute', right: '-2px', bottom: '-2px', width: '9px', height: '9px', borderRadius: '999px', background: online ? '#1D9E75' : 'var(--color-text-tertiary)', border: '2px solid var(--color-background-secondary)' }} />
      )}
    </span>
  );
}

function ConversationMessageRow({
  message,
  accent,
  members,
  onMentionClick,
}: {
  message: ConversationMessage;
  accent: string;
  members: Member[];
  onMentionClick: (mentionKey: string) => void;
}) {
  if (message.sender_kind === 'system') {
    return (
      <div style={{ alignSelf: 'center', maxWidth: '680px', color: 'var(--color-text-tertiary)', fontSize: '11px', padding: '4px 9px', borderRadius: '999px', background: 'var(--color-background-secondary)', margin: '5px 0' }}>
        {message.content}
      </div>
    );
  }

  const isMe = message.sender_name === '我';
  const isAgent = message.sender_kind === 'agent';
  const rowAccent = isMe ? '#378ADD' : isAgent ? accent : '#7F77DD';
  const mentionChips = messageMentionChips(message, members);
  return (
    <div className="animate-msg-in" style={{ display: 'flex', gap: '10px', alignItems: 'flex-start', padding: '7px 4px', justifyContent: isMe ? 'flex-end' : 'flex-start' }}>
      {!isMe && <Avatar name={message.sender_name} accent={rowAccent} />}
      <div style={{ maxWidth: '78%', minWidth: 0, display: 'flex', flexDirection: 'column', alignItems: isMe ? 'flex-end' : 'flex-start' }}>
        <div style={{ display: 'flex', gap: '7px', alignItems: 'baseline', marginBottom: '4px', padding: isMe ? '0 4px 0 0' : '0 0 0 2px' }}>
          <span style={{ fontSize: '12px', fontWeight: 650, color: isAgent ? 'var(--color-text-success)' : 'var(--color-text-secondary)' }}>{message.sender_name}</span>
          {message.route_label && <span style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>{message.route_label}</span>}
          <span style={{ fontSize: '10px', color: 'var(--color-text-tertiary)' }}>{formatChatTime(message.created_at)}</span>
        </div>
        {mentionChips.length > 0 && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', marginBottom: '6px', padding: isMe ? '0 4px 0 0' : '0 0 0 2px' }}>
            {mentionChips.map((member) => (
              <button
                key={`${message.id}-${member.id}`}
                onClick={() => onMentionClick(member.mention_key)}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '4px',
                  maxWidth: '220px',
                  padding: '2px 7px',
                  borderRadius: '999px',
                  background: 'var(--color-background-secondary)',
                  color: member.role === 'agent' ? 'var(--color-text-success)' : 'var(--color-text-secondary)',
                  fontSize: '10px',
                  border: '0.5px solid var(--color-border-tertiary)',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  cursor: 'pointer',
                }}
                title={`@${member.mention_key}`}
              >
                @{member.mention_key}
              </button>
            ))}
          </div>
        )}
        <div style={{ background: isMe ? 'var(--color-background-info)' : 'var(--color-background-secondary)', color: isMe ? 'var(--color-text-info)' : 'var(--color-text-primary)', border: isMe ? 'none' : '0.5px solid var(--color-border-tertiary)', borderRadius: isMe ? '8px 8px 3px 8px' : '8px 8px 8px 3px', borderLeft: isAgent ? `2px solid ${accent}` : undefined, padding: '9px 11px', fontSize: '13px', lineHeight: 1.6, boxShadow: '0 1px 2px rgba(0,0,0,0.03)', width: '100%', overflowX: 'auto' }}>
          <div className="prose prose-sm max-w-none" style={{ color: isMe ? 'var(--color-text-info)' : 'var(--color-text-primary)' }}>
            <MarkdownRenderer>{message.content}</MarkdownRenderer>
          </div>
        </div>
      </div>
      {isMe && <Avatar name={message.sender_name} accent={rowAccent} />}
    </div>
  );
}
