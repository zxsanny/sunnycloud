import { describe, expect, it } from 'vitest';
import { formatBytes } from './bytes';

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [999, '999 B'],
    [1_000, '1 kB'],
    [1_200_000, '1.2 MB'],
    [38_200_000_000, '38.2 GB'],
    [100_000_000_000, '100.0 GB'],
    [2_500_000_000_000, '2.5 TB'],
  ])('formats %d bytes as %s', (bytes, expected) => {
    // Act
    const text = formatBytes(bytes);

    // Assert
    expect(text).toBe(expected);
  });
});
