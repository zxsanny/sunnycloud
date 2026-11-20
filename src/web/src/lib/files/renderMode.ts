const INLINE_TYPES = new Set([
  'image/jpeg',
  'image/png',
  'image/gif',
  'image/webp',
  'image/avif',
  'video/mp4',
  'video/webm',
]);

/**
 * Whether the server's media type may be shown by the browser itself. Only these seven types are `inline`;
 * HTML, SVG, XML, JavaScript, PDF, polyglots and anything unknown are offered as a download (AC-47). The
 * argument is the media type the server stored, never a file extension.
 */
export function renderMode(mediaType: string | null | undefined): 'inline' | 'attachment' {
  const base = (mediaType ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  return INLINE_TYPES.has(base) ? 'inline' : 'attachment';
}
