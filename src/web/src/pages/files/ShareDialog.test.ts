import { cleanup, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer, type Reply } from '../../../tests/support/http';
import { makeNode } from '../../../tests/support/treeServer';
import { api } from '$lib/api';
import RowActions from './RowActions.svelte';

const NOW = '2026-11-21T10:00:00.000Z';
const FILE = makeNode({ name: 'beach-sunset.jpg', kind: 'file', media_type: 'image/jpeg' });
const FOLDER = makeNode({ name: 'Trips', kind: 'folder' });
const TOKEN = 'sc_shr_Q7mVb4LpT8wZr1NcY6dHs3EjA5uG0oRkWiPqXe9fK2x';
const SHARES = 'GET /api/v1/shares';
const SERVER_INFO = 'GET /api/v1/server-info';
const DAY = 86_400;

const serverInfo = (over: Record<string, unknown> = {}) => ({
  api_versions: ['v1'],
  batch_max_entries: 100,
  block_size: 2_097_152,
  longpoll_max_wait: 30,
  max_lanes: 4,
  max_staged_per_account: 8,
  min_client_version: '0.1.0',
  small_file_max: 2_097_152,
  share_default_ttl: 7 * DAY,
  share_max_ttl: 30 * DAY,
  ...over,
});

const info = (id: string, over: Record<string, unknown> = {}) => ({
  id,
  node_id: FILE.node_id,
  expires_at: '2026-11-27T10:00:00Z',
  has_password: false,
  created_at: '2026-11-20T09:00:00Z',
  ...over,
});
const created = (over: Record<string, unknown> = {}) => ({
  id: '0190aaaa-0000-7000-8000-0000000000e1',
  token: TOKEN,
  url_path: `/s/${TOKEN}`,
  expires_at: '2026-11-28T10:00:00Z',
  has_password: false,
  ...over,
});

function serve(extra: Record<string, Reply> = {}) {
  return stubServer({ [SHARES]: json({ items: [] }), [SERVER_INFO]: json(serverInfo()), ...extra });
}

/** Opens the dialog and waits for the server's limits to have reached the expiry field. */
async function openDialog(node = FILE) {
  const user = userEvent.setup();
  render(RowActions, { node });
  await user.click(screen.getByRole('button', { name: `Share link for ${node.name}` }));
  const dialog = await screen.findByRole('dialog', { name: `Share ${node.name}` });
  await within(dialog).findByRole('option', { name: /^7 days/ });
  return { user, dialog };
}

