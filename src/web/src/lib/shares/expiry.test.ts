import { describe, expect, it } from 'vitest';
import { parseExpiryDays } from './expiry';

describe('the expiry field', () => {
  it('leaves expires_in out when the field is empty, so the server applies its own default', () => {
    // Arrange
    const typed = ['', '   '];

    // Act
    const choices = typed.map(parseExpiryDays);

    // Assert
    expect(choices).toEqual([
      { ok: true, expiresIn: undefined },
      { ok: true, expiresIn: undefined },
    ]);
  });

  it('turns whole days into seconds', () => {
    // Arrange
    const typed = '7';

    // Act
    const choice = parseExpiryDays(typed);

    // Assert
    expect(choice).toEqual({ ok: true, expiresIn: 7 * 86_400 });
  });

  it.each(['0', '-3', '1.5', 'abc', '7d', '1e3', '99999999999999999999'])(
    'refuses %j without asking the server',
    (typed) => {
      // Arrange

      // Act
      const choice = parseExpiryDays(typed);

      // Assert
      expect(choice).toEqual({
        ok: false,
        text: 'Enter a whole number of days, or leave the field empty.',
      });
    },
  );

  it('does not cap the number: the longest lifetime is the server answer, never a value held here', () => {
    // Arrange
    const typed = '3650';

    // Act
    const choice = parseExpiryDays(typed);

    // Assert
    expect(choice).toEqual({ ok: true, expiresIn: 3650 * 86_400 });
  });
});
