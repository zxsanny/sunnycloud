import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer, type Reply } from '../../tests/support/http';
import { navigate } from '$lib/router';
import { session } from '$lib/session';
import AccountPage from './AccountPage.svelte';
import AdminPage from './AdminPage.svelte';

const ME = '0190aaaa-0000-7000-8000-0000000000aa';
const OTHER = '0190aaaa-0000-7000-8000-0000000000cc';
const OFF = '0190aaaa-0000-7000-8000-0000000000dd';
const GONE = '0190aaaa-0000-7000-8000-0000000000ee';

const accountView = (role: 'admin' | 'user') => ({
  id: ME,
  display_name: 'Marta Hryn',
  role,
  identities: [{ provider: 'github', email: 'marta@example.org' }],
  quota: { limit_bytes: null, effective_limit: 100_000_000_000, used: 0, reserved: 0 },
  root_node_id: '0190aaaa-0000-7000-8000-0000000000bb',
});

const user = (id: string, over: Record<string, unknown> = {}) => ({
  id,
  display_name: id === ME ? 'Marta Hryn' : id === OTHER ? 'Daria Koval' : `Person ${id.slice(-2)}`,
  role: 'user',
  state: 'active',
  last_login: '2026-11-02T10:00:00Z',
  used: 71_400_000_000,
  limit: null,
  ...over,
});

const DEFAULT_QUOTA = 100_000_000_000;

const USERS = {
  default_quota_bytes: DEFAULT_QUOTA,
  items: [
    user(ME, { role: 'admin' }),
    user(OTHER),
    user(OFF, { state: 'disabled' }),
    user(GONE, { state: 'deleting' }),
  ],
};

const USERS_URL = 'GET /api/v1/admin/users';

function serve(over: Record<string, Reply> = {}) {
  return stubServer({
    'GET /api/v1/accounts/me': json(accountView('admin')),
    'GET /api/v1/accounts/me/quota': json({
      limit_bytes: null,
      effective_limit: 1,
      used: 0,
      reserved: 0,
    }),
    'GET /api/v1/devices': json({ items: [] }),
    'GET /api/v1/sessions': json({ items: [] }),
    'GET /api/v1/auth/providers': json({ providers: [], setup_required: false }),
    [USERS_URL]: json(USERS),
    ...over,
  });
}

beforeEach(() => {
  session.status = 'ready';
  session.account = accountView('admin');
  navigate('/admin?tab=users', { replace: true });
});
afterEach(() => vi.unstubAllGlobals());

