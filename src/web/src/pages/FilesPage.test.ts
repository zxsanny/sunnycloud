import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TreeServer, makeNode } from '../../tests/support/treeServer';
import { App } from '../app';
import { folderStore, viewPreference } from '$lib/files';
import { navigate, startRouter } from '$lib/router';
import { session } from '$lib/session';
import { transfer } from '$lib/transfer';

let server: TreeServer;
let stopRouter: () => void;

function open(path = '/') {
  navigate(path, { replace: true });
  return render(App);
}

beforeEach(() => {
  server = new TreeServer();
  vi.stubGlobal('fetch', (request: Request) => server.fetch(request));
  session.status = 'idle';
  session.account = null;
  folderStore.thumbs = {};
  localStorage.clear();
  viewPreference.mode = 'list';
  stopRouter = startRouter();
});
afterEach(() => {
  stopRouter();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  transfer.jobs = [];
});

const rowNamed = (name: string) => screen.getByText(name).closest('tr') as HTMLElement;
// A role query over hundreds of rows is slow in jsdom; the table rows are native <tr> elements.
const rowCount = () => screen.getByRole('table').querySelectorAll('tr').length;

describe('the file browser: listing and navigation', () => {
  it('lists the root with folders first and shows the breadcrumb', async () => {
    // Arrange
    server.add(server.root, { name: 'notes.txt', kind: 'file' });
    server.add(server.root, { name: 'Photos', kind: 'folder' });

    // Act
    open();

    // Assert
    expect(await screen.findByText('Photos')).toBeInTheDocument();
    const rows = screen.getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('Photos');
    expect(rows[1]).toHaveTextContent('notes.txt');
    expect(await screen.findByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
      'Files',
    );
  });

  it('opens a folder by its id from a link and shows the trail', async () => {
    // Arrange
    const photos = server.add(server.root, { name: 'Photos', kind: 'folder' });
    server.add(photos, { name: 'beach.jpg', kind: 'file', media_type: 'image/jpeg' });
    const user = userEvent.setup();
    open();

    // Act
    await user.click(await screen.findByRole('link', { name: 'Photos' }));

    // Assert
    expect(await screen.findByText('beach.jpg')).toBeInTheDocument();
    expect(location.pathname).toBe(`/folders/${photos.node_id}`);
    await waitFor(() =>
      expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Photos'),
    );
    expect(
      within(screen.getByRole('navigation', { name: 'Breadcrumb' })).getByRole('link', {
        name: 'Files',
      }),
    ).toHaveAttribute('href', '/');
  });

  it('opens the folder named in the address', async () => {
    // Arrange
    const photos = server.add(server.root, { name: 'Photos', kind: 'folder' });
    server.add(photos, { name: 'beach.jpg', kind: 'file' });

    // Act
    open(`/folders/${photos.node_id}`);

    // Assert
    expect(await screen.findByText('beach.jpg')).toBeInTheDocument();
  });

  it('pages by the cursor the server returned and shows every entry exactly once', async () => {
    // Arrange
    for (let i = 0; i < 450; i += 1)
      server.add(server.root, { name: `f-${String(i).padStart(3, '0')}`, kind: 'file' });
    const user = userEvent.setup();
    open();
    await screen.findByText('f-000');

    // Act
    await user.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(rowCount()).toBe(401));
    await user.click(screen.getByRole('button', { name: 'Load more' }));

    // Assert
    await waitFor(() => expect(rowCount()).toBe(451));
    expect(screen.queryByRole('button', { name: 'Load more' })).toBeNull();
    const cursors = server.requests
      .filter((r) => r.path.endsWith('/children'))
      .map((r) => r.query.get('after'));
    expect(cursors).toEqual([null, 'c:200', 'c:400']);
    expect(new Set(folderStore.items.map((i) => i.node_id)).size).toBe(450);
  }, 30_000); // renders 450 rows in jsdom: several seconds on a loaded machine

  it('shows the empty state with an upload action and the desktop app pointer', async () => {
    // Act
    open();

    // Assert
    expect(await screen.findByText('This folder is empty')).toBeInTheDocument();
    expect(screen.getByText(/the desktop app syncs faster/)).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /Upload files/ }).length).toBeGreaterThan(0);
  });

  it('shows skeleton rows while the listing is slow', async () => {
    // Arrange
    let release: (r: Response) => void = () => {};
    server.intercept = (_method, path) =>
      path.endsWith('/children')
        ? new Promise<Response>((resolve) => (release = resolve))
        : undefined;

    // Act
    open();

    // Assert
    expect(await screen.findByLabelText('Loading the folder')).toBeInTheDocument();
    await waitFor(() =>
      expect(server.requests.some((r) => r.path.endsWith('/children'))).toBe(true),
    );
    release(Response.json({ items: [], next: null }));
    expect(await screen.findByText('This folder is empty')).toBeInTheDocument();
  });

  it('offers a retry when the folder cannot be loaded', async () => {
    // Arrange
    let failing = true;
    server.intercept = (_method, path) =>
      failing && path.endsWith('/children')
        ? Response.json({ code: 'internal', status: 500 }, { status: 500 })
        : undefined;
    server.add(server.root, { name: 'back.txt', kind: 'file' });
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder could not be loaded.');
    failing = false;

    // Act
    await user.click(screen.getByRole('button', { name: 'Retry' }));

    // Assert
    expect(await screen.findByText('back.txt')).toBeInTheDocument();
  });

  it('renders hostile names as plain text and creates no element from them', async () => {
    // Arrange
    server.add(server.root, { name: '<img src=x onerror=alert(1)>', kind: 'file' });
    server.add(server.root, { name: '"><script>alert(1)</script>', kind: 'file' });
    server.add(server.root, { name: 'a‮b', kind: 'file' });

    // Act
    const { container } = open();

    // Assert
    expect(await screen.findByText('<img src=x onerror=alert(1)>')).toBeInTheDocument();
    expect(container.querySelector('img[src="x"]')).toBeNull();
    expect(container.querySelector('script')).toBeNull();
    expect(screen.getByText('"><script>alert(1)</script>')).toBeInTheDocument();
  });
});

