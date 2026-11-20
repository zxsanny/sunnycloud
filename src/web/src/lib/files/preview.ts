import type { NodeView } from '$lib/api';
import { renderMode } from './renderMode';

export type PreviewKind = 'image' | 'video' | 'none';
export type ThumbState = 'ready' | 'pending' | 'failed' | 'none';

/** Which kinds get a server-made thumbnail: images and videos. Everything else gets a neutral icon. */
export function previewKind(mediaType: string | null | undefined): PreviewKind {
  const base = (mediaType ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  if (base.startsWith('image/')) return 'image';
  if (base.startsWith('video/')) return 'video';
  return 'none';
}

export const thumbnailUrl = (nodeId: string, size: 'small' | 'preview') =>
  `/api/v1/nodes/${encodeURIComponent(nodeId)}/thumbnail?size=${size}`;

export const contentUrl = (nodeId: string) => `/api/v1/nodes/${encodeURIComponent(nodeId)}/content`;

export type LightboxSource =
  { kind: 'image'; url: string } | { kind: 'poster'; url: string } | { kind: 'none' };

/**
 * What the lightbox may show for a file. A ready thumbnail (a server-made WebP, for any image or video type, a
 * video as its poster only). Otherwise the original, but only an image whose server media type is on the
 * allowlist. Nothing else is ever put in an image, frame or video element.
 */
export function lightboxSource(node: NodeView, thumb: ThumbState): LightboxSource {
  const kind = previewKind(node.media_type);
  if (thumb === 'ready' && kind !== 'none') {
    const url = thumbnailUrl(node.node_id, 'preview');
    return kind === 'video' ? { kind: 'poster', url } : { kind: 'image', url };
  }
  if (kind === 'image' && renderMode(node.media_type) === 'inline') {
    return { kind: 'image', url: contentUrl(node.node_id) };
  }
  return { kind: 'none' };
}
