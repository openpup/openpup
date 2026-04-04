import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';
import { formatMonthDayTime } from '../utils/locale';

type PlatformKey = 'telegram' | 'discord' | 'slack' | 'weixin' | 'qqbot';
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

interface WeixinConfig {
  base_url: string;
  access_token: string;
  owner_user_id: string;
  allowed_users: string[];
  proxy_url?: string | null;
  account_id?: string | null;
  route_tag?: string | null;
}

interface QQBotConfig {
  app_id: string;
  client_secret: string;
  owner_user_id: string;
  allowed_chats: string[];
}

interface BridgeConfig {
  telegram?: TelegramConfig | null;
  discord?: DiscordConfig | null;
  slack?: SlackConfig | null;
  weixin?: WeixinConfig | null;
  qqbot?: QQBotConfig | null;
}

interface BridgeConnectionStatus {
  platform: PlatformKey;
  status: PlatformStatus;
  connected: boolean;
  last_seen?: number | null;
  error_msg?: string | null;
}

interface StoredWeixinAccount {
  account_id: string;
  base_url?: string | null;
  user_id?: string | null;
  configured: boolean;
  saved_at?: string | null;
}

interface WeixinQrStartResult {
  session_key: string;
  qrcode_url?: string | null;
  message: string;
}

interface WeixinQrWaitResult {
  connected: boolean;
  status: string;
  session_key: string;
  qrcode_url?: string | null;
  account_id?: string | null;
  base_url?: string | null;
  user_id?: string | null;
  message: string;
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
  weixinBaseUrl: string;
  weixinAccessToken: string;
  weixinOwnerUserId: string;
  weixinAllowedUsers: string;
  weixinProxyUrl: string;
  weixinAccountId: string;
  weixinRouteTag: string;
  qqbotAppId: string;
  qqbotClientSecret: string;
  qqbotOwnerUserId: string;
  qqbotAllowedChats: string;
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

const DEFAULT_WEIXIN_BASE_URL = 'https://ilinkai.weixin.qq.com';

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

function formatWeixinAccountTitle(account: StoredWeixinAccount, activeAccountId: string, lang: 'zh' | 'en'): string {
  const primary = account.user_id?.trim() || account.account_id;
  if (account.account_id === activeAccountId && account.user_id?.trim()) {
    return `${primary} · ${lang === 'zh' ? '当前账号' : 'Current'}`;
  }
  return primary;
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
  readOnly?: boolean;
  onChange: (value: string) => void;
}> = ({ label, value, placeholder, type = 'text', readOnly = false, onChange }) => (
  <label style={{ display: 'grid', gap: 6 }}>
    <span style={labelStyle}>{label}</span>
    <input
      type={type}
      style={inputStyle}
      placeholder={placeholder}
      value={value}
      readOnly={readOnly}
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
    weixinBaseUrl: DEFAULT_WEIXIN_BASE_URL,
    weixinAccessToken: '',
    weixinOwnerUserId: '',
    weixinAllowedUsers: '',
    weixinProxyUrl: '',
    weixinAccountId: '',
    weixinRouteTag: '',
    qqbotAppId: '',
    qqbotClientSecret: '',
    qqbotOwnerUserId: '',
    qqbotAllowedChats: '',
  });
  const [statuses, setStatuses] = useState<BridgeConnectionStatus[]>([]);
  const [weixinAccounts, setWeixinAccounts] = useState<StoredWeixinAccount[]>([]);
  const [weixinLoginSessionKey, setWeixinLoginSessionKey] = useState('');
  const [weixinQrUrl, setWeixinQrUrl] = useState('');
  const [weixinQrStatus, setWeixinQrStatus] = useState('');
  const [weixinQrBusy, setWeixinQrBusy] = useState(false);
  const [weixinWaitBusy, setWeixinWaitBusy] = useState(false);
  const [weixinActivateBusy, setWeixinActivateBusy] = useState<string | null>(null);
  const [weixinAdvancedOpen, setWeixinAdvancedOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadAll = async () => {
    try {
      const [cfg, statusList, accountList] = await Promise.all([
        invoke<BridgeConfig>('get_bridge_config'),
        invoke<BridgeConnectionStatus[]>('get_bridge_status'),
        invoke<StoredWeixinAccount[]>('list_weixin_accounts'),
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
        weixinBaseUrl: cfg.weixin?.base_url ?? DEFAULT_WEIXIN_BASE_URL,
        weixinAccessToken: cfg.weixin?.access_token ?? '',
        weixinOwnerUserId: cfg.weixin?.owner_user_id ?? '',
        weixinAllowedUsers: listToText(cfg.weixin?.allowed_users),
        weixinProxyUrl: cfg.weixin?.proxy_url ?? '',
        weixinAccountId: cfg.weixin?.account_id ?? '',
        weixinRouteTag: cfg.weixin?.route_tag ?? '',
        qqbotAppId: cfg.qqbot?.app_id ?? '',
        qqbotClientSecret: cfg.qqbot?.client_secret ?? '',
        qqbotOwnerUserId: cfg.qqbot?.owner_user_id ?? '',
        qqbotAllowedChats: listToText(cfg.qqbot?.allowed_chats),
      });
      setStatuses(statusList);
      setWeixinAccounts(accountList);
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

  const startWeixinQrLogin = async () => {
    if (!form.weixinBaseUrl.trim()) {
      setError(t('bridge_weixin_base_required', lang));
      return;
    }
    setWeixinQrBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await invoke<WeixinQrStartResult>('start_weixin_qr_login', {
        baseUrl: form.weixinBaseUrl.trim(),
        proxyUrl: normalizeOptional(form.weixinProxyUrl),
        routeTag: normalizeOptional(form.weixinRouteTag),
        accountId: normalizeOptional(form.weixinAccountId),
      });
      setWeixinLoginSessionKey(result.session_key);
      setWeixinQrUrl(result.qrcode_url ?? '');
      setWeixinQrStatus('wait');
      setMessage(result.message);
    } catch (err) {
      setError(String(err));
    } finally {
      setWeixinQrBusy(false);
    }
  };

  const pollWeixinQrLogin = async () => {
    if (!weixinLoginSessionKey || weixinWaitBusy) return;
    setWeixinWaitBusy(true);
    try {
      const result = await invoke<WeixinQrWaitResult>('wait_weixin_qr_login', {
        baseUrl: form.weixinBaseUrl.trim(),
        proxyUrl: normalizeOptional(form.weixinProxyUrl),
        routeTag: normalizeOptional(form.weixinRouteTag),
        sessionKey: weixinLoginSessionKey,
        timeoutMs: 35_000,
      });
      setWeixinQrStatus(result.status);
      if (result.qrcode_url) {
        setWeixinQrUrl(result.qrcode_url);
      }
      if (result.connected) {
        setWeixinLoginSessionKey('');
        setWeixinQrUrl('');
        setMessage(result.message);
        await loadAll();
        return;
      }
      if (result.account_id) {
        setForm((prev) => ({ ...prev, weixinAccountId: result.account_id ?? prev.weixinAccountId }));
      }
      if (result.base_url) {
        setForm((prev) => ({ ...prev, weixinBaseUrl: result.base_url ?? prev.weixinBaseUrl }));
      }
      if (result.user_id) {
        setForm((prev) => {
          const allowed = textToList(prev.weixinAllowedUsers);
          if (!allowed.includes(result.user_id!)) allowed.push(result.user_id!);
          return {
            ...prev,
            weixinOwnerUserId: result.user_id ?? prev.weixinOwnerUserId,
            weixinAllowedUsers: allowed.join(', '),
          };
        });
      }
      setMessage(result.message);
    } catch (err) {
      setError(String(err));
    } finally {
      setWeixinWaitBusy(false);
    }
  };

  const cancelWeixinQrLogin = async () => {
    if (!weixinLoginSessionKey) return;
    try {
      await invoke('cancel_weixin_qr_login', { sessionKey: weixinLoginSessionKey });
      setWeixinLoginSessionKey('');
      setWeixinQrUrl('');
      setWeixinQrStatus('');
    } catch (err) {
      setError(String(err));
    }
  };

  const activateWeixinAccount = async (accountId: string) => {
    setWeixinActivateBusy(accountId);
    setError(null);
    try {
      await invoke<BridgeConfig>('activate_weixin_account', { accountId });
      await loadAll();
      await refreshStatus();
      setMessage(t('bridge_weixin_account_activated', lang));
    } catch (err) {
      setError(String(err));
    } finally {
      setWeixinActivateBusy(null);
    }
  };

  useEffect(() => {
    void loadAll();
    const timer = window.setInterval(() => {
      void refreshStatus();
    }, 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!weixinLoginSessionKey) return;
    const timer = window.setInterval(() => {
      void pollWeixinQrLogin();
    }, 2500);
    return () => window.clearInterval(timer);
  }, [weixinLoginSessionKey, weixinWaitBusy, form.weixinBaseUrl, form.weixinProxyUrl, form.weixinRouteTag]);

  const statusMap = useMemo(
    () => Object.fromEntries(statuses.map((item) => [item.platform, item])) as Partial<Record<PlatformKey, BridgeConnectionStatus>>,
    [statuses],
  );
  const activeWeixinAccount = useMemo(
    () => weixinAccounts.find((account) => account.account_id === form.weixinAccountId.trim()) ?? null,
    [weixinAccounts, form.weixinAccountId],
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
          weixin: form.weixinBaseUrl.trim() && form.weixinAccessToken.trim() ? {
            base_url: form.weixinBaseUrl.trim(),
            access_token: form.weixinAccessToken.trim(),
            owner_user_id: form.weixinOwnerUserId.trim(),
            allowed_users: textToList(form.weixinAllowedUsers),
            proxy_url: normalizeOptional(form.weixinProxyUrl),
            account_id: normalizeOptional(form.weixinAccountId),
            route_tag: normalizeOptional(form.weixinRouteTag),
          } : null,
          qqbot: form.qqbotAppId.trim() ? {
            app_id: form.qqbotAppId.trim(),
            client_secret: form.qqbotClientSecret.trim(),
            owner_user_id: form.qqbotOwnerUserId.trim(),
            allowed_chats: textToList(form.qqbotAllowedChats),
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
    showProxy = true,
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

          {showProxy && (
            <BridgeField
              label="Proxy"
              value={proxyValue}
              placeholder="http://127.0.0.1:7890 or socks5://127.0.0.1:1080"
              onChange={(value) => setForm((prev) => ({ ...prev, [`${platform}ProxyUrl`]: value } as FormState))}
            />
          )}

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
                  : showProxy && proxyValue.trim()
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

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12, marginBottom: 16 }}>
        {(['telegram', 'discord', 'slack', 'weixin', 'qqbot'] as PlatformKey[]).map((platform) => {
          const status = statusMap[platform];
          return (
            <div key={platform} style={{ ...cardStyle, padding: 14 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 6 }}>
                <span style={{ width: 9, height: 9, borderRadius: '50%', background: statusMeta(status, lang).dot, flexShrink: 0 }} />
                <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)', textTransform: 'capitalize' }}>{platform === 'qqbot' ? 'QQ Bot' : platform}</span>
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

        {renderBridgeCard(
          'weixin',
          'Weixin',
          t('bridge_weixin_desc', lang),
          form.weixinProxyUrl,
          <>
            <BridgeField
              label={lang === 'zh' ? '微信账号' : 'Weixin Account'}
              value={activeWeixinAccount?.user_id || form.weixinOwnerUserId || ''}
              placeholder={lang === 'zh' ? '扫码后自动绑定' : 'Bound automatically after scan'}
              readOnly
              onChange={() => {}}
            />
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', marginTop: -4 }}>
              {activeWeixinAccount
                ? [activeWeixinAccount.base_url, lang === 'zh' ? '已绑定' : 'Linked'].filter(Boolean).join(' · ')
                : (lang === 'zh' ? '直接扫码登录，默认只允许扫码者本人使用。' : 'Scan to sign in. By default only the scanning user is allowed.')}
            </div>
            <div
              style={{
                display: 'grid',
                gap: 10,
                padding: '12px',
                borderRadius: 12,
                border: '0.5px solid var(--color-border-secondary)',
                background: 'var(--color-background-secondary)',
              }}
            >
              <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                {t('bridge_weixin_qr_title', lang)}
              </div>
              <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                {lang === 'zh'
                  ? '填写网关地址后直接扫码登录即可。访问令牌、内部账号标识等细节会自动处理，不需要手动填写。'
                  : 'Enter the gateway URL and scan to sign in. Tokens and internal account identifiers are handled automatically.'}
              </div>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <button
                  onClick={() => void startWeixinQrLogin()}
                  disabled={weixinQrBusy || !form.weixinBaseUrl.trim()}
                  style={{
                    padding: '7px 12px',
                    borderRadius: 10,
                    border: 'none',
                    background: 'var(--color-text-primary)',
                    color: 'var(--color-background-primary)',
                    fontSize: 12,
                    fontWeight: 600,
                    cursor: weixinQrBusy ? 'not-allowed' : 'pointer',
                    opacity: weixinQrBusy ? 0.65 : 1,
                  }}
                >
                  {weixinQrBusy
                    ? t('bridge_weixin_qr_starting', lang)
                    : (activeWeixinAccount ? (lang === 'zh' ? '重新扫码登录' : 'Scan Again') : t('bridge_weixin_qr_start', lang))}
                </button>
                {weixinLoginSessionKey && (
                  <button
                    onClick={() => void cancelWeixinQrLogin()}
                    style={{
                      padding: '7px 12px',
                      borderRadius: 10,
                      border: '0.5px solid var(--color-border-secondary)',
                      background: 'var(--color-background-primary)',
                      color: 'var(--color-text-secondary)',
                      fontSize: 12,
                      fontWeight: 600,
                      cursor: 'pointer',
                    }}
                  >
                    {t('bridge_weixin_qr_cancel', lang)}
                  </button>
                )}
              </div>
              {weixinLoginSessionKey && (
                <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                  {t('bridge_weixin_qr_status', lang)}: {weixinQrStatus || 'wait'}
                </div>
              )}
              {weixinQrUrl && (
                <div style={{ display: 'grid', gap: 8 }}>
                  <img
                    src={weixinQrUrl}
                    alt="Weixin QR"
                    style={{ width: 180, height: 180, objectFit: 'contain', borderRadius: 12, background: 'white', padding: 10 }}
                  />
                </div>
              )}
            </div>
            <div style={{ display: 'grid', gap: 8 }}>
              <button
                onClick={() => setWeixinAdvancedOpen((prev) => !prev)}
                style={{
                  padding: '7px 12px',
                  borderRadius: 10,
                  border: '0.5px solid var(--color-border-secondary)',
                  background: 'var(--color-background-primary)',
                  color: 'var(--color-text-secondary)',
                  fontSize: 12,
                  fontWeight: 600,
                  cursor: 'pointer',
                  justifySelf: 'start',
                }}
              >
                {weixinAdvancedOpen
                  ? (lang === 'zh' ? '收起高级设置' : 'Hide Advanced Settings')
                  : (lang === 'zh' ? '高级设置' : 'Advanced Settings')}
              </button>
              {weixinAdvancedOpen && (
                <div
                  style={{
                    display: 'grid',
                    gap: 12,
                    padding: '12px',
                    borderRadius: 12,
                    border: '0.5px solid var(--color-border-secondary)',
                    background: 'var(--color-background-secondary)',
                  }}
                >
                  <BridgeField
                    label={lang === 'zh' ? '网关地址' : 'Gateway URL'}
                    value={form.weixinBaseUrl}
                    placeholder={DEFAULT_WEIXIN_BASE_URL}
                    onChange={(value) => setForm((prev) => ({ ...prev, weixinBaseUrl: value || DEFAULT_WEIXIN_BASE_URL }))}
                  />
                  <BridgeField
                    label="Proxy"
                    value={form.weixinProxyUrl}
                    placeholder="http://127.0.0.1:7890 or socks5://127.0.0.1:1080"
                    onChange={(value) => setForm((prev) => ({ ...prev, weixinProxyUrl: value }))}
                  />
                  <BridgeField
                    label={lang === 'zh' ? 'Owner 用户 ID' : 'Owner User ID'}
                    value={form.weixinOwnerUserId}
                    placeholder="Weixin owner user id"
                    onChange={(value) => setForm((prev) => ({ ...prev, weixinOwnerUserId: value }))}
                  />
                  <BridgeField
                    label={lang === 'zh' ? '允许用户' : 'Allowed Users'}
                    value={form.weixinAllowedUsers}
                    placeholder="user_id_1, user_id_2"
                    onChange={(value) => setForm((prev) => ({ ...prev, weixinAllowedUsers: value }))}
                  />
                </div>
              )}
            </div>
            <div
              style={{
                display: 'grid',
                gap: 10,
                padding: '12px',
                borderRadius: 12,
                border: '0.5px solid var(--color-border-secondary)',
                background: 'var(--color-background-secondary)',
              }}
            >
              <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--color-text-primary)' }}>
                {t('bridge_weixin_saved_accounts', lang)}
              </div>
              {weixinAccounts.length === 0 ? (
                <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                  {t('bridge_weixin_no_accounts', lang)}
                </div>
              ) : (
                <div style={{ display: 'grid', gap: 8 }}>
                  {weixinAccounts.map((account) => (
                    <div
                      key={account.account_id}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 12,
                        padding: '10px 12px',
                        borderRadius: 10,
                        background: 'var(--color-background-primary)',
                        border: '0.5px solid var(--color-border-secondary)',
                      }}
                    >
                      <div style={{ minWidth: 0 }}>
                        <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--color-text-primary)' }}>
                          {formatWeixinAccountTitle(account, form.weixinAccountId.trim(), lang)}
                        </div>
                        <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                          {[
                            account.user_id && account.user_id !== account.account_id ? account.account_id : null,
                            account.base_url,
                            account.account_id === form.weixinAccountId.trim()
                              ? (lang === 'zh' ? '当前生效' : 'Active')
                              : null,
                          ].filter(Boolean).join(' · ') || t('bridge_no_activity', lang)}
                        </div>
                      </div>
                      <button
                        onClick={() => void activateWeixinAccount(account.account_id)}
                        disabled={weixinActivateBusy === account.account_id}
                        style={{
                          padding: '6px 10px',
                          borderRadius: 999,
                          border: '0.5px solid var(--color-border-secondary)',
                          background: 'var(--color-background-primary)',
                          color: 'var(--color-text-secondary)',
                          fontSize: 11,
                          fontWeight: 600,
                          cursor: weixinActivateBusy === account.account_id ? 'not-allowed' : 'pointer',
                          opacity: weixinActivateBusy === account.account_id ? 0.65 : 1,
                        }}
                      >
                        {weixinActivateBusy === account.account_id
                          ? t('bridge_weixin_activating', lang)
                          : t('bridge_weixin_activate', lang)}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </>,
          false,
        )}

        {renderBridgeCard(
          'qqbot',
          'QQ Bot',
          t('bridge_qqbot_desc', lang),
          '',
          <>
            <BridgeField
              label="App ID"
              value={form.qqbotAppId}
              placeholder="QQ Bot App ID"
              onChange={(value) => setForm((prev) => ({ ...prev, qqbotAppId: value }))}
            />
            <BridgeField
              label="Client Secret"
              value={form.qqbotClientSecret}
              placeholder="QQ Bot Client Secret"
              type="password"
              onChange={(value) => setForm((prev) => ({ ...prev, qqbotClientSecret: value }))}
            />
            <BridgeField
              label="Owner User ID"
              value={form.qqbotOwnerUserId}
              placeholder="QQ Bot owner user openid"
              onChange={(value) => setForm((prev) => ({ ...prev, qqbotOwnerUserId: value }))}
            />
            <BridgeField
              label="Allowed Chats"
              value={form.qqbotAllowedChats}
              placeholder="c2c:xxx, group:xxx, channel:xxx"
              onChange={(value) => setForm((prev) => ({ ...prev, qqbotAllowedChats: value }))}
            />
          </>,
          false,
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