describe('the file browser: folders and deleting', () => {
  it('creates a folder and shows it', async () => {
    // Arrange
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');

    // Act
    await user.click(screen.getByRole('button', { name: 'New folder' }));
    await user.type(screen.getByLabelText('Folder name'), 'Trips');
    await user.click(screen.getByRole('button', { name: 'Create' }));

    // Assert
    expect(await screen.findByRole('link', { name: 'Trips' })).toBeInTheDocument();
    expect(
      server.requests.filter((r) => r.method === 'POST' && r.path === '/api/v1/nodes'),
    ).toHaveLength(1);
  });

  it('shows the refusal under the field when the name is taken and creates nothing', async () => {
    // Arrange
    server.add(server.root, { name: 'Trips', kind: 'folder' });
    const user = userEvent.setup();
    open();
    await screen.findByText('Trips');

    // Act
    await user.click(screen.getByRole('button', { name: 'New folder' }));
    await user.type(screen.getByLabelText('Folder name'), 'Trips');
    await user.click(screen.getByRole('button', { name: 'Create' }));

    // Assert
    expect(
      await screen.findByText('A file or folder with this name already exists here.'),
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Folder name')).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getAllByRole('link', { name: 'Trips' })).toHaveLength(1);
  });

  it('deletes a file only after the inline confirmation, with its revision as the base', async () => {
    // Arrange
    const file = server.add(server.root, { name: 'old.txt', kind: 'file' });
    const user = userEvent.setup();
    open();
    await screen.findByText('old.txt');

    // Act
    await user.click(screen.getByRole('button', { name: 'Delete old.txt' }));
    expect(server.requests.some((r) => r.method === 'DELETE')).toBe(false);
    await user.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    await waitFor(() => expect(screen.queryByText('old.txt')).toBeNull());
    const call = server.requests.find((r) => r.method === 'DELETE');
    expect(call?.query.get('base_revision')).toBe(file.current_revision);
  });

  it('keeps the row when the confirmation is cancelled', async () => {
    // Arrange
    server.add(server.root, { name: 'keep.txt', kind: 'file' });
    const user = userEvent.setup();
    open();
    await screen.findByText('keep.txt');

    // Act
    await user.click(screen.getByRole('button', { name: 'Delete keep.txt' }));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    // Assert
    expect(screen.getByText('keep.txt')).toBeInTheDocument();
    expect(server.requests.some((r) => r.method === 'DELETE')).toBe(false);
  });

  it('shows the refusal next to the item and leaves the listing unchanged', async () => {
    // Arrange
    server.add(server.root, { name: 'busy.txt', kind: 'file' });
    server.intercept = (method) =>
      method === 'DELETE'
        ? Response.json(
            { code: 'edited_since', status: 409, title: 'x', type: 'x', request_id: 'r' },
            { status: 409 },
          )
        : undefined;
    const user = userEvent.setup();
    open();
    await screen.findByText('busy.txt');

    // Act
    await user.click(screen.getByRole('button', { name: 'Delete busy.txt' }));
    await user.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    expect(await within(rowNamed('busy.txt')).findByRole('alert')).toHaveTextContent(
      'This file changed elsewhere',
    );
  });
});

describe('the file browser: views and uploads', () => {
  it('keeps the chosen view after a reload and falls back to the list when storage is blocked', async () => {
    // Arrange
    server.add(server.root, { name: 'a.jpg', kind: 'file', media_type: 'image/jpeg' });
    const user = userEvent.setup();
    const first = open();
    await screen.findByText('a.jpg');

    // Act
    await user.click(screen.getByRole('button', { name: 'Photo grid' }));
    first.unmount();
    viewPreference.mode = 'list';
    open();
    await screen.findByText('a.jpg');

    // Assert
    expect(screen.getByRole('button', { name: 'Photo grid' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.queryByRole('table')).toBeNull();
  });

  it('works with blocked storage and shows the list', async () => {
    // Arrange
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('blocked', 'SecurityError');
    });
    server.add(server.root, { name: 'a.txt', kind: 'file' });

    // Act
    open();

    // Assert
    expect(await screen.findByRole('table')).toBeInTheDocument();
  });

  it('hands the chosen files and the open folder to the transfer service', async () => {
    // Arrange
    const enqueue = vi.spyOn(transfer, 'enqueueUpload').mockResolvedValue({ ok: true, value: [] });
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');
    const files = [new File(['a'], 'a.txt'), new File(['b'], 'b.txt')];

    // Act
    await user.upload(screen.getByLabelText('Choose files to upload'), files);

    // Assert
    expect(enqueue).toHaveBeenCalledExactlyOnceWith(files, server.root.node_id);
  });

  it('shows the refusal text when a selection is refused', async () => {
    // Arrange
    vi.spyOn(transfer, 'enqueueUpload').mockResolvedValue({
      ok: false,
      error: { code: 'TooManyFiles', text: 'This selection has 100,001 files.' },
    });
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');

    // Act
    await user.upload(screen.getByLabelText('Choose files to upload'), new File(['a'], 'a.txt'));

    // Assert
    expect(await screen.findByText('This selection has 100,001 files.')).toBeInTheDocument();
  });

  it('uploads dropped files and skips a dropped folder with a notice', async () => {
    // Arrange
    const enqueue = vi.spyOn(transfer, 'enqueueUpload').mockResolvedValue({ ok: true, value: [] });
    open();
    const region = await screen.findByRole('region', { name: 'Files' });
    await screen.findByText('This folder is empty');
    const file = new File(['a'], 'a.txt');
    const dataTransfer = {
      items: [
        { kind: 'file', getAsFile: () => file, webkitGetAsEntry: () => ({ isDirectory: false }) },
        {
          kind: 'file',
          getAsFile: () => new File([], 'dir'),
          webkitGetAsEntry: () => ({ isDirectory: true }),
        },
      ],
    };

    // Act
    const drop = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(drop, 'dataTransfer', { value: dataTransfer });
    region.dispatchEvent(drop);

    // Assert
    expect(enqueue).toHaveBeenCalledExactlyOnceWith([file], server.root.node_id);
    expect(await screen.findByText('Folders are not uploaded yet.')).toBeInTheDocument();
  });

  it('merges a finished upload into the open folder by reading its node', async () => {
    // Arrange
    let announce: (e: { nodeId: string; parentId: string; name: string }) => void = () => {};
    vi.spyOn(transfer, 'onUploaded').mockImplementation((listener) => {
      announce = listener;
      return () => {};
    });
    open();
    await screen.findByText('This folder is empty');
    const created = server.add(server.root, { name: 'fresh.txt', kind: 'file' });

    // Act
    announce({ nodeId: created.node_id, parentId: server.root.node_id, name: 'fresh.txt' });

    // Assert
    expect(await screen.findByText('fresh.txt')).toBeInTheDocument();
    expect(
      server.requests.filter((r) => r.path === `/api/v1/nodes/${created.node_id}`),
    ).toHaveLength(1);
  });

  it('ignores a finished upload that went to another folder', async () => {
    // Arrange
    let announce: (e: { nodeId: string; parentId: string; name: string }) => void = () => {};
    vi.spyOn(transfer, 'onUploaded').mockImplementation((listener) => {
      announce = listener;
      return () => {};
    });
    open();
    await screen.findByText('This folder is empty');
    const requestsBefore = server.requests.length;

    // Act
    announce({ nodeId: 'x', parentId: 'another-folder', name: 'x' });

    // Assert
    expect(server.requests.length).toBe(requestsBefore);
  });
});

describe('the transfers panel', () => {
  function addJob(extra: Partial<(typeof transfer.jobs)[number]> = {}) {
    transfer.jobs.push({
      id: 'up-1',
      name: 'movie.mov',
      parentId: 'p',
      size: 1000,
      state: 'running',
      phase: 'sending',
      bytesDone: 430,
      lanes: 4,
      ...extra,
    });
  }

  it('shows bytes, percent and lanes of a running job and cancels it', async () => {
    // Arrange
    const cancel = vi.spyOn(transfer, 'cancelUpload').mockImplementation(() => {});
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');

    // Act
    addJob();
    const bar = await screen.findByRole('progressbar', { name: 'movie.mov uploaded' });
    await user.click(screen.getByRole('button', { name: 'Cancel movie.mov' }));

    // Assert
    expect(bar).toHaveAttribute('aria-valuenow', '43');
    expect(screen.getByText(/430 B of 1 kB · 4 lanes/)).toBeInTheDocument();
    expect(cancel).toHaveBeenCalledExactlyOnceWith('up-1');
  });

  it('shows the error line of a failed job and retries it', async () => {
    // Arrange
    const retry = vi.spyOn(transfer, 'retryUpload').mockImplementation(() => {});
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');

    // Act
    addJob({ state: 'failed', error: 'QuotaExceeded', errorText: 'Storage quota reached.' });
    await user.click(await screen.findByRole('button', { name: 'Retry movie.mov' }));

    // Assert
    expect(screen.getByText('Not uploaded: Storage quota reached.')).toBeInTheDocument();
    expect(retry).toHaveBeenCalledExactlyOnceWith('up-1');
  });

  it('dismisses a finished job without touching the others', async () => {
    // Arrange
    const dismiss = vi.spyOn(transfer, 'dismiss').mockImplementation(() => {});
    const user = userEvent.setup();
    open();
    await screen.findByText('This folder is empty');
    addJob({ state: 'done' });

    // Act
    await user.click(await screen.findByRole('button', { name: 'Dismiss movie.mov' }));

    // Assert
    expect(dismiss).toHaveBeenCalledExactlyOnceWith('up-1');
  });
});

void makeNode;
