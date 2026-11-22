import { describe, expect, it } from 'vitest';
import {
  inviteStatus,
  isAdmin,
  limitBytes,
  readiness,
  tabOf,
  userChoiceLabel,
  userName,
} from './view';

const account = (role: 'user' | 'admin') =>
  ({ id: 'a', display_name: 'A', role, identities: [], quota: {}, root_node_id: 'r' }) as never;

describe('navigation by role (AC-1)', () => {
  it('shows the admin group to the admin role only', () => {
    // Arrange
    const roles = [account('admin'), account('user'), null];

    // Act
    const shown = roles.map((who) => isAdmin(who));

    // Assert
    expect(shown).toEqual([true, false, false]);
  });

  it('opens Users for a missing or unknown tab', () => {
    // Arrange / Act / Assert
    expect([tabOf(null), tabOf('nope'), tabOf('health')]).toEqual(['users', 'users', 'health']);
  });
});

describe('quota input to bytes (AC-5)', () => {
  it.each([
    ['250', 'GB', 250_000_000_000],
    ['1.5', 'TB', 1_500_000_000_000],
    ['0.1', 'GB', 100_000_000],
    ['0', 'GB', 0],
    [' 2 ', 'TB', 2_000_000_000_000],
  ] as const)('reads %s %s as %d bytes exactly', (text, unit, expected) => {
    // Act
    const parsed = limitBytes(text, unit);

    // Assert
    expect(parsed).toEqual({ ok: true, bytes: expected });
  });

  it.each([
    ['', 'GB'],
    ['abc', 'GB'],
    ['-5', 'GB'],
    ['1e3', 'GB'],
    ['1.0000000001', 'GB'],
    ['99999999999', 'TB'],
  ] as const)('refuses %j %s', (text, unit) => {
    // Act
    const parsed = limitBytes(text, unit);

    // Assert
    expect(parsed.ok).toBe(false);
  });
});

describe('health items to banner (AC-6)', () => {
  const item = (name: string, status: 'ok' | 'degraded' | 'down') => ({ name, status, detail: '' });

  it('reports ready for all ok checks and for none', () => {
    // Act / Assert
    expect(readiness({ items: [item('db', 'ok')] })).toEqual({ state: 'ok', failing: [] });
    expect(readiness({ items: [] })).toEqual({ state: 'ok', failing: [] });
  });

  it('lets the worst status win and names the failing checks, worst first', () => {
    // Arrange
    const health = { items: [item('db', 'ok'), item('blobs', 'degraded'), item('queue', 'down')] };

    // Act
    const result = readiness(health);

    // Assert
    expect(result).toEqual({ state: 'down', failing: ['queue', 'blobs'] });
  });
});

describe('invite status', () => {
  const now = Date.parse('2026-11-10T00:00:00Z');

  it('is used when redeemed, expired after the expiry, pending otherwise', () => {
    // Act / Assert
    expect(
      inviteStatus(
        { redeemed_at: '2026-11-01T00:00:00Z', expires_at: '2026-12-01T00:00:00Z' },
        now,
      ),
    ).toBe('used');
    expect(inviteStatus({ redeemed_at: null, expires_at: '2026-11-09T00:00:00Z' }, now)).toBe(
      'expired',
    );
    expect(inviteStatus({ expires_at: '2026-11-11T00:00:00Z' }, now)).toBe('pending');
  });
});

describe('user names', () => {
  const named = { id: 'u1', display_name: 'Daria Koval' };
  const unnamed = { id: 'u2', display_name: '  ' };

  it('uses the display name, and the id when the account has none', () => {
    // Act
    const names = [userName(named), userName(unnamed)];

    // Assert
    expect(names).toEqual(['Daria Koval', 'u2']);
  });

  it('writes a picker label with the name, the (you) marker and the id as the detail', () => {
    // Act
    const labels = [
      userChoiceLabel(named, false),
      userChoiceLabel(named, true),
      userChoiceLabel(unnamed, true),
    ];

    // Assert
    expect(labels).toEqual(['Daria Koval · u1', 'Daria Koval (you) · u1', 'u2 (you)']);
  });
});
