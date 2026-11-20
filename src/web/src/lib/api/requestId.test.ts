import { describe, expect, it } from 'vitest';
import { uuidV7 } from './requestId';

describe('uuidV7', () => {
  it('formats a version 7, variant 10 UUID', () => {
    // Arrange
    const random = new Uint8Array(10).fill(0xff);

    // Act
    const id = uuidV7(1_700_000_000_000, random);

    // Assert
    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  it('starts with the millisecond timestamp in 48 bits so ids sort by time', () => {
    // Arrange
    const random = new Uint8Array(10);

    // Act
    const id = uuidV7(0x0190_1234_5678, random);

    // Assert
    expect(id.startsWith('01901234-5678-7')).toBe(true);
  });

  it('gives two different ids for the same millisecond', () => {
    // Act
    const first = uuidV7(1_700_000_000_000);
    const second = uuidV7(1_700_000_000_000);

    // Assert
    expect(first).not.toBe(second);
  });
});
