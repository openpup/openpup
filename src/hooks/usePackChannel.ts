import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type { ChannelMessageRecord, ChannelRecord, DelegationPlan } from '../types/channel';

interface UsePackChannelResult {
  channels: ChannelRecord[];
  activeChannelId: string | null;
  messages: ChannelMessageRecord[];
  plan: DelegationPlan | null;
  isCompleted: boolean;
  completionEventCount: number;
  activeCount: number;
  error: string | null;
  openChannel: (id: string) => Promise<void>;
  setActiveChannelId: (id: string | null) => void;
  refreshChannels: () => Promise<void>;
  clearCompletedChannels: () => Promise<void>;
  clearStaleChannels: () => Promise<void>;
}

export function usePackChannel(): UsePackChannelResult {
  const [channels, setChannels] = useState<ChannelRecord[]>([]);
  const [activeChannelId, setActiveChannelId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChannelMessageRecord[]>([]);
  const [plan, setPlan] = useState<DelegationPlan | null>(null);
  const [isCompleted, setIsCompleted] = useState(false);
  const [completionEventCount, setCompletionEventCount] = useState(0);
  const [activeCount, setActiveCount] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const unlistens = useRef<UnlistenFn[]>([]);
  const activeChannelIdRef = useRef<string | null>(null);
  const preferredChannelIdRef = useRef<string | null>(null);

  const refreshChannels = useCallback(async () => {
    try {
      const [list, count] = await Promise.all([
        invoke<ChannelRecord[]>('list_channels'),
        invoke<number>('get_active_channel_count'),
      ]);
      setChannels(list);
      setActiveCount(count);
      setError(null);
      setActiveChannelId((prev) => {
        const preferred = preferredChannelIdRef.current;
        if (preferred && list.some((channel) => channel.id === preferred)) {
          return preferred;
        }
        if (prev && list.some((channel) => channel.id === prev)) return prev;
        // A just-created channel can briefly lag behind list_channels; keep the
        // current selection so delegation_plan/channel_message events still bind.
        if (prev) return prev;
        return list.find((channel) => channel.status === 'active')?.id ?? null;
      });
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const openChannel = useCallback(async (id: string) => {
    try {
      preferredChannelIdRef.current = id;
      const [history, savedPlan] = await Promise.all([
        invoke<ChannelMessageRecord[]>('get_channel_messages', { channelId: id }),
        invoke<DelegationPlan | null>('get_channel_plan', { channelId: id }),
      ]);
      setActiveChannelId(id);
      setMessages(history);
      setPlan(savedPlan);
      setError(null);
    } catch (err) {
      setError(String(err));
      setMessages([]);
      setPlan(null);
    }
  }, []);

  const clearCompletedChannels = useCallback(async () => {
    try {
      await invoke<number>('clear_completed_channels');
      const currentId = activeChannelIdRef.current;
      const current = channels.find((channel) => channel.id === currentId);
      if (current?.status === 'active' && currentId) {
        preferredChannelIdRef.current = currentId;
      } else {
        preferredChannelIdRef.current = null;
        setActiveChannelId(null);
        setMessages([]);
        setPlan(null);
      }
      await refreshChannels();
    } catch (err) {
      setError(String(err));
    }
  }, [channels, refreshChannels]);

  const clearStaleChannels = useCallback(async () => {
    try {
      await invoke<number>('clear_stale_channels', { max_age_seconds: 30 * 60 });
      const currentId = activeChannelIdRef.current;
      const current = channels.find((channel) => channel.id === currentId);
      const staleThreshold = 30 * 60;
      const nowSeconds = Math.floor(Date.now() / 1000);
      const updatedAt = current?.updated_at ?? 0;
      const normalizedUpdatedAt = updatedAt > 1_000_000_000_000 ? Math.floor(updatedAt / 1000) : updatedAt;
      const keepCurrent = current?.status === 'active' && currentId && nowSeconds - normalizedUpdatedAt < staleThreshold;
      if (keepCurrent) {
        preferredChannelIdRef.current = currentId;
      } else {
        preferredChannelIdRef.current = null;
        setActiveChannelId(null);
        setMessages([]);
        setPlan(null);
      }
      await refreshChannels();
    } catch (err) {
      setError(String(err));
    }
  }, [channels, refreshChannels]);

  useEffect(() => {
    activeChannelIdRef.current = activeChannelId;
  }, [activeChannelId]);

  useEffect(() => {
    void refreshChannels();
    const poll = setInterval(() => {
      void refreshChannels();
    }, 3000);
    return () => clearInterval(poll);
  }, [refreshChannels]);

  useEffect(() => {
    if (!activeChannelId) {
      setMessages([]);
      setPlan(null);
      return;
    }
    void openChannel(activeChannelId);
  }, [activeChannelId, openChannel]);

  useEffect(() => {
    if (!activeChannelId) {
      setIsCompleted(false);
      return;
    }
    const current = channels.find((channel) => channel.id === activeChannelId);
    setIsCompleted(current?.status === 'completed');
  }, [channels, activeChannelId]);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      listen<DelegationPlan>('delegation_plan', ({ payload }) => {
        preferredChannelIdRef.current = payload.channel_id;
        setPlan(payload);
        setActiveChannelId(payload.channel_id);
        setMessages([]);
        setIsCompleted(false);
        void refreshChannels();
      }),
      listen<ChannelMessageRecord>('channel_message', ({ payload }) => {
        setMessages((prev) => {
          if (payload.channel_id !== activeChannelIdRef.current) return prev;
          if (prev.some((msg) => msg.id === payload.id)) return prev;
          return [...prev, payload];
        });
      }),
      listen<{ channel_id: string }>('channel_completed', ({ payload }) => {
        setChannels((prev) => prev.map((channel) => channel.id === payload.channel_id ? { ...channel, status: 'completed' } : channel));
        if (payload.channel_id === activeChannelIdRef.current) {
          setIsCompleted(true);
          setCompletionEventCount((prev) => prev + 1);
        }
        void refreshChannels();
      }),
    ]).then((fns) => {
      if (cancelled) {
        fns.forEach((fn) => fn());
      } else {
        unlistens.current.push(...fns);
      }
    });

    return () => {
      cancelled = true;
      unlistens.current.forEach((fn) => fn());
      unlistens.current = [];
    };
  }, [refreshChannels]);

  return {
    channels,
    activeChannelId,
    messages,
    plan,
    isCompleted,
    completionEventCount,
    activeCount,
    error,
    openChannel,
    setActiveChannelId,
    refreshChannels,
    clearCompletedChannels,
    clearStaleChannels,
  };
}
