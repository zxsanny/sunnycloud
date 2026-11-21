import type { Account, AdminHealth } from '$lib/api';

export const TABS = ['users', 'invites', 'quotas', 'health'] as const;
export type Tab = (typeof TABS)[number];

export const TAB_LABELS: Record<Tab, string> = {
  users: 'Users',
  invites: 'Invites',
  quotas: 'Quotas',
  health: 'Health',
};

/** The tab a `?tab=` value names; a missing or unknown value opens Users. */
export function tabOf(value: string | null): Tab {
  return TABS.find((tab) => tab === value) ?? 'users';
}

/** Navigation and admin routes are shown to the admin role only; the server still answers 403 to anyone else. */
export function isAdmin(account: Account | null | undefined): boolean {
  return account?.role === 'admin';
}

export type QuotaUnit = 'GB' | 'TB';

const UNIT_DIGITS: Record<QuotaUnit, number> = { GB: 9, TB: 12 };

export type LimitParse = { ok: true; bytes: number } | { ok: false; reason: string };

/**
 * A typed quota in decimal units (1 GB = 10^9 bytes, as everywhere in the app) as whole bytes.
 * The digits are split instead of multiplied so `0.1` GB is exactly 100000000.
 */
export function limitBytes(text: string, unit: QuotaUnit): LimitParse {
  const typed = text.trim();
  const match = /^(\d+)(?:\.(\d+))?$/.exec(typed);
  if (match === null) return { ok: false, reason: 'Enter a number, such as 250 or 1.5.' };
  const digits = UNIT_DIGITS[unit];
  const fraction = match[2] ?? '';
  if (fraction.length > digits) {
    return {
      ok: false,
      reason: `A limit must be a whole number of bytes: use at most ${digits} decimals in ${unit}.`,
    };
  }
  const bytes = BigInt(match[1] + fraction.padEnd(digits, '0'));
  if (bytes > BigInt(Number.MAX_SAFE_INTEGER)) {
    return { ok: false, reason: 'This limit is too large.' };
  }
  return { ok: true, bytes: Number(bytes) };
}

export type HealthState = 'ok' | 'degraded' | 'down';

export interface Readiness {
  state: HealthState;
  /** The checks that are not ok, worst first, in the server's order within one status. */
  failing: string[];
}

const SEVERITY: Record<HealthState, number> = { ok: 0, degraded: 1, down: 2 };

/** The worst status of the checks wins; an empty list reports nothing wrong. */
export function readiness(health: AdminHealth): Readiness {
  let state: HealthState = 'ok';
  for (const item of health.items) {
    if (SEVERITY[item.status] > SEVERITY[state]) state = item.status;
  }
  const failing = health.items
    .filter((item) => item.status !== 'ok')
    .sort((a, b) => SEVERITY[b.status] - SEVERITY[a.status])
    .map((item) => item.name);
  return { state, failing };
}

export type InviteStatus = 'pending' | 'used' | 'expired';

/** The list has no revoked marker: a revoked invite is not told apart here (see the task report). */
export function inviteStatus(
  invite: { redeemed_at?: string | null; expires_at: string },
  now: number = Date.now(),
): InviteStatus {
  if (invite.redeemed_at) return 'used';
  const expires = Date.parse(invite.expires_at);
  return !Number.isNaN(expires) && expires <= now ? 'expired' : 'pending';
}
