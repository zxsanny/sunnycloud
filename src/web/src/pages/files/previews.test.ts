import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TreeServer, type Node } from '../../../tests/support/treeServer';
import { App } from '../../app';
import { folderStore, viewPreference } from '$lib/files';
import { navigate } from '$lib/router';
import { session } from '$lib/session';

let server: TreeServer;

const hashOf = (node: Node) => node.content_hash as string;

function add(name: string, media: string, state?: string) {
  const node = server.add(server.root, { name, kind: 'file', media_type: media });
  if (state) server.thumbStates[hashOf(node)] = state;
  return node;
}

function open(mode: 'list' | 'grid' = 'list') {
  viewPreference.mode = mode;
  navigate('/', { replace: true });
  return render(App);
}

beforeEach(() => {
  server = new TreeServer();
  vi.stubGlobal('fetch', (request: Request) => server.fetch(request));
  session.status = 'idle';
  session.account = null;
  folderStore.thumbs = {};
  localStorage.clear();
});
afterEach(() => vi.unstubAllGlobals());

const rowNamed = (name: string) => screen.getByText(name).closest('tr') as HTMLElement;

describe('thumbnails in the listing', () => {
  it('shows a ready image as a lazy, async-decoded <img> from the small thumbnail route', async () => {
    // Arrange
    const photo = add('beach.jpg', 'image/jpeg', 'ready');

    // Act
    open();

    // Assert
    await screen.findByText('beach.jpg');
    const image = await waitFor(() => {
      const img = rowNamed('beach.jpg').querySelector('img');
      expect(img).not.toBeNull();
      return img as HTMLImageElement;
    });
    expect(image.getAttribute('src')).toBe(`/api/v1/nodes/${photo.node_id}/thumbnail?size=small`);
    expect(image).toHaveAttribute('loading', 'lazy');
    expect(image).toHaveAttribute('decoding', 'async');
    expect(image).toHaveAttribute('alt', '');
  });

  it('shows a skeleton and "Preview pending" while the preview is not ready, with no image', async () => {
    // Arrange
    add('new.jpg', 'image/jpeg', 'pending');

    // Act
    open();

    // Assert
    await screen.findByText('new.jpg');
    await waitFor(() =>
      expect(within(rowNamed('new.jpg')).getByText('Preview pending')).toBeInTheDocument(),
    );
    expect(rowNamed('new.jpg').querySelector('img')).toBeNull();
  });

  it.each([
    { label: 'a failed preview', state: 'failed' as string | undefined, media: 'image/jpeg' },
    { label: 'a preview with no job', state: undefined, media: 'image/jpeg' },
    { label: 'a kind that has no preview', state: undefined, media: 'application/pdf' },
  ])('shows the neutral icon and "No preview" for $label', async ({ state, media }) => {
    // Arrange
    add('thing', media, state);

    // Act
    open();

    // Assert
    await screen.findByText('thing');
    await waitFor(() =>
      expect(within(rowNamed('thing')).getByText('No preview')).toBeInTheDocument(),
    );
    expect(rowNamed('thing').querySelector('img')).toBeNull();
  });

  it('asks for the thumbnail states once per page, for the previewable files only', async () => {
    // Arrange
    const photo = add('a.jpg', 'image/jpeg', 'ready');
    const clip = add('b.mov', 'video/quicktime', 'ready');
    add('c.pdf', 'application/pdf');
    server.add(server.root, { name: 'folder', kind: 'folder' });

    // Act
    open();
    await screen.findByText('a.jpg');

    // Assert
    await waitFor(() =>
      expect(server.requests.some((r) => r.path === '/api/v1/thumbnail-states')).toBe(true),
    );
    const calls = server.requests.filter((r) => r.path === '/api/v1/thumbnail-states');
    expect(calls).toHaveLength(1);
    expect((calls[0]?.body as { content_hashes: string[] }).content_hashes.sort()).toEqual(
      [hashOf(photo), hashOf(clip)].sort(),
    );
  });

  it('shows a video as its poster with a type badge and creates no video, frame or object element', async () => {
    // Arrange
    add('holiday.mov', 'video/quicktime', 'ready');

    // Act
    const { container } = open();

    // Assert
    await screen.findByText('holiday.mov');
    await waitFor(() =>
      expect(within(rowNamed('holiday.mov')).getByText('Video')).toBeInTheDocument(),
    );
    expect(rowNamed('holiday.mov').querySelector('img')).not.toBeNull();
    expect(container.querySelector('video, iframe, object, embed, audio')).toBeNull();
  });

  it('falls back to the placeholder when the thumbnail image fails to load', async () => {
    // Arrange
    add('broken.jpg', 'image/jpeg', 'ready');
    open();
    await screen.findByText('broken.jpg');
    const image = await waitFor(() => {
      const img = rowNamed('broken.jpg').querySelector('img');
      expect(img).not.toBeNull();
      return img as HTMLImageElement;
    });

    // Act
    await fireEvent.error(image);

    // Assert
    expect(within(rowNamed('broken.jpg')).getByText('No preview')).toBeInTheDocument();
    expect(rowNamed('broken.jpg').querySelector('img')).toBeNull();
  });

  it('shows the same states on photo grid tiles', async () => {
    // Arrange
    add('a.jpg', 'image/jpeg', 'ready');
    add('b.jpg', 'image/jpeg', 'pending');

    // Act
    const { container } = open('grid');

    // Assert
    await screen.findByText('a.jpg');
    await waitFor(() => expect(container.querySelectorAll('li img')).toHaveLength(1));
    expect(screen.getByText('Preview pending')).toBeInTheDocument();
  });

  it('takes a pushed thumbnail state frame through the store seam', async () => {
    // Arrange
    const photo = add('late.jpg', 'image/jpeg', 'pending');
    open();
    await screen.findByText('late.jpg');
    await waitFor(() =>
      expect(within(rowNamed('late.jpg')).getByText('Preview pending')).toBeInTheDocument(),
    );

    // Act
    folderStore.setThumbState(hashOf(photo), 'ready');

    // Assert
    await waitFor(() => expect(rowNamed('late.jpg').querySelector('img')).not.toBeNull());
  });
});

