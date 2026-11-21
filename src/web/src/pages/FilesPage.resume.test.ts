import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TreeServer } from '../../tests/support/treeServer';
import { App } from '../app';
import { folderStore, viewPreference } from '$lib/files';
import { notices } from '$lib/notices';
import { navigate, startRouter } from '$lib/router';
import { session } from '$lib/session';
import { transfer, type JobProgress } from '$lib/transfer';

let server: TreeServer;
let stopRouter: () => void;

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
  notices.items = [];
});

const stopped = (id: string, name: string): JobProgress => ({
  id,
  direction: 'upload',
  name,
  parentId: 'p',
  size: 5_000_000,
  state: 'stopped',
  phase: null,
  bytesDone: 0,
  lanes: 0,
  errorText: 'Stopped when the tab closed. Select the same file to continue.',
});

async function openWith(...rows: JobProgress[]) {
  navigate('/', { replace: true });
  render(App);
  await screen.findByText('This folder is empty');
  transfer.jobs.push(...rows);
}

describe('uploads stopped when the tab closed', () => {
  it('lists the stopped uploads in a banner and gives each row a Select file action', async () => {
    // Arrange
    await openWith(stopped('up-1', 'archive-2019.zip'), stopped('up-2', 'video.mov'));

    // Act
    const banner = await screen.findByText('2 uploads stopped when the tab closed.');

    // Assert
    expect(banner.closest('[role="status"]')).toHaveTextContent(
      'Select the same files again to continue; only the missing parts are sent.',
    );
    expect(screen.getByRole('button', { name: 'Select file for archive-2019.zip' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Select file for video.mov' })).toBeVisible();
    expect(screen.getAllByText(/Stopped when the tab closed. Select the same file/)).toHaveLength(
      2,
    );
  });

  it('says "1 upload" for a single stopped upload and shows no banner without one', async () => {
    // Arrange
    await openWith();

    // Act
    expect(screen.queryByText(/stopped when the tab closed\./)).not.toBeInTheDocument();
    transfer.jobs.push(stopped('up-1', 'archive-2019.zip'));

    // Assert
    expect(await screen.findByText('1 upload stopped when the tab closed.')).toBeInTheDocument();
  });

  it('sends each file chosen with the banner to the stopped upload of the same name', async () => {
    // Arrange
    const resume = vi
      .spyOn(transfer, 'resumeUpload')
      .mockResolvedValue({ ok: true, value: { kind: 'Resumed', resentBytes: 0 } });
    const user = userEvent.setup();
    await openWith(stopped('up-1', 'archive-2019.zip'), stopped('up-2', 'video.mov'));
    const video = new File(['v'], 'video.mov');
    const archive = new File(['a'], 'archive-2019.zip');

    // Act
    await user.click(await screen.findByRole('button', { name: 'Select files' }));
    await user.upload(screen.getByLabelText('Choose files to upload'), [video, archive]);

    // Assert
    expect(resume).toHaveBeenCalledTimes(2);
    expect(resume).toHaveBeenCalledWith(video, 'up-2');
    expect(resume).toHaveBeenCalledWith(archive, 'up-1');
  });

  it('sends the file chosen on a row to that row, whatever its name', async () => {
    // Arrange
    const resume = vi.spyOn(transfer, 'resumeUpload').mockResolvedValue({
      ok: true,
      value: { kind: 'Restarted', reason: 'ChangedSinceLastTime' },
    });
    const user = userEvent.setup();
    await openWith(stopped('up-1', 'archive-2019.zip'), stopped('up-2', 'video.mov'));
    const renamed = new File(['v'], 'video-final.mov');

    // Act
    await user.click(await screen.findByRole('button', { name: 'Select file for video.mov' }));
    await user.upload(screen.getByLabelText('Choose files to upload'), renamed);

    // Assert
    expect(resume).toHaveBeenCalledExactlyOnceWith(renamed, 'up-2');
  });

  it('says so when a chosen file matches no stopped upload, and resumes nothing', async () => {
    // Arrange
    const resume = vi.spyOn(transfer, 'resumeUpload');
    const user = userEvent.setup();
    await openWith(stopped('up-1', 'archive-2019.zip'));

    // Act
    await user.click(await screen.findByRole('button', { name: 'Select file' }));
    await user.upload(
      screen.getByLabelText('Choose files to upload'),
      new File(['x'], 'other.bin'),
    );

    // Assert
    expect(resume).not.toHaveBeenCalled();
    expect(await screen.findByText('No stopped upload has that name.')).toBeInTheDocument();
    expect(screen.getByText('other.bin')).toBeInTheDocument();
  });

  it('treats the next ordinary pick as a new upload when the person closed the resume dialog', async () => {
    // Arrange
    const resume = vi.spyOn(transfer, 'resumeUpload');
    const enqueue = vi.spyOn(transfer, 'enqueueUpload').mockResolvedValue({ ok: true, value: [] });
    const user = userEvent.setup();
    await openWith(stopped('up-1', 'archive-2019.zip'));
    await user.click(await screen.findByRole('button', { name: 'Select file' }));
    const file = new File(['x'], 'new.bin');

    // Act
    await user.click(screen.getAllByRole('button', { name: 'Upload files' })[0] as HTMLElement);
    await user.upload(screen.getByLabelText('Choose files to upload'), file);

    // Assert
    expect(resume).not.toHaveBeenCalled();
    await waitFor(() => expect(enqueue).toHaveBeenCalledOnce());
  });

  it('lists the stopped uploads of the account once it is known, and stops the uploads when the page is hidden', async () => {
    // Arrange
    const load = vi.spyOn(transfer, 'loadStopped').mockResolvedValue();
    const suspend = vi.spyOn(transfer, 'suspend').mockImplementation(() => {});
    navigate('/', { replace: true });
    render(App);
    await screen.findByText('This folder is empty');

    // Act
    window.dispatchEvent(new Event('pagehide'));

    // Assert
    await waitFor(() => expect(load).toHaveBeenCalled());
    expect(suspend).toHaveBeenCalledOnce();
  });
});
