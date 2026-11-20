import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { account, json, problem, stubServer } from '../../tests/support/http';
import { followExternal, navigate, route } from '$lib/router';
import { session } from '$lib/session';
import SignInPage from './SignInPage.svelte';

vi.mock('$lib/router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/router')>()),
  followExternal: vi.fn(),
}));

const AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth?state=s';
const SIGNED_OUT = problem(401, 'unauthenticated');

function providers(list: { key: string; display_name: string }[], setupRequired = false) {
  return json({ providers: list, setup_required: setupRequired });
}

beforeEach(() => {
  session.status = 'idle';
  session.account = null;
  vi.mocked(followExternal).mockClear();
  navigate('/login', { replace: true });
});
afterEach(() => vi.unstubAllGlobals());

describe('the sign-in page', () => {
  it.each([
    { count: 1, list: [{ key: 'oidc', display_name: 'Google' }] },
    {
      count: 2,
      list: [
        { key: 'github', display_name: 'GitHub' },
        { key: 'oidc', display_name: 'Google' },
      ],
    },
    {
      count: 3,
      list: [
        { key: 'oidc', display_name: 'Google' },
        { key: 'apple', display_name: 'Apple' },
        { key: 'github', display_name: 'GitHub' },
      ],
    },
  ])('shows exactly the $count providers the API lists, in the API order', async ({ list }) => {
    // Arrange
    stubServer({
      'GET /api/v1/auth/providers': providers(list),
      'GET /api/v1/accounts/me': SIGNED_OUT,
    });

    // Act
    render(SignInPage);

    // Assert
    const buttons = await screen.findAllByRole('button', { name: /^Continue with / });
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(
      list.map((p) => `${p.display_name.slice(0, 1).toUpperCase()}Continue with ${p.display_name}`),
    );
  });

  it('shows the heading and the note about the administrator', async () => {
    // Arrange
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
    });

    // Act
    render(SignInPage);

    // Assert
    expect(screen.getByRole('heading', { name: 'Sign in to sunnycloud' })).toBeInTheDocument();
    expect(
      await screen.findByText(/Your administrator chose these sign-in options/),
    ).toBeInTheDocument();
  });

  it('starts a login attempt for the picked provider and follows the returned authorization URL', async () => {
    // Arrange
    const user = userEvent.setup();
    navigate('/login?return=%2Ffiles%2Fx', { replace: true });
    const { calls } = stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: /Continue with Google/ }));

    // Assert
    const attempt = calls.find((c) => c.method === 'POST');
    expect(attempt?.body).toEqual({
      provider: 'oidc',
      return_to: '/files/x',
      client_kind: 'web',
      invite_token: null,
      setup_token: null,
    });
    expect(followExternal).toHaveBeenCalledExactlyOnceWith(AUTH_URL);
  });

  it('sends the start page as return path when the return parameter points to another host', async () => {
    // Arrange
    const user = userEvent.setup();
    navigate('/login?return=https%3A%2F%2Fevil.test', { replace: true });
    const { calls } = stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: /Continue with Google/ }));

    // Assert
    expect(calls.find((c) => c.method === 'POST')?.body).toMatchObject({ return_to: '/' });
  });

  it('moves to the first-run page, keeping the error, while the server has no accounts', async () => {
    // Arrange
    navigate('/login?error=invitation_required', { replace: true });
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }], true),
      'GET /api/v1/accounts/me': SIGNED_OUT,
    });

    // Act
    render(SignInPage);

    // Assert
    await waitFor(() => expect(route.path).toBe('/setup'));
    expect(route.query.get('error')).toBe('invitation_required');
  });

  it('sends a signed-in user to the return path instead of showing the page', async () => {
    // Arrange
    navigate('/login?return=%2Ffiles%2Fx', { replace: true });
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': json(account),
    });

    // Act
    render(SignInPage);

    // Assert
    await waitFor(() => expect(route.path).toBe('/files/x'));
  });

  it('shows the invite state and carries the invite token only in the request body', async () => {
    // Arrange
    const user = userEvent.setup();
    navigate('/invite#sc_inv_secret123', { replace: true });
    const { calls } = stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: /Continue with Google/ }));

    // Assert
    expect(screen.getByText('You have an invite.')).toBeInTheDocument();
    expect(calls.find((c) => c.method === 'POST')?.body).toMatchObject({
      invite_token: 'sc_inv_secret123',
    });
    expect(calls.every((c) => !c.url.href.includes('secret123'))).toBe(true);
    expect(JSON.stringify({ ...localStorage, ...sessionStorage })).not.toContain('secret123');
  });

  it.each([
    { reason: 'invitation_required', text: 'This account is not invited.' },
    { reason: 'account_not_active', text: 'This account is deactivated.' },
    { reason: 'something_new', text: 'Sign-in did not complete.' },
  ])('shows the "$text" state for the error reason $reason', async ({ reason, text }) => {
    // Arrange
    navigate(`/login?error=${reason}`, { replace: true });
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
    });

    // Act
    render(SignInPage);

    // Assert
    expect(await screen.findByText(text)).toBeInTheDocument();
    expect(await screen.findByRole('button', { name: /Continue with Google/ })).toBeInTheDocument();
  });

  it('shows an inline message when the attempt is refused with a code the page owns', async () => {
    // Arrange
    const user = userEvent.setup();
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
      'POST /api/v1/auth/login-attempts': problem(400, 'invalid_request'),
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: /Continue with Google/ }));

    // Assert
    expect(await screen.findByText('Sign-in could not start.')).toBeInTheDocument();
    expect(followExternal).not.toHaveBeenCalled();
  });

  it('leaves a rate limit to the app-level notice and shows no inline message', async () => {
    // Arrange
    const user = userEvent.setup();
    stubServer({
      'GET /api/v1/auth/providers': providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
      'POST /api/v1/auth/login-attempts': problem(429, 'rate_limited', { retry_after: 5 }),
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: /Continue with Google/ }));

    // Assert
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Continue with Google/ })).toBeEnabled(),
    );
    expect(screen.queryByText('Sign-in could not start.')).toBeNull();
  });

  it('offers a retry when the provider list cannot be loaded', async () => {
    // Arrange
    const user = userEvent.setup();
    let attempt = 0;
    stubServer({
      'GET /api/v1/auth/providers': () =>
        ++attempt === 1
          ? problem(500, 'internal')
          : providers([{ key: 'oidc', display_name: 'Google' }]),
      'GET /api/v1/accounts/me': SIGNED_OUT,
    });
    render(SignInPage);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Retry' }));

    // Assert
    expect(await screen.findByRole('button', { name: /Continue with Google/ })).toBeInTheDocument();
  });
});