describe('opening a file', () => {
  it('opens an image with a ready preview in a lightbox that shows the 1440 px preview and a download link', async () => {
    // Arrange
    const photo = add('beach.jpg', 'image/jpeg', 'ready');
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole('button', { name: 'beach.jpg' }));

    // Act
    const dialog = await screen.findByRole('dialog', { name: 'beach.jpg' });

    // Assert
    expect(within(dialog).getByRole('img', { name: 'beach.jpg' })).toHaveAttribute(
      'src',
      `/api/v1/nodes/${photo.node_id}/thumbnail?size=preview`,
    );
    expect(within(dialog).getByRole('link', { name: /Download/ })).toHaveAttribute(
      'href',
      `/api/v1/nodes/${photo.node_id}/content`,
    );
  });

  it('closes on Escape and returns focus to the control that opened it', async () => {
    // Arrange
    add('beach.jpg', 'image/jpeg', 'ready');
    const user = userEvent.setup();
    open();
    const opener = await screen.findByRole('button', { name: 'beach.jpg' });
    await user.click(opener);
    await screen.findByRole('dialog');
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();

    // Act
    await user.keyboard('{Escape}');

    // Assert
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(opener).toHaveFocus();
  });

  it('keeps Tab inside the dialog', async () => {
    // Arrange
    add('beach.jpg', 'image/jpeg', 'ready');
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole('button', { name: 'beach.jpg' }));
    const dialog = await screen.findByRole('dialog');

    // Act
    await user.tab();
    await user.tab();
    await user.tab();

    // Assert
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
  });

  it('shows a video only as its poster, says playback is missing, and creates no video element', async () => {
    // Arrange
    add('holiday.mov', 'video/quicktime', 'ready');
    const user = userEvent.setup();
    const { container } = open();
    await user.click(await screen.findByRole('button', { name: 'holiday.mov' }));

    // Act
    const dialog = await screen.findByRole('dialog');

    // Assert
    expect(within(dialog).getByText(/Video playback is not available yet/)).toBeInTheDocument();
    expect(within(dialog).getByRole('img')).toBeInTheDocument();
    expect(container.querySelector('video, iframe, object, embed')).toBeNull();
  });

  it('shows the original of an allowlisted image whose thumbnail is not ready', async () => {
    // Arrange
    const photo = add('fresh.png', 'image/png', 'pending');
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole('button', { name: 'fresh.png' }));

    // Act
    const dialog = await screen.findByRole('dialog');

    // Assert
    expect(within(dialog).getByRole('img')).toHaveAttribute(
      'src',
      `/api/v1/nodes/${photo.node_id}/content`,
    );
  });

  it.each([
    { name: 'report.pdf', media: 'application/pdf' },
    { name: 'page.html', media: 'text/html' },
    { name: 'logo.svg', media: 'image/svg+xml' },
    { name: 'x.png', media: 'text/html' },
  ])(
    'offers $name ($media) as a download only: no dialog, no image, no frame',
    async ({ name, media }) => {
      // Arrange
      const node = add(name, media, 'failed');
      const user = userEvent.setup();
      const { container } = open();
      const link = await screen.findByRole('link', { name });

      // Act
      await user.click(link);

      // Assert
      expect(link).toHaveAttribute('href', `/api/v1/nodes/${node.node_id}/content`);
      expect(link).toHaveAttribute('download', name);
      expect(screen.queryByRole('dialog')).toBeNull();
      expect(rowNamed(name).querySelector('img')).toBeNull();
      expect(container.querySelector('iframe, object, embed, video')).toBeNull();
    },
  );

  it('offers a plain download link for every file', async () => {
    // Arrange
    const node = add('notes.txt', 'text/plain');

    // Act
    open();

    // Assert
    expect(await screen.findByRole('link', { name: 'Download notes.txt' })).toHaveAttribute(
      'href',
      `/api/v1/nodes/${node.node_id}/content`,
    );
  });
});
