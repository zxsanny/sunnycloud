import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { account, json, problem, stubServer } from '../../tests/support/http';
import { navigate, route } from '$lib/router';
import { session } from '$lib/session';
import HomePage from './HomePage.svelte';

beforeEach(() => {
  session.status = 'idle';
  session.account = null;
  navigate('/', { replace: true });
});
afterEach(() => vi.unstubAllGlobals());

describe('the signed-in start page', () => {
  it('shows the account name, the provider identity and the storage used', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/accounts/me': json(account) });

    // Act
    render(HomePage);

    // Assert
    expect(await screen.findByText('oidc: marta@example.org')).toBeInTheDocument();
    expect(screen.getByText('Storage: 38.2 GB of 100.0 GB')).toBeInTheDocument();
    expect(screen.getAllByText('Marta Hryn').length).toBeGreaterThan(0);
    expect(screen.getByText('Your files will appear here')).toBeInTheDocument();
  });

  it('marks an administrator', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/accounts/me': json({ ...account, role: 'admin' }) });

    // Act
    render(HomePage);

    // Assert
    expect(await screen.findByText('Admin')).toBeInTheDocument();
  });

  it('offers a retry when the account cannot be loaded', async () => {
    // Arrange
    const user = userEvent.setup();
    let attempt = 0;
    stubServer({
      'GET /api/v1/accounts/me': () => (++attempt === 1 ? problem(500, 'internal') : json(account)),
    });
    render(HomePage);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Retry' }));

    // Assert
    expect(await screen.findByText('oidc: marta@example.org')).toBeInTheDocument();
  });

  it('signs out with the session route and goes to the sign-in page', async () => {
    // Arrange
    const user = userEvent.setup();
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': json(account),
      'DELETE /api/v1/sessions/current': new Response(null, { status: 204 }),
    });
    render(HomePage);
    await screen.findByText('oidc: marta@example.org');

    // Act
    await user.click(screen.getByRole('button', { name: 'Sign out' }));

    // Assert
    await waitFor(() => expect(route.path).toBe('/login'));
    expect(calls.map((c) => `${c.method} ${c.url.pathname}`)).toContain(
      'DELETE /api/v1/sessions/current',
    );
  });
});
