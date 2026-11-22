import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer, type Reply } from '../../tests/support/http';
import { followExternal, navigate } from '$lib/router';
import { session } from '$lib/session';
import AccountPage from './AccountPage.svelte';

vi.mock('$lib/router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/router')>()),
  followExternal: vi.fn(),
}));

const ACCOUNT_ID = '0190aaaa-0000-7000-8000-0000000000aa';
const AUTH_URL = 'https://github.com/login/oauth/authorize?state=s';

const accountView = (over: Record<string, unknown> = {}) => ({
  id: ACCOUNT_ID,
  display_name: 'Marta Hryn',
  role: 'user',
  identities: [{ provider: 'github', email: 'marta.hryn@example.org' }],
  quota: {
    limit_bytes: null,
    effective_limit: 100_000_000_000,
    used: 38_200_000_000,
    reserved: 1_200_000_000,
  },
  root_node_id: '0190aaaa-0000-7000-8000-0000000000bb',
  ...over,
});

const quotaView = (over: Record<string, unknown> = {}) => ({
  limit_bytes: null,
  effective_limit: 100_000_000_000,
  used: 38_200_000_000,
  reserved: 1_200_000_000,
  ...over,
});

const device = (id: string, name: string, platform: string) => ({
  id,
  name,
  platform,
  created_at: '2026-03-04T10:00:00.000000Z',
  last_seen_at: null,
});

const PROVIDERS = {
  providers: [
    { key: 'github', display_name: 'GitHub' },
    { key: 'oidc', display_name: 'Google' },
  ],
  setup_required: false,
};

/** The routes every test needs; a test overrides the ones it is about. */
function serve(over: Record<string, Reply> = {}) {
  return stubServer({
    'GET /api/v1/accounts/me': json(accountView()),
    'GET /api/v1/accounts/me/quota': json(quotaView()),
    'GET /api/v1/devices': json({ items: [] }),
    'GET /api/v1/sessions': json({ items: [] }),
    'GET /api/v1/auth/providers': json(PROVIDERS),
    ...over,
  });
}

beforeEach(() => {
  session.status = 'ready';
  session.account = null;
  vi.mocked(followExternal).mockClear();
  navigate('/account', { replace: true });
});
afterEach(() => vi.unstubAllGlobals());

describe('the account page: profile and storage (AC-1)', () => {
  it('shows the name, email, provider and role of the account view', async () => {
    // Arrange
    serve({ 'GET /api/v1/accounts/me': json(accountView({ role: 'admin' })) });

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('marta.hryn@example.org')).toBeInTheDocument();
    expect(screen.getByTestId('profile-name')).toHaveTextContent('Marta Hryn');
    expect(
      within(screen.getByRole('list', { name: 'Linked providers' })).getByText('GitHub'),
    ).toBeInTheDocument();
    expect(screen.getByText('Admin')).toBeInTheDocument();
  });

  it('shows used, reserved and the default quota with the server-default mark', async () => {
    // Arrange
    serve();

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('38.2 GB')).toBeInTheDocument();
    expect(screen.getByText('of 100.0 GB used')).toBeInTheDocument();
    expect(screen.getByText('1.2 GB')).toBeInTheDocument();
    expect(screen.getByText('100.0 GB · server default')).toBeInTheDocument();
  });

  it('shows no server-default mark for a quota the account owns', async () => {
    // Arrange
    serve({
      'GET /api/v1/accounts/me/quota': json(
        quotaView({ limit_bytes: 50_000_000_000, effective_limit: 50_000_000_000 }),
      ),
    });

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('50.0 GB')).toBeInTheDocument();
    expect(screen.queryByText(/server default/)).not.toBeInTheDocument();
  });
});