describe('visibility (AC-1)', () => {
  it('shows the admin link and all four tabs to an admin', async () => {
    // Arrange
    serve();

    // Act
    render(AdminPage);

    // Assert
    expect(await screen.findByRole('link', { name: /Admin/ })).toHaveAttribute('href', '/admin');
    const tabs = within(screen.getByRole('tablist')).getAllByRole('tab');
    expect(tabs.map((tab) => tab.textContent?.trim())).toEqual([
      'Users',
      'Invites',
      'Quotas',
      'Health',
    ]);
  });

  it.each(['users', 'invites', 'quotas', 'health'])(
    'shows not found on the %s tab to a plain user and calls no admin operation',
    async (tab) => {
      // Arrange
      session.account = accountView('user');
      navigate(`/admin?tab=${tab}`, { replace: true });
      const { calls } = serve({ 'GET /api/v1/accounts/me': json(accountView('user')) });

      // Act
      render(AdminPage);

      // Assert
      expect(await screen.findByText('Page not found')).toBeInTheDocument();
      expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
      expect(calls.some((call) => call.url.pathname.includes('/admin/'))).toBe(false);
    },
  );

  it('shows no admin link on the account page of a plain user', async () => {
    // Arrange
    session.account = accountView('user');
    serve({ 'GET /api/v1/accounts/me': json(accountView('user')) });

    // Act
    render(AccountPage);

    // Assert
    await screen.findByRole('heading', { name: 'Account' });
    expect(screen.queryByRole('link', { name: /Admin/ })).not.toBeInTheDocument();
  });

  it('opens the tab named by ?tab=', async () => {
    // Arrange
    navigate('/admin?tab=health', { replace: true });
    serve({ 'GET /api/v1/admin/health': json({ items: [] }) });

    // Act
    render(AdminPage);

    // Assert
    expect(await screen.findByText('Ready.')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Health' })).toHaveAttribute('aria-selected', 'true');
  });
});

describe('role and state changes (AC-2, AC-3, AC-7)', () => {
  it('makes a user an admin with one call and shows the new role', async () => {
    // Arrange
    const { calls } = serve({
      [`PATCH /api/v1/admin/users/${OTHER}`]: new Response(null, { status: 204 }),
    });
    render(AdminPage);
    const row = (await screen.findByText(OTHER)).closest('tr')!;

    // Act
    await userEvent.click(within(row).getByRole('button', { name: 'Make admin' }));

    // Assert
    await within(row).findByRole('button', { name: 'Make user' });
    const patches = calls.filter((call) => call.method === 'PATCH');
    expect(patches).toHaveLength(1);
    expect(patches[0]?.body).toEqual({ role: 'admin' });
  });

  it('deactivates and reactivates with one call each', async () => {
    // Arrange
    const { calls } = serve({
      [`PATCH /api/v1/admin/users/${OTHER}`]: new Response(null, { status: 204 }),
      [`PATCH /api/v1/admin/users/${OFF}`]: new Response(null, { status: 204 }),
    });
    render(AdminPage);
    const active = (await screen.findByText(OTHER)).closest('tr')!;
    const off = screen.getByText(OFF).closest('tr')!;

    // Act
    await userEvent.click(within(active).getByRole('button', { name: 'Deactivate' }));
    await userEvent.click(within(off).getByRole('button', { name: 'Reactivate' }));

    // Assert
    await within(active).findByRole('button', { name: 'Reactivate' });
    await within(off).findByRole('button', { name: 'Deactivate' });
    expect(calls.filter((call) => call.method === 'PATCH').map((call) => call.body)).toEqual([
      { state: 'disabled' },
      { state: 'active' },
    ]);
  });

  it('shows an account in deletion as removing data with no actions and no delete offer', async () => {
    // Arrange
    serve();

    // Act
    render(AdminPage);

    // Assert
    const row = (await screen.findByText(GONE)).closest('tr')!;
    expect(within(row).getByText('Removing data…')).toBeInTheDocument();
    expect(within(row).queryAllByRole('button')).toHaveLength(0);
    expect(screen.queryByText(/Delete account/)).not.toBeInTheDocument();
  });

  it('shows the last-admin refusal inline and leaves role and state unchanged', async () => {
    // Arrange
    serve({
      [`PATCH /api/v1/admin/users/${ME}`]: problem(409, 'last_admin', {
        title: 'This is the last active admin; make another user an admin first.',
      }),
    });
    render(AdminPage);
    const row = (await screen.findByText(ME)).closest('tr')!;

    // Act
    await userEvent.click(within(row).getByRole('button', { name: 'Make user' }));

    // Assert
    expect(await screen.findByText(/the last active admin/)).toBeInTheDocument();
    expect(within(row).getByRole('button', { name: 'Make user' })).toBeInTheDocument();
    expect(within(row).getByRole('button', { name: 'Deactivate' })).toBeInTheDocument();
  });
});

describe('invites (AC-4)', () => {
  const invites = (items: unknown[]) => json({ items });
  const NOW = Date.now();
  const iso = (offsetDays: number) => new Date(NOW + offsetDays * 86_400_000).toISOString();
  const invite = (id: string, note: string, over: Record<string, unknown> = {}) => ({
    id,
    note,
    created_at: iso(-3),
    expires_at: iso(4),
    redeemed_at: null,
    ...over,
  });

  beforeEach(() => navigate('/admin?tab=invites', { replace: true }));

  it('shows the empty state when there are no invites', async () => {
    // Arrange
    serve({ 'GET /api/v1/admin/invites': invites([]) });

    // Act
    render(AdminPage);

    // Assert
    expect(await screen.findByText('No invites yet')).toBeInTheDocument();
  });

  it('lists Pending, Used and Expired invites and revokes a pending one with one call', async () => {
    // Arrange
    const { calls } = serve({
      'GET /api/v1/admin/invites': invites([
        invite('i1', 'Old laptop'),
        invite('i2', 'Daria', { redeemed_at: iso(-1) }),
        invite('i3', 'Book club', { expires_at: iso(-1) }),
      ]),
      'DELETE /api/v1/admin/invites/i1': new Response(null, { status: 204 }),
    });
    render(AdminPage);
    const rows = await screen.findAllByRole('row');
    const text = rows.map((row) => row.textContent);
    expect(text.some((t) => t?.includes('Old laptop') && t.includes('Pending'))).toBe(true);
    expect(text.some((t) => t?.includes('Daria') && t.includes('Used'))).toBe(true);
    expect(text.some((t) => t?.includes('Book club') && t.includes('Expired'))).toBe(true);
    expect(screen.getAllByRole('button', { name: /Revoke invite/ })).toHaveLength(1);

    // Act
    await userEvent.click(screen.getByRole('button', { name: /Revoke invite Old laptop/ }));

    // Assert
    await waitFor(() => expect(screen.queryByText('Old laptop')).not.toBeInTheDocument());
    expect(calls.filter((call) => call.method === 'DELETE')).toHaveLength(1);
  });

  const URL_ = 'https://drive.example.org/invite#sc_inv_SECRET';

  it('shows the invite link once with a copy of exactly it, the expiry from the answer, and not again after closing', async () => {
    // Arrange
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    let listed = 0;
    const { calls } = serve({
      'GET /api/v1/admin/invites': () => invites(listed++ === 0 ? [] : [invite('i9', 'Lesia')]),
      'POST /api/v1/admin/invites': json({
        token: 'sc_inv_SECRET',
        url: URL_,
        expires_at: '2026-12-05T10:00:00Z',
      }),
    });
    render(AdminPage);
    await screen.findByText('No invites yet');

    // Act
    await userEvent.type(screen.getByLabelText('Note (optional)'), 'Lesia');
    await userEvent.click(screen.getByRole('button', { name: 'Create invite' }));
    await screen.findByTestId('invite-url');
    await userEvent.click(screen.getByRole('button', { name: 'Copy invite link' }));

    // Assert
    expect(screen.getByTestId('invite-url')).toHaveTextContent(URL_);
    expect(writeText).toHaveBeenCalledWith(URL_);
    expect(screen.getByText('Single use · expires 5 Dec 2026')).toBeInTheDocument();
    expect(calls.find((call) => call.method === 'POST')?.body).toEqual({ note: 'Lesia' });
    expect(screen.getByRole('table')).toHaveTextContent('Pending');

    // Act
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));

    // Assert
    expect(screen.queryByText(/sc_inv_SECRET/)).not.toBeInTheDocument();
  });

  it('shows the expiry of the answer even when the refreshed list is not read', async () => {
    // Arrange
    let listed = 0;
    serve({
      'GET /api/v1/admin/invites': () => (listed++ === 0 ? invites([]) : problem(500, 'internal')),
      'POST /api/v1/admin/invites': json({
        token: 'sc_inv_SECRET',
        url: URL_,
        expires_at: '2026-12-05T10:00:00Z',
      }),
    });
    render(AdminPage);
    await screen.findByText('No invites yet');

    // Act
    await userEvent.click(screen.getByRole('button', { name: 'Create invite' }));

    // Assert
    expect(await screen.findByText('Single use · expires 5 Dec 2026')).toBeInTheDocument();
  });

  it('writes the token and the link nowhere in browser storage', async () => {
    // Arrange
    serve({
      'GET /api/v1/admin/invites': invites([]),
      'POST /api/v1/admin/invites': json({
        token: 'sc_inv_SECRET',
        url: URL_,
        expires_at: '2026-12-05T10:00:00Z',
      }),
    });
    render(AdminPage);
    await screen.findByText('No invites yet');

    // Act
    await userEvent.click(screen.getByRole('button', { name: 'Create invite' }));
    await screen.findByTestId('invite-url');

    // Assert
    expect(JSON.stringify({ ...localStorage })).not.toContain('sc_inv_SECRET');
    expect(JSON.stringify({ ...sessionStorage })).not.toContain('sc_inv_SECRET');
  });
});

