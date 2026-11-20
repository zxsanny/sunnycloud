import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer } from '../../tests/support/http';
import { followExternal, navigate, route } from '$lib/router';
import SetupPage from './SetupPage.svelte';

vi.mock('$lib/router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/router')>()),
  followExternal: vi.fn(),
}));

const AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth?state=s';
const FIRST_RUN = json({
  providers: [{ key: 'oidc', display_name: 'Google' }],
  setup_required: true,
});

beforeEach(() => {
  vi.mocked(followExternal).mockClear();
  navigate('/setup', { replace: true });
});
afterEach(() => vi.unstubAllGlobals());

describe('the first-run setup page', () => {
  it('shows the first step with the token field and the command to print a new token', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/auth/providers': FIRST_RUN });

    // Act
    render(SetupPage);

    // Assert
    expect(await screen.findByLabelText('Setup token')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Set up this server' })).toBeInTheDocument();
    expect(
      screen.getByText('docker compose exec api sunnycloud admin setup-token'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
  });

  it('redirects to sign-in once an account exists', async () => {
    // Arrange
    stubServer({
      'GET /api/v1/auth/providers': json({
        providers: [{ key: 'oidc', display_name: 'Google' }],
        setup_required: false,
      }),
    });

    // Act
    render(SetupPage);

    // Assert
    await waitFor(() => expect(route.path).toBe('/login'));
  });

  it('marks the field invalid with the neutral token message after a rejected token', async () => {
    // Arrange
    navigate('/setup?error=invitation_required', { replace: true });
    stubServer({ 'GET /api/v1/auth/providers': FIRST_RUN });

    // Act
    render(SetupPage);

    // Assert
    const field = await screen.findByLabelText('Setup token');
    expect(field).toHaveAttribute('aria-invalid', 'true');
    expect(
      screen.getByText(
        'This setup token is not valid. A token works once and changes when the server restarts.',
      ),
    ).toBeInTheDocument();
  });

  it('moves to step 2 without any server call and shows the provider buttons', async () => {
    // Arrange
    const user = userEvent.setup();
    const { calls } = stubServer({ 'GET /api/v1/auth/providers': FIRST_RUN });
    render(SetupPage);

    // Act
    await user.type(await screen.findByLabelText('Setup token'), 'TOKEN-1');
    await user.click(screen.getByRole('button', { name: 'Continue' }));

    // Assert
    expect(
      screen.getByText('Now sign in with the account that will be the administrator.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Continue with Google/ })).toBeInTheDocument();
    expect(calls).toHaveLength(1);
  });

  it('sends the setup token only in the login attempt body and keeps it out of URLs and storage', async () => {
    // Arrange
    const user = userEvent.setup();
    const { calls } = stubServer({
      'GET /api/v1/auth/providers': FIRST_RUN,
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });
    render(SetupPage);
    await user.type(await screen.findByLabelText('Setup token'), '  TOKEN-SECRET  ');
    await user.click(screen.getByRole('button', { name: 'Continue' }));

    // Act
    await user.click(screen.getByRole('button', { name: /Continue with Google/ }));

    // Assert
    const attempt = calls.find((c) => c.method === 'POST');
    expect(attempt?.body).toEqual({
      provider: 'oidc',
      return_to: '/',
      client_kind: 'web',
      invite_token: null,
      setup_token: 'TOKEN-SECRET',
    });
    expect(calls.every((c) => !c.url.href.includes('SECRET'))).toBe(true);
    expect(globalThis.location.href).not.toContain('SECRET');
    expect(JSON.stringify({ ...localStorage, ...sessionStorage })).not.toContain('SECRET');
    expect(followExternal).toHaveBeenCalledExactlyOnceWith(AUTH_URL);
  });

  it('copies the setup command', async () => {
    // Arrange
    const user = userEvent.setup();
    stubServer({ 'GET /api/v1/auth/providers': FIRST_RUN });
    render(SetupPage);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Copy command' }));

    // Assert
    expect(await navigator.clipboard.readText()).toBe(
      'docker compose exec api sunnycloud admin setup-token',
    );
  });

  it('offers a retry when the provider list cannot be loaded', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/auth/providers': problem(500, 'internal') });

    // Act
    render(SetupPage);

    // Assert
    expect(await screen.findByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });
});