describe('the account page: devices (AC-2, AC-3, AC-7)', () => {
  it('lists each desktop device with name, platform and added date', async () => {
    // Arrange
    serve({
      'GET /api/v1/devices': json({
        items: [device('d1', 'zx-thinkpad', 'Linux'), device('d2', 'studio-mac', 'macOS')],
      }),
    });

    // Act
    render(AccountPage);

    // Assert
    const list = await screen.findByRole('list', { name: 'Devices' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent('zx-thinkpad');
    expect(rows[0]).toHaveTextContent('Linux');
    expect(rows[0]).toHaveTextContent('added 4 Mar 2026');
    expect(rows[1]).toHaveTextContent('studio-mac');
  });

  it('makes no call when the user chooses Revoke and then Cancel', async () => {
    // Arrange
    const server = serve({
      'GET /api/v1/devices': json({ items: [device('d1', 'zx-thinkpad', 'Linux')] }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke zx-thinkpad' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    // Assert
    expect(server.calls.filter((call) => call.method === 'DELETE')).toHaveLength(0);
    expect(screen.getByText('zx-thinkpad')).toBeInTheDocument();
    expect(screen.queryByText('Revoke zx-thinkpad?')).not.toBeInTheDocument();
  });

  it('revokes with exactly one call on confirmation and removes the row', async () => {
    // Arrange
    const server = serve({
      'GET /api/v1/devices': json({
        items: [device('d1', 'zx-thinkpad', 'Linux'), device('d2', 'studio-mac', 'macOS')],
      }),
      'DELETE /api/v1/devices/d1': new Response(null, { status: 204 }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke zx-thinkpad' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Revoke device' }));

    // Assert
    await waitFor(() => expect(screen.queryByText('zx-thinkpad')).not.toBeInTheDocument());
    expect(screen.getByText('studio-mac')).toBeInTheDocument();
    const deletes = server.calls.filter((call) => call.method === 'DELETE');
    expect(deletes.map((call) => call.url.pathname)).toEqual(['/api/v1/devices/d1']);
  });

  it('shows the server error in the row and keeps the row when the revoke fails', async () => {
    // Arrange
    serve({
      'GET /api/v1/devices': json({ items: [device('d1', 'zx-thinkpad', 'Linux')] }),
      'DELETE /api/v1/devices/d1': problem(404, 'not_found'),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke zx-thinkpad' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Revoke device' }));

    // Assert
    expect(await screen.findByTestId('revoke-error')).toHaveTextContent('not_found');
    expect(screen.getByTestId('revoke-error')).toHaveTextContent(
      '0190aaaa-0000-7000-8000-000000000001',
    );
    expect(screen.getByText('zx-thinkpad')).toBeInTheDocument();
  });

  it('shows the empty state without app links when no device is signed in', async () => {
    // Arrange
    serve();

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('No apps signed in yet')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Get the/ })).not.toBeInTheDocument();
  });

  it('shows skeleton rows while the lists are loading', () => {
    // Arrange
    serve({
      'GET /api/v1/accounts/me': () => new Promise<Response>(() => {}),
      'GET /api/v1/accounts/me/quota': () => new Promise<Response>(() => {}),
      'GET /api/v1/devices': () => new Promise<Response>(() => {}),
    });

    // Act
    render(AccountPage);

    // Assert
    expect(screen.getByLabelText('Loading your profile')).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByLabelText('Loading storage')).toBeInTheDocument();
    expect(screen.getByLabelText('Loading devices')).toBeInTheDocument();
  });
});

describe('the account page: a failing section (NFR reliability)', () => {
  it('shows the error with the request id and keeps the other sections', async () => {
    // Arrange
    serve({ 'GET /api/v1/devices': problem(403, 'forbidden') });

    // Act
    render(AccountPage);

    // Assert
    const error = await screen.findByText('The devices could not be loaded.');
    expect(error.closest('[role="alert"]')).toHaveTextContent(
      '0190aaaa-0000-7000-8000-000000000001',
    );
    expect(await screen.findByText('marta.hryn@example.org')).toBeInTheDocument();
    expect(await screen.findByText('38.2 GB')).toBeInTheDocument();
  });
});

