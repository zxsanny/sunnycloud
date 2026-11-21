import type { ApiError, DeviceList, Quota } from '$lib/api';
import { formatBytes, formatDate } from '$lib/format';

export interface QuotaView {
  used: string;
  reserved: string;
  limit: string;
  /** True only when the account has no limit of its own and the installation default applies. */
  serverDefault: boolean;
  percent: number;
}

/** The quota view as text; "server default" is shown only when `limit_bytes` is null. */
export function quotaView(quota: Quota): QuotaView {
  const percent =
    quota.effective_limit > 0
      ? Math.min(100, Math.round((quota.used / quota.effective_limit) * 100))
      : 0;
  return {
    used: formatBytes(quota.used),
    reserved: formatBytes(quota.reserved),
    limit: formatBytes(quota.effective_limit),
    serverDefault: quota.limit_bytes === null,
    percent,
  };
}

export interface DeviceRow {
  id: string;
  name: string;
  platform: string;
  added: string;
  lastSeen: string;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function plural(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? '' : 's'} ago`;
}

/** "2 min ago", "3 days ago"; older than 30 days is the date. A missing or unreadable time is "never". */
export function lastSeenText(iso: string | null | undefined, now: number = Date.now()): string {
  if (!iso) return 'Never seen';
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return 'Never seen';
  const age = Math.max(0, now - at);
  if (age < MINUTE) return 'Last seen just now';
  if (age < HOUR) return `Last seen ${Math.floor(age / MINUTE)} min ago`;
  if (age < DAY) return `Last seen ${plural(Math.floor(age / HOUR), 'hour')}`;
  if (age < 30 * DAY) return `Last seen ${plural(Math.floor(age / DAY), 'day')}`;
  return `Last seen ${formatDate(iso)}`;
}

/** One row per device in the order the server gave them. */
export function deviceRows(list: DeviceList, now: number = Date.now()): DeviceRow[] {
  return list.items.map((device) => ({
    id: device.id,
    name: device.name,
    platform: device.platform,
    added: formatDate(device.created_at),
    lastSeen: lastSeenText(device.last_seen_at, now),
  }));
}

const DEADLINE = new Intl.DateTimeFormat('en-GB', { dateStyle: 'medium', timeStyle: 'short' });

/** `21 Nov 2026, 14:30` from the deadline the server sent; an unreadable value is shown as it came. */
export function deadlineText(iso: string): string {
  const at = new Date(iso);
  return Number.isNaN(at.getTime()) ? iso : DEADLINE.format(at);
}

/** The server's own words for a refusal, never a rewrite: its title, or a plain fallback when it sent none. */
export function errorText(error: ApiError): string {
  if (error.kind === 'network') return 'The server could not be reached.';
  return error.title ?? 'The server refused the request.';
}
