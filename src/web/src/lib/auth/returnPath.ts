/**
 * The return path after sign-in must stay inside the app: a relative path with one leading slash.
 * Everything else (other hosts, `//host`, backslash tricks, `javascript:`) becomes `/`.
 */
export function safeReturnPath(raw: string | null | undefined): string {
  if (!raw || !raw.startsWith('/') || raw.startsWith('//') || raw.includes('\\')) return '/';
  for (const char of raw) {
    const code = char.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return '/';
  }
  const origin = globalThis.location.origin;
  if (!URL.canParse(raw, origin)) return '/';
  return new URL(raw, origin).origin === origin ? raw : '/';
}

/** The provider's authorization URL is the one place the app leaves its origin; only http(s) is followed. */
export function isFollowableUrl(raw: string): boolean {
  if (!URL.canParse(raw)) return false;
  const { protocol } = new URL(raw);
  return protocol === 'https:' || protocol === 'http:';
}