/** Opens the dialog when the server states no usable limits: nothing to wait for but the list. */
async function openDialogWithoutLimits(node = FILE) {
  const user = userEvent.setup();
  render(RowActions, { node });
  await user.click(screen.getByRole('button', { name: `Share link for ${node.name}` }));
  const dialog = await screen.findByRole('dialog', { name: `Share ${node.name}` });
  await within(dialog).findByText('This file has no links yet.');
  return { user, dialog };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date(NOW));
  api.setErrorHandler(() => {});
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('the share dialog: a link shown once (AC-1)', () => {
  it('creates a link with the default expiry, shows the one URL, copies exactly it and lists the link without a token after reopening', async () => {
    // Arrange
    let listed: unknown[] = [];
    const server = serve({
      [SHARES]: () => json({ items: listed }),
      'POST /api/v1/shares': () => {
        listed = [
          info('0190aaaa-0000-7000-8000-0000000000e1', { expires_at: '2026-11-28T10:00:00Z' }),
        ];
        return json(created(), 201);
      },
    });
    const { user, dialog } = await openDialog();
    const url = `${location.origin}/s/${TOKEN}`;

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));
    const panel = await screen.findByRole('region', { name: 'Link created' });
    await user.click(within(panel).getByRole('button', { name: 'Copy link' }));

    // Assert (the one URL, the copy, the request)
    expect(panel).toHaveTextContent(url);
    expect(document.body.textContent?.split(TOKEN).length).toBe(2);
    expect(await navigator.clipboard.readText()).toBe(url);
    expect(await within(panel).findByText('Link copied')).toBeVisible();
    expect(within(panel).getByText('This is the only time the full link is shown.')).toBeVisible();
    expect(panel).toHaveTextContent('Expires 28 Nov 2026');
    const post = server.calls.find((call) => call.method === 'POST');
    expect(post?.body).toEqual({ node_id: FILE.node_id, expires_in: 7 * DAY });

    // Act (close and open again)
    await user.click(screen.getByRole('button', { name: 'Done' }));
    await user.click(screen.getByRole('button', { name: `Share link for ${FILE.name}` }));
    const again = await screen.findByRole('dialog', { name: `Share ${FILE.name}` });

    // Assert (created, expiry and password flag listed; the token nowhere)
    await within(again).findByText(/Expires 28 Nov 2026/);
    expect(again).toHaveTextContent('Created 20 Nov 2026');
    expect(again).toHaveTextContent('No password');
    expect(again.textContent).not.toContain(TOKEN);
    expect(document.body.textContent).not.toContain(TOKEN);
    expect(within(again).queryByRole('region', { name: 'Link created' })).not.toBeInTheDocument();
  });

  it('sends the chosen expiry as seconds and the password only in the create request', async () => {
    // Arrange
    const server = serve({ 'POST /api/v1/shares': json(created({ has_password: true }), 201) });
    const { user, dialog } = await openDialog();
    await user.selectOptions(within(dialog).getByLabelText('Expires'), '1 day, until 22 Nov 2026');
    await user.click(within(dialog).getByRole('switch', { name: 'Require a password' }));
    const password = within(dialog).getByLabelText('Password');

    // Act
    await user.type(password, 'sunset-at-lake');
    await user.click(within(dialog).getByRole('button', { name: 'Show password' }));
    const shown = (password as HTMLInputElement).type;
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));
    const panel = await screen.findByRole('region', { name: 'Link created' });

    // Assert
    expect(shown).toBe('text');
    expect(within(dialog).queryByText('sunset-at-lake')).not.toBeInTheDocument();
    expect(server.calls.find((call) => call.method === 'POST')?.body).toEqual({
      node_id: FILE.node_id,
      expires_in: DAY,
      password: 'sunset-at-lake',
    });
    expect(panel).toHaveTextContent('Password required');
    expect(JSON.stringify({ ...localStorage, ...sessionStorage })).not.toContain('sunset-at-lake');
  });

  it('asks for a password before it creates a link that needs one', async () => {
    // Arrange
    const server = serve();
    const { user, dialog } = await openDialog();

    // Act
    await user.click(within(dialog).getByRole('switch', { name: 'Require a password' }));

    // Assert
    const create = within(dialog).getByRole('button', { name: 'Create link' });
    expect(create).toBeDisabled();
    expect(create).toHaveAttribute('title', 'Enter a password, or turn the option off.');
    expect(within(dialog).getByText('Send the password separately from the link.')).toBeVisible();
    expect(server.calls.filter((call) => call.method === 'POST')).toEqual([]);
  });
});

