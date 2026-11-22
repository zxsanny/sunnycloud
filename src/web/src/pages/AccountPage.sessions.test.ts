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

const sessionItem = (id: string, label: string, over: Record<string, unknown> = {}) => ({
  id,
  label,
  client_kind: 'web',
  ip: null,
  current: false,
  created_at: '2026-11-01T10:00:00.000000Z',
  last_seen_at: '2026-11-21T11:58:00.000000Z',
  ...over,
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

describe('the account page: phone apps and browser sessions (AC-2, AC-3)', () => {
  const SESSIONS = {
    items: [
      sessionItem('s-here', 'Firefox on Linux', { current: true, ip: '203.0.113.24' }),
      sessionItem('s-safari', 'Safari on iPhone', { ip: '198.51.100.7' }),
      sessionItem('s-chrome', 'Chrome on Windows'),
      sessionItem('s-pixel', 'Pixel 8', {
        client_kind: 'android',
        created_at: '2026-04-02T10:00:00Z',
      }),
    ],
  };

  it('lists the 2 desktop devices and the Android session in one list and the 3 browsers in their own', async () => {
    // Arrange
    serve({
      'GET /api/v1/devices': json({
        items: [device('d1', 'zx-thinkpad', 'Linux'), device('d2', 'studio-mac', 'macOS')],
      }),
      'GET /api/v1/sessions': json(SESSIONS),
    });

    // Act
    render(AccountPage);

    // Assert
    const devices = within(await screen.findByRole('list', { name: 'Devices' })).getAllByRole(
      'listitem',
    );
    expect(devices).toHaveLength(3);
    expect(devices[2]).toHaveTextContent('Pixel 8');
    expect(devices[2]).toHaveTextContent('Android app · signed in 2 Apr 2026');
    const browsers = within(screen.getByRole('list', { name: 'Browser sessions' })).getAllByRole(
      'listitem',
    );
    expect(browsers).toHaveLength(3);
    expect(browsers.map((row) => row.textContent)).not.toContain(expect.stringContaining('Pixel'));
  });

  it('marks this browser, shows the address only when one was recorded and offers no Revoke on this browser', async () => {
    // Arrange
    serve({ 'GET /api/v1/sessions': json(SESSIONS) });

    // Act
    render(AccountPage);

    // Assert
    const list = await screen.findByRole('list', { name: 'Browser sessions' });
    const [here, safari, chrome] = within(list).getAllByRole('listitem');
    expect(within(here!).getByTestId('current-chip')).toHaveTextContent('This browser');
    expect(here).toHaveTextContent('Signed in 1 Nov 2026 · 203.0.113.24');
    expect(within(here!).queryByRole('button')).not.toBeInTheDocument();
    expect(safari).toHaveTextContent('198.51.100.7');
    expect(chrome).toHaveTextContent('Signed in 1 Nov 2026');
    expect(chrome).not.toHaveTextContent('·');
    expect(within(chrome!).getByRole('button', { name: 'Revoke Chrome on Windows' })).toBeVisible();
    expect(within(list).getAllByRole('button')).toHaveLength(2);
  });

  it('revokes a browser session with one call to its id and removes only that row', async () => {
    // Arrange
    const server = serve({
      'GET /api/v1/sessions': json(SESSIONS),
      'DELETE /api/v1/sessions/s-safari': new Response(null, { status: 204 }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke Safari on iPhone' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Revoke browser' }));

    // Assert
    await waitFor(() => expect(screen.queryByText('Safari on iPhone')).not.toBeInTheDocument());
    expect(screen.getByText('Chrome on Windows')).toBeInTheDocument();
    expect(screen.getByText('Pixel 8')).toBeInTheDocument();
    const deletes = server.calls.filter((call) => call.method === 'DELETE');
    expect(deletes.map((call) => call.url.pathname)).toEqual(['/api/v1/sessions/s-safari']);
  });

  it('revokes a phone session through the sessions route, not the devices route', async () => {
    // Arrange
    const server = serve({
      'GET /api/v1/sessions': json(SESSIONS),
      'DELETE /api/v1/sessions/s-pixel': new Response(null, { status: 204 }),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke Pixel 8' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Revoke app' }));

    // Assert
    await waitFor(() => expect(screen.queryByText('Pixel 8')).not.toBeInTheDocument());
    const deletes = server.calls.filter((call) => call.method === 'DELETE');
    expect(deletes.map((call) => call.url.pathname)).toEqual(['/api/v1/sessions/s-pixel']);
    expect(screen.getByText('Safari on iPhone')).toBeInTheDocument();
  });

  it('shows the server error in the row and keeps it when a session revoke fails', async () => {
    // Arrange
    serve({
      'GET /api/v1/sessions': json(SESSIONS),
      'DELETE /api/v1/sessions/s-chrome': problem(404, 'not_found'),
    });
    const user = userEvent.setup();
    render(AccountPage);
    await user.click(await screen.findByRole('button', { name: 'Revoke Chrome on Windows' }));

    // Act
    await user.click(screen.getByRole('button', { name: 'Revoke browser' }));

    // Assert
    expect(await screen.findByTestId('revoke-error')).toHaveTextContent('not_found');
    expect(screen.getByText('Chrome on Windows')).toBeInTheDocument();
  });

  it('shows the empty state of the browser list when no browser session is listed', async () => {
    // Arrange
    serve();

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('No browsers signed in')).toBeInTheDocument();
  });

  it('keeps the desktop devices and the profile when the sessions list fails', async () => {
    // Arrange
    serve({
      'GET /api/v1/devices': json({ items: [device('d1', 'zx-thinkpad', 'Linux')] }),
      'GET /api/v1/sessions': problem(500, 'internal'),
    });

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('The phone apps could not be loaded.')).toBeInTheDocument();
    expect(screen.getByText('The browser sessions could not be loaded.')).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Devices' })).toHaveTextContent('zx-thinkpad');
    expect(await screen.findByText('marta.hryn@example.org')).toBeInTheDocument();
  });
});

