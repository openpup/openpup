import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';
import { formatMonthDayTime } from '../utils/locale';

type PlatformKey = 'telegram' | 'discord' | 'slack';
type PlatformStatus = 'unconfigured' | 'connecting' | 'connected' | 'error';

interface TelegramConfig {
  bot_token: string;
  owner_user_id: string;
  allowed_chats: string[];
  proxy_url?: string | null;
}

interface DiscordConfig {
  bot_token: string;
  owner_user_id: string;
  allowed_channels: string[];
  proxy_url?: string | null;
}

interface SlackConfig {
  bot_token: string;
  app_token: string;
  owner_user_id: string;
  allowed_channels: string[];
  proxy_url?: string | null;
}

interface BridgeConfig {
  telegram?: TelegramConfig | null;
  discord?: DiscordConfig | null;
  slack?: SlackConfig | null;
}

interface BridgeConnectionStatus {
  platform: PlatformKey;
  status: PlatformStatus;
  connected: boolean;
  last_seen?: number | null;
  error_msg?: string | null;
}

interface FormState {
  telegramBotToken: string;
  telegramOwnerUserId: string;
  telegramAllowedChats: string;
  telegramProxyUrl: string;
  discordBotToken: string;
  discordOwnerUserId: string;
  discordAllowedChannels: string;
  discordProxyUrl: string;
  slackBotToken: string;
  slackAppToken: string;
  slackOwnerUserId: string;
  slackAllowedChannels: string;
  slackProxyUrl: string;
}

const inputStyle: React.CSSProperties = {
  borderRadius: 10,
  background: 'var(--color-background-primary)',
  border: '0.5px solid var(--color-border-secondary)',
  padding: '9px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
  width: '100%',
};

const cardStyle: React.CSSProperties = {
  borderRadius: 16,
  border: '0.5px solid var(--color-border-tertiary)',
  background: 'color-mix(in srgb, var(--color-background-primary) 88%, var(--color-background-secondary) 12%)',
  padding: 16,
};

const labelStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: 'var(--color-text-tertiary)',
  textTransform: 'uppercase',
  letterSpacing: '0.05em',
};

function listToText(values: string[] | undefined): string {
  return (values ?? []).join(', ');
}

function textToList(value: string): string[] {
  return value.split(',').map((item) => item.trim()).filter(Boolean);
}

function normalizeOptional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function formatTime(ts: number | null | undefined, lang: 'zh' | 'en'): string {
  if (!ts) return t('bridge_no_activity', lang);
  return formatMonthDayTime(ts, lang);
}

function statusMeta(status: BridgeConnectionStatus | undefined, lang: 'zh' | 'en') {
  switch (status?.status) {
    case 'connected':
      return {
        label: t('bridge_status_connected', lang),
        dot: '#1D9E75',
        text: 'var(--color-text-success)',
        background: 'var(--color-background-success)',
      };
    case 'connecting':
      return {
        label: t('bridge_status_connecting', lang),
        dot: '#BA7517',
        text: 'var(--color-text-warning)',
        background: 'var(--color-background-warning)',
      };
    case 'error':
      return {
        label: t('bridge_status_error', lang),
        dot: '#DB4C40',
        text: 'var(--color-text-danger)',
        background: 'var(--color-background-danger)',
      };
    default:
      return {
        label: t('bridge_status_unconfigured', lang),
        dot: 'var(--color-text-tertiary)',
        text: 'var(--color-text-tertiary)',
        background: 'var(--color-background-secondary)',
      };
  }
}

const BridgeField: React.FC<{
  label: string;
  value: string;
  placeholder: string;
  type?: string;
  onChange: (value: string) => void;
}> = ({ label, value, placeholder, type = 'text', onChange }) => (
  <label style={{ display: 'grid', gap: 6 }}>
    <span style={labelStyle}>{label}</span>
    <input
      type={type}
      style={inputStyle}
      placeholder={placeholder}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  </label>
);

const StatusPill: React.FC<{ status?: BridgeConnectionStatus; lang: 'zh' | 'en' }> = ({ status, lang }) => {
  const meta = statusMeta(status, lang);
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        padding: '5px 10px',
        borderRadius: 999,
        fontSize: 11,
        fontWeight: 700,
        color: meta.text,
        background: meta.background,
      }}
    >
      <span style={{ width: 8, height: 8, borderRadius: '50%', background: meta.dot, flexShrink: 0 }} />
      {meta.label}
    </span>
  );
};

