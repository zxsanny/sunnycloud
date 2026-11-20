const FORMAT = new Intl.DateTimeFormat('en-GB', {
  day: 'numeric',
  month: 'short',
  year: 'numeric',
});

/** `30 Oct 2026` from an RFC 3339 time; an unreadable value gives an empty string. */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return '';
  const at = new Date(iso);
  return Number.isNaN(at.getTime()) ? '' : FORMAT.format(at);
}