describe('the share dialog: the expiry (AC-2)', () => {
  it('preselects the server default and offers no choice above the longest lifetime', async () => {
    // Arrange
    serve();

    // Act
    const { dialog } = await openDialog();

    // Assert
    const field = within(dialog).getByLabelText('Expires');
    expect(field).toHaveDisplayValue('7 days, until 28 Nov 2026');
    expect(
      within(field)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual([
      '1 day, until 22 Nov 2026',
      '7 days, until 28 Nov 2026',
      '30 days, until 21 Dec 2026',
    ]);
    expect(within(dialog).getByText('This server allows links for up to 30 days.')).toBeVisible();
  });

  it('offers only what fits under a short longest lifetime', async () => {
    // Arrange
    serve({ [SERVER_INFO]: json(serverInfo({ share_default_ttl: DAY, share_max_ttl: 3 * DAY })) });
    const user = userEvent.setup();
    render(RowActions, { node: FILE });

    // Act
    await user.click(screen.getByRole('button', { name: `Share link for ${FILE.name}` }));
    const dialog = await screen.findByRole('dialog', { name: `Share ${FILE.name}` });
    await within(dialog).findByText('This server allows links for up to 3 days.');

    // Assert
    const field = within(dialog).getByLabelText('Expires');
    expect(field).toHaveDisplayValue('1 day, until 22 Nov 2026');
    expect(within(field).getAllByRole('option')).toHaveLength(2);
    expect(within(field).queryByRole('option', { name: /^7 days/ })).not.toBeInTheDocument();
  });

  it('offers only "Server default" and sends no expires_in when the server states no limits', async () => {
    // Arrange
    const server = serve({
      [SERVER_INFO]: json(serverInfo({ share_default_ttl: undefined, share_max_ttl: undefined })),
      'POST /api/v1/shares': json(created(), 201),
    });
    const { user, dialog } = await openDialogWithoutLimits();
    const field = within(dialog).getByLabelText('Expires');

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));
    await screen.findByRole('region', { name: 'Link created' });

    // Assert
    expect(field).toHaveDisplayValue('Server default');
    expect(server.calls.find((call) => call.method === 'POST')?.body).toEqual({
      node_id: FILE.node_id,
    });
  });

  it('falls back to "Server default" when the limits cannot be read and still lets the owner create a link', async () => {
    // Arrange
    serve({ [SERVER_INFO]: problem(500, 'internal') });
    const { dialog } = await openDialogWithoutLimits();

    // Act
    const field = within(dialog).getByLabelText('Expires');

    // Assert
    expect(field).toHaveDisplayValue('Server default');
    expect(within(dialog).getByRole('button', { name: 'Create link' })).toBeEnabled();
  });

  it('shows the server refusal of an expiry next to the field and creates nothing', async () => {
    // Arrange
    serve({ 'POST /api/v1/shares': problem(400, 'invalid_request') });
    const { user, dialog } = await openDialog();
    const field = within(dialog).getByLabelText('Expires');

    // Act
    await user.selectOptions(field, '30 days, until 21 Dec 2026');
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));

    // Assert
    const error = await within(dialog).findByText(
      'The server does not accept this expiry. Choose a shorter time.',
    );
    expect(field).toHaveAccessibleDescription(/does not accept this expiry/);
    expect(error).toBeVisible();
    expect(screen.queryByRole('region', { name: 'Link created' })).not.toBeInTheDocument();
  });
});

describe('the share dialog: folders and refusals (AC-3)', () => {
  it('disables Share link on a folder row with its reason and opens no dialog', async () => {
    // Arrange
    serve();
    const user = userEvent.setup();
    render(RowActions, { node: FOLDER });

    // Act
    const share = screen.getByRole('button', { name: 'Share link for Trips' });
    await user.click(share);

    // Assert
    expect(share).toBeDisabled();
    expect(share).toHaveAttribute('title', 'Folder sharing is not available in this version');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows a server TargetNotShareable as such and leaves the list unchanged', async () => {
    // Arrange
    serve({ 'POST /api/v1/shares': problem(422, 'target_not_shareable') });
    const { user, dialog } = await openDialog();

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));

    // Assert
    expect(
      await within(dialog).findByText('This item cannot be shared. Links work for files only.'),
    ).toBeVisible();
    expect(within(dialog).getByText('This file has no links yet.')).toBeVisible();
  });

  it('says so when the file is gone', async () => {
    // Arrange
    serve({ 'POST /api/v1/shares': problem(404, 'not_found') });
    const { user, dialog } = await openDialog();

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Create link' }));

    // Assert
    expect(await within(dialog).findByText('This file no longer exists.')).toBeVisible();
  });
});

