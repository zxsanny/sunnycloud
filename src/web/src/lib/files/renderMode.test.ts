import { describe, expect, it } from 'vitest';
import { renderMode } from './renderMode';

describe('renderMode', () => {
  it.each([
    'image/jpeg',
    'image/png',
    'image/gif',
    'image/webp',
    'image/avif',
    'video/mp4',
    'video/webm',
    'IMAGE/PNG',
    'image/png; charset=binary',
  ])('shows %s inline', (type) => {
    // Act
    const mode = renderMode(type);

    // Assert
    expect(mode).toBe('inline');
  });

  it.each([
    'image/svg+xml',
    'text/html',
    'application/xml',
    'text/javascript',
    'application/pdf',
    'application/octet-stream',
    'image/heic',
    '',
    null,
    undefined,
  ])('offers %s as a download', (type) => {
    // Act
    const mode = renderMode(type);

    // Assert
    expect(mode).toBe('attachment');
  });
});
