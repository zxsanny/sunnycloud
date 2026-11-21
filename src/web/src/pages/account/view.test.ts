import { describe, expect, it } from 'vitest';
import { deviceRows, lastSeenText, quotaView } from './view';

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
    const rows = deviceRows(list, NOW);

    // Assert
    expect(rows.map((row) => row.id)).toEqual(['a', 'b']);
    expect(rows[1]).toMatchObject({
      platform: 'macOS',
      added: '19 Jun 2026',
      lastSeen: 'Last seen 2 min ago',
    });
  });
});
