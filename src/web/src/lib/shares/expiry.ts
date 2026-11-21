const SECONDS_PER_DAY = 86_400;

export type ExpiryChoice =
  { ok: true; expiresIn: number | undefined } | { ok: false; text: string };

/**
 * The owner's expiry field in whole days. Empty means "the server's default": `expires_in` is omitted, because no
 * operation tells the client the default or the longest lifetime. A value above the server's longest lifetime is
 * refused by the server (`ExpiryOutOfRange`), not by this function.
 */
export function parseExpiryDays(text: string): ExpiryChoice {
  const trimmed = text.trim();
  if (trimmed === '') return { ok: true, expiresIn: undefined };
  const days = /^\d+$/.test(trimmed) ? Number(trimmed) : NaN;
  const seconds = days * SECONDS_PER_DAY;
  if (!(days >= 1) || !Number.isSafeInteger(seconds)) {
    return { ok: false, text: 'Enter a whole number of days, or leave the field empty.' };
  }
  return { ok: true, expiresIn: seconds };
}
