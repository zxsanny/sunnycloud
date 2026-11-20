import { afterEach, describe, expect, it, vi } from 'vitest';
import { account, json, problem, stubServer } from '../../../tests/support/http';
import { createApi } from './client';
import type { ApiError } from './errors';

afterEach(() => vi.unstubAllGlobals());

describe('the API client wrapper', () => {
  it('sends a same-origin request with a fresh request id and the client header', async () => {
    // Arrange
    const { calls } = stubServer({ 'GET /api/v1/accounts/me': json(account) });
    const api = createApi({ version: '9.8.7' });

    // Act
    await api.getAccountsMe();
    await api.getAccountsMe();

    // Assert
    const [first, second] = calls;
    expect(first?.credentials).toBe('same-origin');
    expect(first?.url.origin).toBe(globalThis.location.origin);
    expect(first?.headers.get('X-Sunnycloud-Client')).toBe('web/9.8.7');
    expect(first?.headers.get('X-Request-Id')).toMatch(/^[0-9a-f-]{36}$/);
    expect(first?.headers.get('X-Request-Id')).not.toBe(second?.headers.get('X-Request-Id'));
  });

  it('never adds an Authorization or a CSRF header', async () => {
    // Arrange
    const { calls } = stubServer({
      'DELETE /api/v1/sessions/current': new Response(null, { status: 204 }),
    });

    // Act
    await createApi().deleteSessionsCurrent();

    // Assert
    const names = [...(calls[0]?.headers.keys() ?? [])];
    expect(names.filter((n) => /authorization|csrf|xsrf/i.test(n))).toEqual([]);
  });

  it('returns the parsed body of a successful call', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/accounts/me': json(account) });

    // Act
    const result = await createApi().getAccountsMe();

    // Assert
    expect(result).toEqual({ ok: true, value: account });
  });

  it('returns a success with no body for a 204', async () => {
    // Arrange
    stubServer({ 'DELETE /api/v1/sessions/current': new Response(null, { status: 204 }) });

    // Act
    const result = await createApi().deleteSessionsCurrent();

    // Assert
    expect(result.ok).toBe(true);
  });

  it('sends the login attempt as a JSON body', async () => {
    // Arrange
    const { calls } = stubServer({
      'POST /api/v1/auth/login-attempts': json({ authorization_url: 'https://idp.test/auth' }, 201),
    });
    const body = { provider: 'oidc', return_to: '/', client_kind: 'web' as const };

    // Act
    const result = await createApi().postLoginAttempt(body);

    // Assert
    expect(calls[0]?.body).toEqual(body);
    expect(result).toEqual({ ok: true, value: { authorization_url: 'https://idp.test/auth' } });
  });

  it('turns a problem body into an ApiError with status, code and request id', async () => {
    // Arrange
    stubServer({
      'GET /api/v1/accounts/me': problem(
        429,
        'rate_limited',
        { retry_after: 7 },
        { 'X-Request-Id': 'r-1' },
      ),
    });

    // Act
    const result = await createApi().getAccountsMe();

    // Assert
    expect(result).toMatchObject({
      ok: false,
      error: {
        kind: 'http',
        status: 429,
        code: 'rate_limited',
        retryAfterSeconds: 7,
        requestId: 'r-1',
      },
    });
  });

  it('turns a 502 with an HTML body into an internal error that carries the request id', async () => {
    // Arrange
    stubServer({
      'GET /api/v1/accounts/me': new Response('<html>Bad gateway</html>', {
        status: 502,
        headers: { 'Content-Type': 'text/html', 'X-Request-Id': 'r-9' },
      }),
    });

    // Act
    const result = await createApi().getAccountsMe();

    // Assert
    expect(result).toMatchObject({
      ok: false,
      error: { status: 502, code: 'internal', requestId: 'r-9' },
    });
  });

  it('turns a dropped connection into a network error without throwing', async () => {
    // Arrange
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('fetch failed')));

    // Act
    const result = await createApi().getAccountsMe();

    // Assert
    expect(result).toMatchObject({
      ok: false,
      error: { kind: 'network', status: null, code: 'network' },
    });
  });

  it('lets a programming error through instead of calling it a network error', async () => {
    // Arrange
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new RangeError('bug')));

    // Act
    const run = createApi().getAccountsMe();

    // Assert
    await expect(run).rejects.toThrow(RangeError);
  });

  it('calls the error handler once for every failed call and never for a success', async () => {
    // Arrange
    const seen: ApiError[] = [];
    stubServer({
      'GET /api/v1/accounts/me': problem(500, 'internal'),
      'GET /api/v1/auth/providers': json({ providers: [], setup_required: false }),
    });
    const api = createApi();
    api.setErrorHandler((error) => seen.push(error));

    // Act
    await api.getAuthProviders();
    await api.getAccountsMe();

    // Assert
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ status: 500, code: 'internal' });
  });

  it('sends nothing more after a 426 until the page is reloaded', async () => {
    // Arrange
    const { stub } = stubServer({
      'GET /api/v1/accounts/me': problem(426, 'upgrade_required', { min_version: '2.0.0' }),
      'GET /api/v1/auth/providers': json({ providers: [], setup_required: false }),
    });
    const api = createApi();

    // Act
    await api.getAccountsMe();
    const later = await api.getAuthProviders();

    // Assert
    expect(stub).toHaveBeenCalledTimes(1);
    expect(later).toMatchObject({ ok: false, error: { status: 426, minVersion: '2.0.0' } });
  });
});
