import { describe, expect, it } from 'vitest';
import { parseCursor } from '.';

describe('the cursor of /changes/head', () => {
  it.each([
    ['1.42', { epoch: 1n, seq: 42n }],
    [
      '18446744073709551615.9007199254740993',
      { epoch: 18446744073709551615n, seq: 9007199254740993n },
    ],
  ])('reads %s', (text, point) => {
    // Act
    const parsed = parseCursor(text);

    // Assert
    expect(parsed).toEqual(point);
  });

  it.each(['', '1', '1.', '.1', '1.2.3', 'a.b', '-1.2', '1.2 '])('refuses %j', (text) => {
    // Act
    const parsed = parseCursor(text);

    // Assert
    expect(parsed).toBeNull();
  });
});
