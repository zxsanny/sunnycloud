import { render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, stubServer, type Reply } from '../../tests/support/http';
import { navigate } from '$lib/router';
import { session } from '$lib/session';
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

describe('users (AC-2)', () => {
  it('shows the display name first with the account id as the secondary detail and the (you) marker', async () => {
    // Arrange
    serve();

    // Act
    render(AdminPage);

    // Assert
    const mine = (await screen.findByText(ME)).closest('tr')!;
    expect(within(mine).getByTestId('user-name')).toHaveTextContent('Marta Hryn');
    expect(within(mine).getByTestId('user-id')).toHaveTextContent(ME);
    expect(mine).toHaveTextContent('(you)');
    const other = screen.getByText(OTHER).closest('tr')!;
    expect(within(other).getByTestId('user-name')).toHaveTextContent('Daria Koval');
    expect(other).not.toHaveTextContent('(you)');
  });

  it('shows the id alone for an account that has no name', async () => {
    // Arrange
    serve({
      [USERS_URL]: json({
        default_quota_bytes: DEFAULT_QUOTA,
        items: [user(OTHER, { display_name: '' })],
      }),
    });

    // Act
    render(AdminPage);

    // Assert
    const row = (await screen.findByText(OTHER)).closest('tr')!;
    expect(within(row).queryByTestId('user-name')).not.toBeInTheDocument();
  });
});

describe('quotas (AC-5)', () => {
  beforeEach(() => navigate('/admin?tab=quotas', { replace: true }));

  it('names people in the picker and the list by display name, with the id as detail and the (you) marker', async () => {
    // Arrange
    serve({
      [USERS_URL]: json({
        default_quota_bytes: DEFAULT_QUOTA,
        items: [
          user(ME, { role: 'admin', limit: 50_000_000_000 }),
          user(OTHER, { limit: 250_000_000_000 }),
        ],
      }),
    });

    // Act
    render(AdminPage);

    // Assert
    const picker = await screen.findByLabelText('User');
    expect(
      within(picker)
        .getAllByRole('option')
        .map((option) => option.textContent?.trim()),
    ).toEqual(['Choose a user', `Marta Hryn (you) · ${ME}`, `Daria Koval · ${OTHER}`]);
    const table = await screen.findByRole('table', { name: 'Per-user quotas' });
    const mine = within(table).getByText(ME).closest('tr')!;
    expect(mine).toHaveTextContent('Marta Hryn');
    expect(mine).toHaveTextContent('(you)');
    expect(within(table).getByRole('button', { name: 'Edit quota of Daria Koval' })).toBeVisible();
  });
});