describe('the account page: the outcome of a link (AC-5)', () => {
  it('shows that the provider was linked and removes the query from the address', async () => {
    // Arrange
    serve();
    navigate('/account?linked=1', { replace: true });

    // Act
    render(AccountPage);

    // Assert
    expect(await screen.findByText('Provider linked.')).toBeInTheDocument();
    expect(location.pathname + location.search).toBe('/account');
  });

  it.each([
    ['identity_taken', 'That sign-in belongs to another account.'],
    ['invalid_request', 'The link request was not recognised.'],
    ['invitation_required', 'That provider account is not invited.'],
    ['account_not_active', 'This account is deactivated.'],
    ['something_else', 'The provider could not be linked.'],
  ])('shows the meaning of ?error=%s in plain words and links nothing', async (reason, title) => {
    // Arrange
    serve();
    navigate(`/account?error=${reason}`, { replace: true });

    // Act
    render(AccountPage);

    // Assert
    const notice = (await screen.findByText(title)).closest('[role]');
    expect(notice).toHaveTextContent('Nothing was linked.');
    expect(notice).not.toHaveTextContent(reason);
    expect(location.search).toBe('');
  });

  it('shows no notice and keeps the address when the page is opened without a link outcome', async () => {
    // Arrange
    serve();
    navigate('/account?tab=x', { replace: true });

    // Act
    render(AccountPage);
    await screen.findByText('marta.hryn@example.org');

    // Assert
    expect(screen.queryByText('Provider linked.')).not.toBeInTheDocument();
    expect(screen.queryByText('Nothing was linked.', { exact: false })).not.toBeInTheDocument();
    expect(location.search).toBe('?tab=x');
  });

  it('shows the linked provider after landing, from the account view the server now answers', async () => {
    // Arrange
    serve({
      'GET /api/v1/accounts/me': json(
        accountView({
          identities: [{ provider: 'github' }, { provider: 'oidc', email: 'marta@example.org' }],
        }),
      ),
    });
    navigate('/account?linked=1', { replace: true });

    // Act
    render(AccountPage);

    // Assert
    const list = await screen.findByRole('list', { name: 'Linked providers' });
    expect(within(list).getAllByRole('listitem')).toHaveLength(2);
    expect(within(list).getByText('Google')).toBeInTheDocument();
  });
});