describe('the account page: sign out (AC-4)', () => {
  it('revokes this browser session and goes to the sign-in page', async () => {
    // Arrange
    const server = serve({
      'DELETE /api/v1/sessions/current': new Response(null, { status: 204 }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await screen.findByText('marta.hryn@example.org');

    // Act
    await user.click(screen.getAllByRole('button', { name: 'Sign out' })[1]!);

    // Assert
    await waitFor(() => expect(location.pathname).toBe('/login'));
    expect(
      server.calls.filter((call) => call.url.pathname === '/api/v1/sessions/current'),
    ).toHaveLength(1);
    expect(session.status).toBe('signed-out');
  });
});

describe('the account page: linking a provider (AC-5)', () => {
  it('offers only providers not yet linked and starts the link with the chosen one', async () => {
    // Arrange
    const server = serve({
      'POST /api/v1/identity-link-attempts': json({ authorization_url: AUTH_URL }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await screen.findByText('marta.hryn@example.org');
    await screen.findByText('GitHub');

    // Act
    await user.click(screen.getByRole('button', { name: 'Link another provider' }));
    await user.click(await screen.findByRole('button', { name: 'Link Google' }));

    // Assert
    expect(screen.queryByRole('button', { name: 'Link GitHub' })).not.toBeInTheDocument();
    await waitFor(() => expect(followExternal).toHaveBeenCalledWith(AUTH_URL));
    const start = server.calls.find(
      (call) => call.url.pathname === '/api/v1/identity-link-attempts',
    );
    expect(start?.body).toEqual({ provider: 'oidc' });
  });

  it('shows the server refusal and does not leave the page when the link cannot start', async () => {
    // Arrange
    serve({ 'POST /api/v1/identity-link-attempts': problem(400, 'invalid_request') });
    const user = userEvent.setup();
    render(AccountPage);
    await screen.findByText('GitHub');
    await user.click(screen.getByRole('button', { name: 'Link another provider' }));

    // Act
    await user.click(await screen.findByRole('button', { name: 'Link Google' }));

    // Assert
    expect(await screen.findByText('The provider could not be linked.')).toBeInTheDocument();
    expect(followExternal).not.toHaveBeenCalled();
  });

  it('refuses to follow a link address that is not http or https', async () => {
    // Arrange
    serve({
      'POST /api/v1/identity-link-attempts': json({ authorization_url: 'javascript:alert(1)' }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await screen.findByText('GitHub');
    await user.click(screen.getByRole('button', { name: 'Link another provider' }));

    // Act
    await user.click(await screen.findByRole('button', { name: 'Link Google' }));

    // Assert
    expect(await screen.findByText(/does not follow/)).toBeInTheDocument();
    expect(followExternal).not.toHaveBeenCalled();
  });
});

describe('the account page: account deletion (AC-6)', () => {
  async function openConfirmation(user: ReturnType<typeof userEvent.setup>) {
    render(AccountPage);
    await screen.findByText('marta.hryn@example.org');
    await user.click(screen.getByRole('button', { name: 'Delete account…' }));
  }

  it('keeps the delete button disabled until the account id is typed', async () => {
    // Arrange
    const server = serve();
    const user = userEvent.setup();
    await openConfirmation(user);

    // Act
    await user.type(screen.getByLabelText('Account ID'), 'wrong');

    // Assert
    expect(screen.getByRole('button', { name: 'Delete my account' })).toBeDisabled();
    expect(server.calls.filter((call) => call.method === 'DELETE')).toHaveLength(0);
  });

  it('sends the typed id, shows the deadline and signs the user out when accepted', async () => {
    // Arrange
    const server = serve({
      'DELETE /api/v1/accounts/me': json({ deadline: '2026-11-21T14:30:00Z' }, 202),
    });
    const user = userEvent.setup();
    await openConfirmation(user);
    await user.type(screen.getByLabelText('Account ID'), ACCOUNT_ID);

    // Act
    await user.click(screen.getByRole('button', { name: 'Delete my account' }));

    // Assert
    expect(await screen.findByText('Account deletion requested.')).toBeInTheDocument();
    expect(screen.getByText(/You are signed out/)).toBeInTheDocument();
    expect(screen.queryByText('marta.hryn@example.org')).not.toBeInTheDocument();
    const request = server.calls.find((call) => call.method === 'DELETE');
    expect(request?.body).toEqual({ confirmation: ACCOUNT_ID });
    expect(session.status).toBe('signed-out');
  });

  it('shows the server refusal inline and changes nothing for the last active admin', async () => {
    // Arrange
    serve({
      'GET /api/v1/accounts/me': json(accountView({ role: 'admin' })),
      'DELETE /api/v1/accounts/me': problem(409, 'last_admin'),
    });
    const user = userEvent.setup();
    await openConfirmation(user);
    await user.type(screen.getByLabelText('Account ID'), ACCOUNT_ID);

    // Act
    await user.click(screen.getByRole('button', { name: 'Delete my account' }));

    // Assert
    expect(await screen.findByTestId('deletion-refusal')).toHaveTextContent('last_admin');
    expect(screen.getByText('marta.hryn@example.org')).toBeInTheDocument();
    expect(session.status).toBe('ready');
  });
});