export const BridgeSettings: React.FC = () => {
  const { lang } = useLang();
  const [form, setForm] = useState<FormState>({
    telegramBotToken: '',
    telegramOwnerUserId: '',
    telegramAllowedChats: '',
    telegramProxyUrl: '',
    discordBotToken: '',
    discordOwnerUserId: '',
    discordAllowedChannels: '',
    discordProxyUrl: '',
    slackBotToken: '',
    slackAppToken: '',
    slackOwnerUserId: '',
    slackAllowedChannels: '',
    slackProxyUrl: '',
  });
  const [statuses, setStatuses] = useState<BridgeConnectionStatus[]>([]);
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadAll = async () => {
    try {
      const [cfg, statusList] = await Promise.all([
        invoke<BridgeConfig>('get_bridge_config'),
        invoke<BridgeConnectionStatus[]>('get_bridge_status'),
      ]);
      setForm({
        telegramBotToken: cfg.telegram?.bot_token ?? '',
        telegramOwnerUserId: cfg.telegram?.owner_user_id ?? '',
        telegramAllowedChats: listToText(cfg.telegram?.allowed_chats),
        telegramProxyUrl: cfg.telegram?.proxy_url ?? '',
        discordBotToken: cfg.discord?.bot_token ?? '',
        discordOwnerUserId: cfg.discord?.owner_user_id ?? '',
        discordAllowedChannels: listToText(cfg.discord?.allowed_channels),
        discordProxyUrl: cfg.discord?.proxy_url ?? '',
        slackBotToken: cfg.slack?.bot_token ?? '',
        slackAppToken: cfg.slack?.app_token ?? '',
        slackOwnerUserId: cfg.slack?.owner_user_id ?? '',
        slackAllowedChannels: listToText(cfg.slack?.allowed_channels),
        slackProxyUrl: cfg.slack?.proxy_url ?? '',
      });
      setStatuses(statusList);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshStatus = async () => {
    setRefreshing(true);
    try {
      setStatuses(await invoke<BridgeConnectionStatus[]>('get_bridge_status'));
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setRefreshing(false);
    }
  };

  useEffect(() => {
    void loadAll();
    const timer = window.setInterval(() => {
      void refreshStatus();
    }, 5000);
    return () => window.clearInterval(timer);
  }, []);

  const statusMap = useMemo(
    () => Object.fromEntries(statuses.map((item) => [item.platform, item])) as Partial<Record<PlatformKey, BridgeConnectionStatus>>,
    [statuses],
  );

  const save = async () => {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      await invoke('save_bridge_config', {
        config: {
          telegram: form.telegramBotToken.trim() ? {
            bot_token: form.telegramBotToken.trim(),
            owner_user_id: form.telegramOwnerUserId.trim(),
            allowed_chats: textToList(form.telegramAllowedChats),
            proxy_url: normalizeOptional(form.telegramProxyUrl),
          } : null,
          discord: form.discordBotToken.trim() ? {
            bot_token: form.discordBotToken.trim(),
            owner_user_id: form.discordOwnerUserId.trim(),
            allowed_channels: textToList(form.discordAllowedChannels),
            proxy_url: normalizeOptional(form.discordProxyUrl),
          } : null,
          slack: form.slackBotToken.trim() ? {
            bot_token: form.slackBotToken.trim(),
            app_token: form.slackAppToken.trim(),
            owner_user_id: form.slackOwnerUserId.trim(),
            allowed_channels: textToList(form.slackAllowedChannels),
            proxy_url: normalizeOptional(form.slackProxyUrl),
          } : null,
        },
      });
      setMessage(t('bridge_saved', lang));
      await loadAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const renderBridgeCard = (
    platform: PlatformKey,
    title: string,
    description: string,
    proxyValue: string,
    fields: React.ReactNode,
  ) => {
    const status = statusMap[platform];
    const meta = statusMeta(status, lang);
    return (
      <div style={cardStyle}>
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 14 }}>
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--color-text-primary)', marginBottom: 4 }}>{title}</div>
            <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>{description}</div>
          </div>
          <StatusPill status={status} lang={lang} />
        </div>

        <div style={{ display: 'grid', gap: 12 }}>
          {fields}

          <BridgeField
            label="Proxy"
            value={proxyValue}
            placeholder="http://127.0.0.1:7890 or socks5://127.0.0.1:1080"
            onChange={(value) => setForm((prev) => ({ ...prev, [`${platform}ProxyUrl`]: value } as FormState))}
          />

          <div
            style={{
              display: 'grid',
              gap: 4,
              padding: '10px 12px',
              borderRadius: 12,
              background: 'var(--color-background-secondary)',
            }}
          >
            <div style={{ fontSize: 12, fontWeight: 600, color: meta.text }}>{t('bridge_connection_status', lang)}: {meta.label}</div>
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
              {status?.error_msg
                ? status.error_msg
                : status?.connected
                  ? `${t('bridge_recent_activity', lang)} ${formatTime(status.last_seen, lang)}`
                  : proxyValue.trim()
                    ? t('bridge_proxy_waiting', lang)
                    : t('bridge_direct_mode', lang)}
            </div>
          </div>
        </div>
      </div>
    );
  };

  return (
    <section>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 14 }}>
        <div>
          <h2 style={{ fontSize: '14px', fontWeight: 600, color: 'var(--color-text-primary)', marginBottom: 4 }}>{t('bridge_title', lang)}</h2>
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
            {t('bridge_description', lang)}
          </p>
        </div>
        <button
          onClick={() => void refreshStatus()}
          disabled={refreshing}
          style={{
            padding: '6px 10px',
            borderRadius: 999,
            border: '0.5px solid var(--color-border-secondary)',
            background: 'var(--color-background-primary)',
            color: 'var(--color-text-secondary)',
            fontSize: 12,
            cursor: refreshing ? 'not-allowed' : 'pointer',
            opacity: refreshing ? 0.65 : 1,
          }}
        >
          {refreshing ? t('bridge_refreshing', lang) : t('bridge_refresh', lang)}
        </button>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 12, marginBottom: 16 }}>
        {(['telegram', 'discord', 'slack'] as PlatformKey[]).map((platform) => {
          const status = statusMap[platform];
          return (
            <div key={platform} style={{ ...cardStyle, padding: 14 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 6 }}>
                <span style={{ width: 9, height: 9, borderRadius: '50%', background: statusMeta(status, lang).dot, flexShrink: 0 }} />
                <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)', textTransform: 'capitalize' }}>{platform}</span>
              </div>
              <div style={{ fontSize: 12, color: statusMeta(status, lang).text, fontWeight: 600 }}>{statusMeta(status, lang).label}</div>
              <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', marginTop: 4 }}>
                {status?.connected ? `${t('bridge_recent_activity', lang)} ${formatTime(status.last_seen, lang)}` : t('bridge_refresh_auto', lang)}
              </div>
            </div>
          );
        })}
      </div>

      <div style={{ display: 'grid', gap: 14 }}>
        {renderBridgeCard(
          'telegram',
          'Telegram',
          t('bridge_telegram_desc', lang),
          form.telegramProxyUrl,
          <>
            <BridgeField
              label="Bot Token"
              value={form.telegramBotToken}
              placeholder="Telegram bot token"
              onChange={(value) => setForm((prev) => ({ ...prev, telegramBotToken: value }))}
            />
            <BridgeField
              label="Owner User ID"
              value={form.telegramOwnerUserId}
              placeholder="Telegram owner user id"
              onChange={(value) => setForm((prev) => ({ ...prev, telegramOwnerUserId: value }))}
            />
            <BridgeField
              label="Allowed Chats"
              value={form.telegramAllowedChats}
              placeholder="chat_id_1, chat_id_2"
              onChange={(value) => setForm((prev) => ({ ...prev, telegramAllowedChats: value }))}
            />
          </>,
        )}

        {renderBridgeCard(
          'discord',
          'Discord',
          t('bridge_discord_desc', lang),
          form.discordProxyUrl,
          <>
            <BridgeField
              label="Bot Token"
              value={form.discordBotToken}
              placeholder="Discord bot token"
              onChange={(value) => setForm((prev) => ({ ...prev, discordBotToken: value }))}
            />
            <BridgeField
              label="Owner User ID"
              value={form.discordOwnerUserId}
              placeholder="Discord owner user id"
              onChange={(value) => setForm((prev) => ({ ...prev, discordOwnerUserId: value }))}
            />
            <BridgeField
              label="Allowed Channels"
              value={form.discordAllowedChannels}
              placeholder="channel_id_1, channel_id_2"
              onChange={(value) => setForm((prev) => ({ ...prev, discordAllowedChannels: value }))}
            />
          </>,
        )}

        {renderBridgeCard(
          'slack',
          'Slack',
          t('bridge_slack_desc', lang),
          form.slackProxyUrl,
          <>
            <BridgeField
              label="Bot Token"
              value={form.slackBotToken}
              placeholder="Slack bot token"
              onChange={(value) => setForm((prev) => ({ ...prev, slackBotToken: value }))}
            />
            <BridgeField
              label="App Token"
              value={form.slackAppToken}
              placeholder="Slack app token"
              onChange={(value) => setForm((prev) => ({ ...prev, slackAppToken: value }))}
            />
            <BridgeField
              label="Owner User ID"
              value={form.slackOwnerUserId}
              placeholder="Slack owner user id"
              onChange={(value) => setForm((prev) => ({ ...prev, slackOwnerUserId: value }))}
            />
            <BridgeField
              label="Allowed Channels"
              value={form.slackAllowedChannels}
              placeholder="channel_id_1, channel_id_2"
              onChange={(value) => setForm((prev) => ({ ...prev, slackAllowedChannels: value }))}
            />
          </>,
        )}
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 16 }}>
        <button
          onClick={() => void save()}
          disabled={saving}
          style={{
            padding: '7px 13px',
            borderRadius: 10,
            background: 'var(--color-text-primary)',
            color: 'var(--color-background-primary)',
            border: 'none',
            cursor: saving ? 'not-allowed' : 'pointer',
            opacity: saving ? 0.6 : 1,
            fontSize: 12,
            fontWeight: 600,
          }}
        >
          {saving ? t('bridge_saving', lang) : t('bridge_save', lang)}
        </button>
        {message && <span style={{ fontSize: 12, color: 'var(--color-text-success)' }}>{message}</span>}
      </div>

      {error && <div style={{ fontSize: 12, color: 'var(--color-text-danger)', marginTop: 10 }}>{error}</div>}
    </section>
  );
};