describe('the share dialog: revoke inline (AC-4)', () => {
  const ID_A = '0190aaaa-0000-7000-8000-0000000000a1';
  const ID_B = '0190aaaa-0000-7000-8000-0000000000a2';

  it('confirms in the row with no second dialog, makes one revoke call and drops the row', async () => {
    // Arrange
    const server = serve({
      [SHARES]: json({ items: [info(ID_A), info(ID_B, { has_password: true })] }),
      [`DELETE /api/v1/shares/${ID_A}`]: new Response(null, { status: 204 }),
    });
    const { user, dialog } = await openDialog();
    await within(dialog).findByText('2 active');

    // Act
    await user.click(
      within(dialog).getAllByRole('button', { name: /Revoke link created/ })[0] as HTMLElement,
    );

    // Assert (the question is in the row; no dialog on a dialog; nothing sent yet)
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(within(dialog).getByText('People who have this link lose access.')).toBeVisible();
    expect(server.calls.filter((call) => call.method === 'DELETE')).toEqual([]);

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Revoke link' }));

    // Assert
    await waitFor(() => expect(within(dialog).getByText('1 active')).toBeVisible());
    expect(server.calls.filter((call) => call.method === 'DELETE')).toHaveLength(1);
    expect(within(dialog).getByText('Password')).toBeVisible();
  });

  it('keeps the link when the owner chooses Keep', async () => {
    // Arrange
    const server = serve({ [SHARES]: json({ items: [info(ID_A)] }) });
    const { user, dialog } = await openDialog();
    await user.click(await within(dialog).findByRole('button', { name: /Revoke link created/ }));

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Keep' }));

    // Assert
    expect(within(dialog).getByRole('button', { name: /Revoke link created/ })).toBeVisible();
    expect(server.calls.filter((call) => call.method === 'DELETE')).toEqual([]);
  });

  it('shows a failed revoke in the row and keeps the link', async () => {
    // Arrange
    serve({
      [SHARES]: json({ items: [info(ID_A)] }),
      [`DELETE /api/v1/shares/${ID_A}`]: problem(500, 'internal'),
    });
    const { user, dialog } = await openDialog();
    await user.click(await within(dialog).findByRole('button', { name: /Revoke link created/ }));

    // Act
    await user.click(within(dialog).getByRole('button', { name: 'Revoke link' }));

    // Assert
    expect(
      await within(dialog).findByText('The link could not be revoked. Try again.'),
    ).toBeVisible();
    expect(within(dialog).getByText(/Created 20 Nov 2026/)).toBeVisible();
  });
});

describe('the share dialog: the list of a file (AC-5)', () => {
  it('asks for the links of the file by its node id and says that links follow the file', async () => {
    // Arrange
    const server = serve({
      [SHARES]: json({ items: [info('0190aaaa-0000-7000-8000-0000000000a1')] }),
    });

    // Act
    const { dialog } = await openDialog();

    // Assert
    await within(dialog).findByText(/Created 20 Nov 2026/);
    expect(server.calls[0]?.url.searchParams.get('node_id')).toBe(FILE.node_id);
    expect(
      within(dialog).getByText(
        /Links follow the file: they keep working after you rename or move it/,
      ),
    ).toBeVisible();
  });

  it('marks a listed link whose expiry has passed', async () => {
    // Arrange
    serve({
      [SHARES]: json({
        items: [
          info('0190aaaa-0000-7000-8000-0000000000a1', { expires_at: '2026-11-01T00:00:00Z' }),
        ],
      }),
    });

    // Act
    const { dialog } = await openDialog();

    // Assert
    expect(await within(dialog).findByText(/Expired 1 Nov 2026/)).toBeVisible();
    expect(within(dialog).getByText('0 active')).toBeVisible();
  });

  it('offers a retry when the links cannot be loaded', async () => {
    // Arrange
    serve({ [SHARES]: problem(500, 'internal') });

    // Act
    const { dialog } = await openDialog();

    // Assert
    expect(await within(dialog).findByText('The links could not be loaded.')).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Try again' })).toBeEnabled();
  });

  it('closes with Escape and gives the focus back to the row button', async () => {
    // Arrange
    serve();
    const { user } = await openDialog();

    // Act
    await user.keyboard('{Escape}');

    // Assert
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: `Share link for ${FILE.name}` })).toHaveFocus();
  });
});
