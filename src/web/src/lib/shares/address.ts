/** The path of the public page without its token: what the address and the history keep after the page loaded. */
export const PUBLIC_SHARE_PATH = '/s';

const TOKEN_PATH = /^\/s\/([^/]+)$/;

/** The link token of `/s/<token>`, or null for any other path (a reload of the page after the token left the address). */
export function tokenOfPath(path: string): string | null {
  const match = TOKEN_PATH.exec(path);
  return match?.[1] === undefined ? null : decodeURIComponent(match[1]);
}

/**
 * Replaces the current history entry, so neither the address bar nor the history keeps the token and no later request
 * of the page sends it as a referrer. The router's own state is not touched: the page keeps the token in memory.
 */
export function removeTokenFromAddress() {
  history.replaceState(null, '', PUBLIC_SHARE_PATH);
}

/** The bytes of a link, for the preview and the browser's own download. */
export function shareContentUrl(token: string): string {
  return `/api/v1/public-shares/${encodeURIComponent(token)}/content`;
}
