import { afterEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer } from '../../../tests/support/http';
import { beginLogin } from './login';

afterEach(() => vi.unstubAllGlobals());

const AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth?state=s';

describe('beginLogin', () => {
  it('posts provider, a relative return path and the web client kind, and returns the authorization URL', async () => {
    // Arrange
    const { calls } = stubServer({
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });

    // Act
    const outcome = await beginLogin({ provider: 'oidc', returnTo: '/files/x' });

    // Assert
    expect(calls[0]?.body).toEqual({
      provider: 'oidc',
      return_to: '/files/x',
      client_kind: 'web',
      invite_token: null,
      setup_token: null,
    });
    expect(outcome).toEqual({ ok: true, authorizationUrl: AUTH_URL });
  });

  it('replaces a hostile return path with the start page', async () => {
    // Arrange
    const { calls } = stubServer({
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });

    // Act
    await beginLogin({ provider: 'oidc', returnTo: 'https://evil.test' });

    // Assert
    expect(calls[0]?.body).toMatchObject({ return_to: '/' });
  });

  it('carries the setup token and the invite token only in the request body', async () => {
    // Arrange
    const { calls } = stubServer({
      'POST /api/v1/auth/login-attempts': json({ authorization_url: AUTH_URL }, 201),
    });

    // Act
    await beginLogin({
      provider: 'oidc',
      returnTo: '/',
      setupToken: 'SETUP-SECRET',
      inviteToken: 'INVITE-SECRET',
    });

    // Assert
    expect(calls[0]?.body).toMatchObject({
      setup_token: 'SETUP-SECRET',
      invite_token: 'INVITE-SECRET',
    });
    expect(calls[0]?.url.href).not.toContain('SECRET');
  });

  it('refuses an authorization URL that is not http(s)', async () => {
    // Arrange
    stubServer({
      'POST /api/v1/auth/login-attempts': json({ authorization_url: 'javascript:alert(1)' }, 201),
    });

    // Act
    const outcome = await beginLogin({ provider: 'oidc', returnTo: '/' });

    // Assert
    expect(outcome).toEqual({ ok: false, error: null });
  });

  it('returns the typed error of a refused attempt', async () => {
    // Arrange
    stubServer({ 'POST /api/v1/auth/login-attempts': problem(400, 'invalid_request') });

    // Act
    const outcome = await beginLogin({ provider: 'nope', returnTo: '/' });

    // Assert
    expect(outcome).toMatchObject({ ok: false, error: { status: 400, code: 'invalid_request' } });
  });
});
