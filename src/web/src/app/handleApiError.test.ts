import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer } from '../../tests/support/http';
import { api, type ApiError } from '$lib/api';
import { notices } from '$lib/notices';
import { navigate, route } from '$lib/router';
import { handleApiError } from './handleApiError';

function failure(status: number, code: string, extra: Partial<ApiError> = {}): ApiError {
  return {
    kind: 'http',
    status,
    code,
    title: null,
    requestId: null,
    retryAfterSeconds: null,
    minVersion: null,
    updateUrl: null,
    needBlocks: null,
    ...extra,
  };
}

beforeEach(() => {
  notices.items = [];
  navigate('/', { replace: true });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the app-level error mapping', () => {
  it('sends a 401 on a signed-in page to sign-in with the relative return path and says why', () => {
    // Arrange
    navigate('/files?folder=a', { replace: true });

    // Act
    handleApiError(failure(401, 'unauthenticated'));

    // Assert
    expect(route.path).toBe('/login');
    expect(route.query.get('return')).toBe('/files?folder=a');
    expect(notices.items.map((n) => n.title)).toEqual(['Your session ended.']);
  });

  it('leaves a 401 alone on the sign-in page, which handles it itself', () => {
    // Arrange
    navigate('/login', { replace: true });

    // Act
    handleApiError(failure(401, 'unauthenticated'));

    // Assert
    expect(route.path).toBe('/login');
    expect(notices.items).toEqual([]);
  });

  it('shows a blocking upgrade notice with a reload action for a 426', () => {
    // Act
    handleApiError(failure(426, 'upgrade_required'));

    // Assert
    expect(notices.items).toMatchObject([
      { title: 'A newer version of sunnycloud is required.', action: 'reload', variant: 'warning' },
    ]);
    expect(notices.items[0]?.dismissible).toBeUndefined();
  });

  it('shows a countdown from retry-after for a 429', () => {
    // Arrange
    vi.useFakeTimers({ now: 1_000_000 });

    // Act
    handleApiError(failure(429, 'rate_limited', { retryAfterSeconds: 7 }));

    // Assert
    expect(notices.items).toMatchObject([{ title: 'Too many requests.', retryAt: 1_007_000 }]);
  });

  it('shows the quota numbers from the quota view for a 507', async () => {
    // Arrange
    stubServer({
      'GET /api/v1/accounts/me/quota': json({
        limit_bytes: null,
        effective_limit: 100_000_000_000,
        used: 100_000_000_000,
        reserved: 0,
      }),
    });

    // Act
    handleApiError(failure(507, 'quota_exceeded'));

    // Assert
    await vi.waitFor(() => expect(notices.items).toHaveLength(1));
    expect(notices.items[0]?.title).toBe('Storage quota reached.');
    expect(notices.items[0]?.text).toContain('100.0 GB of 100.0 GB');
  });

  it('shows the request id of a server error for copying and never a raw body', () => {
    // Act
    handleApiError(failure(500, 'internal', { requestId: 'r-9' }));

    // Assert
    expect(notices.items).toMatchObject([
      { variant: 'danger', title: 'Something went wrong on the server.', requestId: 'r-9' },
    ]);
  });

  it('shows a retry hint without an id for a dropped connection', () => {
    // Act
    handleApiError({ ...failure(0, 'network'), kind: 'network', status: null });

    // Assert
    expect(notices.items).toMatchObject([{ title: 'Cannot reach the server.' }]);
    expect(notices.items[0]?.requestId).toBeUndefined();
  });

  it('returns a code the page handles itself without raising a notice', () => {
    // Act
    handleApiError(failure(409, 'name_conflict'));
    handleApiError(failure(401, 'password_required'));

    // Assert
    expect(notices.items).toEqual([]);
    expect(route.path).toBe('/');
  });

  it('replaces a notice that has the same key instead of stacking them', () => {
    // Act
    handleApiError(failure(500, 'internal', { requestId: 'r-1' }));
    handleApiError(failure(500, 'internal', { requestId: 'r-2' }));

    // Assert
    expect(notices.items).toHaveLength(1);
    expect(notices.items[0]?.requestId).toBe('r-2');
  });

  it('is what the real client calls: a 401 from the account call lands on sign-in', async () => {
    // Arrange
    stubServer({ 'GET /api/v1/accounts/me': problem(401, 'unauthenticated') });
    api.setErrorHandler(handleApiError);

    // Act
    const result = await api.getAccountsMe();

    // Assert
    expect(result.ok).toBe(false);
    expect(route.path).toBe('/login');
  });
});
