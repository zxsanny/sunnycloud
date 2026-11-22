import type { ApiError, DeviceList, Quota, SessionList } from '$lib/api';
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

export type RowKind = 'desktop' | 'android' | 'ios' | 'web';

/** One revocable row: a desktop device, a phone session or a browser session. */
export interface DeviceRow {
  /** Unique across kinds: a device id and a session id are different things. */
  key: string;
  /** The id the revoke call takes: a device id for `desktop`, a session id for the others. */
  id: string;
  kind: RowKind;
  name: string;
  /** The line under the name, such as `Desktop app · Linux · added 4 Mar 2026`. */
  detail: string;
  lastSeen: string;
  /** This browser's own session: it has no Revoke, Sign out is the way to end it. */
  current: boolean;
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

type SessionItem = SessionList['items'][number];

const APP_NAMES = { android: 'Android app', ios: 'iOS app' } as const;

/**
 * One list of desktop devices followed by phone sessions (`client_kind` android or ios), each group in the
 * order the server gave it. Web sessions belong to `browserRows`.
 */
export function deviceRows(
  devices: DeviceList,
  sessions: SessionList | null = null,
  now: number = Date.now(),
): DeviceRow[] {
  const desktop = devices.items.map((device): DeviceRow => ({
    key: `device:${device.id}`,
    id: device.id,
    kind: 'desktop',
    name: device.name,
    detail: `Desktop app · ${device.platform} · added ${formatDate(device.created_at)}`,
    lastSeen: lastSeenText(device.last_seen_at, now),
    current: false,
  }));
  const phones = (sessions?.items ?? [])
    .filter(
      (item): item is SessionItem & { client_kind: 'android' | 'ios' } =>
        item.client_kind !== 'web',
    )
    .map((item): DeviceRow => ({
      key: `session:${item.id}`,
      id: item.id,
      kind: item.client_kind,
      name: item.label,
      detail: `${APP_NAMES[item.client_kind]} · signed in ${formatDate(item.created_at)}`,
      lastSeen: lastSeenText(item.last_seen_at, now),
      current: false,
    }));
  return [...desktop, ...phones];
}

/** The web sessions; the address is shown only when the server recorded one. */
export function browserRows(sessions: SessionList, now: number = Date.now()): DeviceRow[] {
  return sessions.items
    .filter((item) => item.client_kind === 'web')
    .map((item): DeviceRow => ({
      key: `session:${item.id}`,
      id: item.id,
      kind: 'web',
      name: item.label,
      detail: `Signed in ${formatDate(item.created_at)}${item.ip ? ` · ${item.ip}` : ''}`,
      lastSeen: item.current ? 'Active now' : lastSeenText(item.last_seen_at, now),
      current: item.current,
    }));
}

export interface LinkOutcome {
  variant: 'success' | 'danger' | 'warning';
  title: string;
  text: string;
}

/**
 * The notice for `/account?linked=1` or `/account?error=<reason>`, the server's redirect ending a provider link.
 * Nothing else in the query is shown: an unknown reason gets the neutral text, never the reason itself.
 */
export function linkOutcome(query: URLSearchParams): LinkOutcome | null {
  const reason = query.get('error');
  if (reason !== null && reason !== '') {
    const nothing = 'Nothing was linked.';
    if (reason === 'identity_taken') {
      return {
        variant: 'danger',
        title: 'That sign-in belongs to another account.',
        text: `${nothing} Each provider identity can be linked to one account only.`,
      };
    }
    if (reason === 'invalid_request') {
      return {
        variant: 'danger',
        title: 'The link request was not recognised.',
        text: `${nothing} It may have expired. Start linking again.`,
      };
    }
    if (reason === 'invitation_required') {
      return {
        variant: 'danger',
        title: 'That provider account is not invited.',
        text: `${nothing} Ask your administrator for an invite, then try again.`,
      };
    }
    if (reason === 'account_not_active') {
      return {
        variant: 'warning',
        title: 'This account is deactivated.',
        text: `${nothing} Ask your administrator to turn it back on.`,
      };
    }
    return {
      variant: 'danger',
      title: 'The provider could not be linked.',
      text: `${nothing} Try again. If it keeps happening, ask your administrator.`,
    };
  }
  if (query.get('linked') === '1') {
    return {
      variant: 'success',
      title: 'Provider linked.',
      text: 'You can now sign in with it as well.',
    };
  }
  return null;
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