describe('quotas (AC-5)', () => {
  beforeEach(() => navigate('/admin?tab=quotas', { replace: true }));

  it('saves 250 GB as exact bytes, lists it, then Use default clears it', async () => {
    // Arrange
    const { calls } = serve({
      [`PUT /api/v1/admin/users/${OTHER}/quota`]: new Response(null, { status: 204 }),
    });
    render(AdminPage);
    await screen.findByText('Per-user quotas', { selector: 'h2' });
    await userEvent.selectOptions(await screen.findByLabelText('User'), OTHER);

    // Act
    await userEvent.type(screen.getByLabelText('Quota'), '250');
    await userEvent.click(screen.getByRole('button', { name: 'Save quota' }));

    // Assert
    const table = await screen.findByRole('table', { name: 'Per-user quotas' });
    expect(within(table).getByText('250.0 GB')).toBeInTheDocument();

    // Act
    await userEvent.click(within(table).getByRole('button', { name: /Use default quota/ }));

    // Assert
    await screen.findByText('Everyone uses the default quota');
    expect(calls.filter((call) => call.method === 'PUT').map((call) => call.body)).toEqual([
      { limit: 250_000_000_000 },
      { limit: null },
    ]);
  });

  it('refuses an invalid limit inline and sends nothing', async () => {
    // Arrange
    const { calls } = serve();
    render(AdminPage);
    await userEvent.selectOptions(await screen.findByLabelText('User'), OTHER);

    // Act
    await userEvent.type(screen.getByLabelText('Quota'), 'lots');
    await userEvent.click(screen.getByRole('button', { name: 'Save quota' }));

    // Assert
    expect(await screen.findByText(/Enter a number/)).toBeInTheDocument();
    expect(calls.some((call) => call.method === 'PUT')).toBe(false);
  });

  it('shows the server refusal inline and keeps the list unchanged', async () => {
    // Arrange
    serve({
      [`PUT /api/v1/admin/users/${OTHER}/quota`]: problem(400, 'invalid_request', {
        title: 'The limit is not valid.',
      }),
    });
    render(AdminPage);
    await userEvent.selectOptions(await screen.findByLabelText('User'), OTHER);

    // Act
    await userEvent.type(screen.getByLabelText('Quota'), '5');
    await userEvent.click(screen.getByRole('button', { name: 'Save quota' }));

    // Assert
    expect(await screen.findByText('The limit is not valid.')).toBeInTheDocument();
    expect(screen.getByText('Everyone uses the default quota')).toBeInTheDocument();
  });

  it('shows the default quota value of the server as read-only with its configuration note', async () => {
    // Arrange
    serve();

    // Act
    render(AdminPage);

    // Assert
    expect(await screen.findByTestId('default-quota')).toHaveTextContent('100.0 GB');
    expect(screen.getByText(/cannot be\s+changed here/)).toBeInTheDocument();
  });
});

