import { describe, expect, it } from 'vitest';
import { makeNode } from '../../../tests/support/treeServer';
import { lightboxSource, previewKind } from './preview';

describe('previewKind', () => {
  it.each([
    ['image/jpeg', 'image'],
    ['image/heic', 'image'],
    ['video/quicktime', 'video'],
    ['application/pdf', 'none'],
    ['text/html', 'none'],
    [null, 'none'],
  ])('%s is %s', (type, kind) => {
    // Act
    const result = previewKind(type);

    // Assert
    expect(result).toBe(kind);
  });
});

describe('lightboxSource', () => {
  it('shows the 1440 px preview of a ready thumbnail', () => {
    // Arrange
    const node = makeNode({ name: 'a.jpg', kind: 'file', media_type: 'image/jpeg' });

    // Act
    const source = lightboxSource(node, 'ready');

    // Assert
    expect(source).toEqual({
      kind: 'image',
      url: `/api/v1/nodes/${node.node_id}/thumbnail?size=preview`,
    });
  });

  it('shows a video only as its poster', () => {
    // Arrange
    const node = makeNode({ name: 'a.mov', kind: 'file', media_type: 'video/quicktime' });

    // Act
    const source = lightboxSource(node, 'ready');

    // Assert
    expect(source.kind).toBe('poster');
  });

  it('asks for the small size of a video poster, the only size a video has', () => {
    // Arrange
    const node = makeNode({ name: 'a.mov', kind: 'file', media_type: 'video/quicktime' });

    // Act
    const source = lightboxSource(node, 'ready');

    // Assert
    expect(source).toEqual({
      kind: 'poster',
      url: `/api/v1/nodes/${node.node_id}/thumbnail?size=small`,
    });
  });

  it('falls back to the original for an allowlisted image whose thumbnail is not ready', () => {
    // Arrange
    const node = makeNode({ name: 'a.png', kind: 'file', media_type: 'image/png' });

    // Act
    const source = lightboxSource(node, 'pending');

    // Assert
    expect(source).toEqual({ kind: 'image', url: `/api/v1/nodes/${node.node_id}/content` });
  });

  it.each(['image/svg+xml', 'text/html', 'application/pdf', 'image/heic'])(
    'never puts the original of %s in an image',
    (type) => {
      // Arrange
      const node = makeNode({ name: 'x', kind: 'file', media_type: type });

      // Act
      const source = lightboxSource(node, 'pending');

      // Assert
      expect(source.kind).toBe('none');
    },
  );

  it('decides by the server media type, not by the file name', () => {
    // Arrange
    const node = makeNode({ name: 'x.png', kind: 'file', media_type: 'text/html' });

    // Act
    const source = lightboxSource(node, 'failed');

    // Assert
    expect(source.kind).toBe('none');
  });

  it('does not use a thumbnail for a kind that has none', () => {
    // Arrange
    const node = makeNode({ name: 'x.pdf', kind: 'file', media_type: 'application/pdf' });

    // Act
    const source = lightboxSource(node, 'ready');

    // Assert
    expect(source.kind).toBe('none');
  });
});
