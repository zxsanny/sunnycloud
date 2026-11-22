import { describe, expect, it } from 'vitest';
import { browserRows, deviceRows, lastSeenText, linkOutcome, quotaView } from './view';

const NOW = Date.parse('2026-11-21T12:00:00Z');

describe('quotaView', () => {
  it('marks the quota as the server default only when the account has no limit of its own', () => {
    // Arrange
    const base = { effective_limit: 100_000_000_000, used: 38_200_000_000, reserved: 0 };

    // Act
    const inherited = quotaView({ ...base, limit_bytes: null });
    const own = quotaView({ ...base, limit_bytes: 100_000_000_000 });

    // Assert
    expect(inherited.serverDefault).toBe(true);
    expect(own.serverDefault).toBe(false);
    expect(inherited.percent).toBe(38);
  });

  it('caps the bar at 100 percent and tolerates a zero limit', () => {
    // Arrange
    const over = { effective_limit: 10, used: 25, reserved: 0, limit_bytes: 10 };
    const none = { effective_limit: 0, used: 0, reserved: 0, limit_bytes: 0 };

    // Act and Assert
    expect(quotaView(over).percent).toBe(100);
    expect(quotaView(none).percent).toBe(0);
  });
});

describe('lastSeenText', () => {
  it.each([
    [null, 'Never seen'],
    ['not a time', 'Never seen'],
    ['2026-11-21T11:59:40Z', 'Last seen just now'],
    ['2026-11-21T11:58:00Z', 'Last seen 2 min ago'],
    ['2026-11-21T11:00:00Z', 'Last seen 1 hour ago'],
    ['2026-11-18T12:00:00Z', 'Last seen 3 days ago'],
    ['2026-03-04T12:00:00Z', 'Last seen 4 Mar 2026'],
  ])('writes %s as "%s"', (iso, expected) => {
    // Act
    const text = lastSeenText(iso, NOW);

    // Assert
    expect(text).toBe(expected);
  });
});

describe('deviceRows', () => {
  it('gives one row per device in the server order, with its own name and platform', () => {
    // Arrange
    const list = {
      items: [
        {
          id: 'a',
          name: 'zx-thinkpad',
          platform: 'Linux',
          created_at: '2026-03-04T10:00:00Z',
          last_seen_at: null,
        },
        {
          id: 'b',
          name: 'studio-mac',
          platform: 'macOS',
          created_at: '2026-06-19T10:00:00Z',
          last_seen_at: '2026-11-21T11:58:00Z',
        },
      ],
    };

    // Act
    const rows = deviceRows(list, null, NOW);

    // Assert
    expect(rows.map((row) => row.id)).toEqual(['a', 'b']);
    expect(rows[1]).toMatchObject({
      detail: 'Desktop app · macOS · added 19 Jun 2026',
      lastSeen: 'Last seen 2 min ago',
    });
  });

  const session = (id: string, kind: 'web' | 'android' | 'ios', over: object = {}) => ({
    id,
    label: `label ${id}`,
    client_kind: kind,
    ip: null,
    current: false,
    created_at: '2026-04-02T10:00:00Z',
    last_seen_at: '2026-11-21T11:00:00Z',
    ...over,
  });
  const devices = {
    items: [{ id: 'x', name: 'tower', platform: 'Linux', created_at: '2026-03-04T10:00:00Z' }],
  };

  it('puts phone sessions after the desktop devices in one list, marked by their app, with no web session', () => {
    // Arrange
    const sessions = {
      items: [session('w', 'web'), session('a', 'android'), session('i', 'ios')],
    };

    // Act
    const rows = deviceRows(devices, sessions, NOW);

    // Assert
    expect(rows.map((row) => [row.kind, row.id])).toEqual([
      ['desktop', 'x'],
      ['android', 'a'],
      ['ios', 'i'],
    ]);
    expect(rows[1]?.detail).toBe('Android app · signed in 2 Apr 2026');
    expect(rows[2]?.detail).toBe('iOS app · signed in 2 Apr 2026');
  });

  it('keeps a device and a session with the same id as two rows with two keys', () => {
    // Arrange
    const sessions = { items: [session('x', 'android')] };

    // Act
    const rows = deviceRows(devices, sessions, NOW);

    // Assert
    expect(rows).toHaveLength(2);
    expect(new Set(rows.map((row) => row.key)).size).toBe(2);
  });
});

describe('browserRows', () => {
  const session = (id: string, over: object = {}) => ({
    id,
    label: `browser ${id}`,
    client_kind: 'web' as const,
    ip: null,
    current: false,
    created_at: '2026-11-01T10:00:00Z',
    last_seen_at: '2026-11-21T11:00:00Z',
    ...over,
  });

  it('lists web sessions only, marks the current one and adds an address only when there is one', () => {
    // Arrange
    const list = {
      items: [
        session('a', { current: true, ip: '203.0.113.24' }),
        session('b'),
        { ...session('c'), client_kind: 'ios' as const },
      ],
    };

    // Act
    const rows = browserRows(list, NOW);

    // Assert
    expect(rows.map((row) => row.id)).toEqual(['a', 'b']);
    expect(rows[0]).toMatchObject({
      current: true,
      detail: 'Signed in 1 Nov 2026 · 203.0.113.24',
      lastSeen: 'Active now',
    });
    expect(rows[1]).toMatchObject({ current: false, detail: 'Signed in 1 Nov 2026' });
  });
});

describe('linkOutcome', () => {
  it('reads nothing from a query without a link outcome', () => {
    // Act
    const outcome = linkOutcome(new URLSearchParams('tab=x'));

    // Assert
    expect(outcome).toBeNull();
  });

  it('lets an error win over a linked flag', () => {
    // Act
    const outcome = linkOutcome(new URLSearchParams('linked=1&error=identity_taken'));

    // Assert
    expect(outcome?.variant).toBe('danger');
  });

  it('treats a linked value other than 1 as no outcome', () => {
    // Act
    const outcome = linkOutcome(new URLSearchParams('linked=0'));

    // Assert
    expect(outcome).toBeNull();
  });
});
