import type { Lang } from '../i18n';

type RelativeTimeMode = 'date' | 'days';

function toMs(ts: number): number {
  return ts < 1_000_000_000_000 ? ts * 1000 : ts;
}

export function localeForLang(lang: Lang): string {
  return lang === 'zh' ? 'zh-CN' : 'en-US';
}

export function formatDateOnly(ts: number, lang: Lang): string {
  return new Intl.DateTimeFormat(localeForLang(lang), {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
  }).format(new Date(toMs(ts)));
}

export function formatDateTime(ts: number, lang: Lang): string {
  return new Intl.DateTimeFormat(localeForLang(lang), {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(toMs(ts)));
}

export function formatMonthDayTime(ts: number, lang: Lang): string {
  return new Intl.DateTimeFormat(localeForLang(lang), {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(toMs(ts)));
}

export function formatRelativeTime(
  ts: number,
  lang: Lang,
  options: {
    olderThanDay?: RelativeTimeMode;
    includeYesterdayTime?: boolean;
  } = {},
): string {
  const normalized = toMs(ts);
  const now = Date.now();
  const diffMs = Math.max(0, now - normalized);
  const diffMin = Math.floor(diffMs / 60_000);
  const diffHour = Math.floor(diffMs / 3_600_000);
  const diffDay = Math.floor(diffMs / 86_400_000);
  const olderThanDay = options.olderThanDay ?? 'days';
  const includeYesterdayTime = options.includeYesterdayTime ?? false;

  if (diffMin < 1) return lang === 'zh' ? '刚刚' : 'just now';
  if (diffMin < 60) return lang === 'zh' ? `${diffMin} 分钟前` : `${diffMin} min ago`;
  if (diffHour < 24) return lang === 'zh' ? `${diffHour} 小时前` : `${diffHour} hr ago`;

  const date = new Date(normalized);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const isYesterday = date.toDateString() === yesterday.toDateString();

  if (isYesterday && includeYesterdayTime) {
    const time = new Intl.DateTimeFormat(localeForLang(lang), {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(date);
    return lang === 'zh' ? `昨天 ${time}` : `Yesterday ${time}`;
  }

  if (olderThanDay === 'date') {
    return formatMonthDayTime(normalized, lang);
  }

  return lang === 'zh' ? `${diffDay} 天前` : `${diffDay} day${diffDay === 1 ? '' : 's'} ago`;
}