describe('health (AC-6)', () => {
  const health = (status: string) => ({
    items: [
      { name: 'database', status: 'ok', detail: 'reachable' },
      { name: 'blob volume', status, detail: 'free space below the floor' },
    ],
  });

  beforeEach(() => navigate('/admin?tab=health', { replace: true }));

  it('names the degraded check, lists every check and re-reads on Check again', async () => {
    // Arrange
    let reads = 0;
    serve({ 'GET /api/v1/admin/health': () => json(health(reads++ === 0 ? 'degraded' : 'ok')) });
    render(AdminPage);

    // Act
    expect(await screen.findByText(/Needs attention: blob volume/)).toBeInTheDocument();
    const table = screen.getByRole('table', { name: 'Health checks' });
    expect(within(table).getAllByRole('row')).toHaveLength(3);
    expect(within(table).getByText('free space below the floor')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Check again' }));

    // Assert
    expect(await screen.findByText('Ready.')).toBeInTheDocument();
    expect(reads).toBe(2);
  });

  it('shows a failing tab error with the request id and leaves the other tabs usable', async () => {
    // Arrange
    serve({ 'GET /api/v1/admin/health': problem(403, 'forbidden', { title: 'Admins only.' }) });
    render(AdminPage);

    // Act
    expect(await screen.findByText('The health checks could not be read.')).toBeInTheDocument();
    expect(screen.getByText('0190aaaa-0000-7000-8000-000000000001')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('tab', { name: 'Users' }));

    // Assert
    expect(await screen.findByText(OTHER)).toBeInTheDocument();
  });
});
