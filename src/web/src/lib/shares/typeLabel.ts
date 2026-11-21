const LABELS: Record<string, string> = {
  'image/jpeg': 'JPEG image',
  'image/png': 'PNG image',
  'image/gif': 'GIF image',
  'image/webp': 'WebP image',
  'image/avif': 'AVIF image',
  'video/mp4': 'MP4 video',
  'video/webm': 'WebM video',
  'application/pdf': 'PDF document',
};

/** A plain name for the media type the server stored; an unknown type is shown as the type itself, as text. */
export function typeLabel(mediaType: string | null | undefined): string {
  const base = (mediaType ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  if (base === '') return 'File';
  return LABELS[base] ?? base;
}
