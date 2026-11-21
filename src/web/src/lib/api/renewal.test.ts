import { afterEach, describe, expect, it, vi } from 'vitest';
import { account, json, problem, stubServer } from '../../../tests/support/http';
import { createApi } from './client';
import { RENEWAL_PATH, withRenewal } from './renewal';

afterEach(() => vi.unstubAllGlobals());

const origin = 'https://drive.test';
const renewed = () => new Response(null, { status: 204 });

describe('session renewal', () => {
  it('repeats a request answered 401 once, after the refresh cookie is traded', async () => {
    // Arrange
    let answers = 0;
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': () =>
        answers++ === 0 ? problem(401, 'unauthenticated') : json(account),
      [`POST ${RENEWAL_PATH}`]: renewed(),
    });

    // Act
    const result = await createApi({ origin }).getAccountsMe();

    // Assert
    expect(result).toEqual({ ok: true, value: account });
    expect(calls.map((c) => `${c.method} ${c.url.pathname}`)).toEqual([
      'GET /api/v1/accounts/me',
      `POST ${RENEWAL_PATH}`,
      'GET /api/v1/accounts/me',
    ]);
  });

  it('sends the renewal with the client header, same-origin, and no body', async () => {
    // Arrange
    let answers = 0;
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': () =>
        answers++ === 0 ? problem(401, 'unauthenticated') : json(account),
      [`POST ${RENEWAL_PATH}`]: renewed(),
    });

    // Act
    await createApi({ origin, version: '1.2.3' }).getAccountsMe();

    // Assert
    const renewal = calls[1];
    expect([
      renewal?.credentials,
      renewal?.headers.get('X-Sunnycloud-Client'),
      renewal?.body,
    ]).toEqual(['same-origin', 'web/1.2.3', undefined]);
  });

  it('answers with the first 401 when the renewal is refused, so the app goes to sign-in', async () => {
    // Arrange
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': problem(401, 'unauthenticated'),
      [`POST ${RENEWAL_PATH}`]: problem(401, 'unauthenticated'),
    });

    // Act
    const result = await createApi({ origin }).getAccountsMe();

    // Assert
    expect([result.ok, calls.length]).toEqual([false, 2]);
    expect(!result.ok && result.error.status).toBe(401);
  });

  it('repeats nothing and renews nothing for a 401 that is not unauthenticated', async () => {
    // Arrange
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': problem(401, 'password_required'),
    });

    // Act
    await createApi({ origin }).getAccountsMe();

    // Assert
    expect(calls).toHaveLength(1);
  });

  it('shares one renewal between calls that fail together', async () => {
    // Arrange
    const seen = new Set<string>();
    const { calls } = stubServer({
      'GET /api/v1/accounts/me': () =>
        seen.has('renewed') ? json(account) : problem(401, 'unauthenticated'),
      [`POST ${RENEWAL_PATH}`]: () => {
        seen.add('renewed');
        return renewed();
      },
    });
    const api = createApi({ origin });

    // Act
    const results = await Promise.all([
      api.getAccountsMe(),
      api.getAccountsMe(),
      api.getAccountsMe(),
    ]);

    // Assert
    expect(results.every((r) => r.ok)).toBe(true);
    expect(calls.filter((c) => c.url.pathname === RENEWAL_PATH)).toHaveLength(1);
  });

  it('never renews for the renewal route itself or for a sign-in attempt', async () => {
    // Arrange
    const sent: string[] = [];
    const send = withRenewal(
      async (request) => {
        sent.push(new URL(request.url).pathname);
        return problem(401, 'unauthenticated');
      },
      origin,
      '1.0.0',
    );

    // Act
    await send(new Request(`${origin}${RENEWAL_PATH}`, { method: 'POST' }));
    await send(new Request(`${origin}/api/v1/auth/login-attempts`, { method: 'POST', body: '{}' }));

    // Assert
    expect(sent).toEqual([RENEWAL_PATH, '/api/v1/auth/login-attempts']);
  });

  it('keeps the body of a repeated request', async () => {
    // Arrange
    let answers = 0;
    const { calls } = stubServer({
      'POST /api/v1/nodes': () =>
        answers++ === 0 ? problem(401, 'unauthenticated') : json({ id: 'n' }, 201),
      [`POST ${RENEWAL_PATH}`]: renewed(),
    });

    // Act
    await createApi({ origin }).postNode('p', 'x');

    // Assert
    const [first, , second] = calls;
    expect([first?.body, second?.body]).toEqual([
      { parent_id: 'p', name: 'x' },
      { parent_id: 'p', name: 'x' },
    ]);
  });
});
