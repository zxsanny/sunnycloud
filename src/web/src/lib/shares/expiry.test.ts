import { describe, expect, it } from 'vitest';
import { durationText, expiryOptions } from './expiry';

const DAY = 86_400;
const NOW = Date.parse('2026-11-21T10:00:00Z');

const secondsOf = (options: ReturnType<typeof expiryOptions>) =>
  options.options.map((option) => option.seconds);

describe('the expiry choices', () => {
  it('preselects the server default and offers nothing above the longest lifetime', () => {
    // Arrange
    const ttl = { defaultTtl: 7 * DAY, maxTtl: 30 * DAY };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(secondsOf(result)).toEqual([DAY, 7 * DAY, 30 * DAY]);
    expect(result.preselected.seconds).toBe(7 * DAY);
    expect(result.preselected.label).toBe('7 days, until 28 Nov 2026');
    expect(result.helper).toBe('This server allows links for up to 30 days.');
  });

  it('drops the presets above a short maximum and adds the maximum itself', () => {
    // Arrange
    const ttl = { defaultTtl: 2 * DAY, maxTtl: 3 * DAY };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(secondsOf(result)).toEqual([DAY, 2 * DAY, 3 * DAY]);
    expect(result.preselected.seconds).toBe(2 * DAY);
  });

  it('caps a default that is longer than the maximum', () => {
    // Arrange
    const ttl = { defaultTtl: 90 * DAY, maxTtl: 30 * DAY };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(result.preselected.seconds).toBe(30 * DAY);
    expect(Math.max(...(secondsOf(result) as number[]))).toBe(30 * DAY);
  });

  it('offers only the server default when the server stated no limits, so expires_in is left out', () => {
    // Arrange
    const ttl = { defaultTtl: null, maxTtl: null };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(secondsOf(result)).toEqual([undefined]);
    expect(result.preselected.label).toBe('Server default');
  });

  it('keeps "Server default" first and selected when only the maximum is stated', () => {
    // Arrange
    const ttl = { defaultTtl: null, maxTtl: 7 * DAY };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(secondsOf(result)).toEqual([undefined, DAY, 7 * DAY]);
    expect(result.preselected.seconds).toBeUndefined();
  });

  it('offers only the default when only the default is stated', () => {
    // Arrange
    const ttl = { defaultTtl: 7 * DAY, maxTtl: null };

    // Act
    const result = expiryOptions(ttl, NOW);

    // Assert
    expect(secondsOf(result)).toEqual([7 * DAY]);
  });
});

describe('durationText', () => {
  it.each([
    [DAY, '1 day'],
    [30 * DAY, '30 days'],
    [36 * 3600, '36 hours'],
    [90 * 60, '90 minutes'],
    [45, '45 seconds'],
  ])('writes %i seconds as "%s"', (seconds, expected) => {
    // Act
    const text = durationText(seconds);

    // Assert
    expect(text).toBe(expected);
  });
});
