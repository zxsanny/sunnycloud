import { describe, expect, it } from 'vitest';
import { type ApiError, isAppLevelError, networkError, parseHttpError } from './errors';

function respond(status: number, headers: Record<string, string> = {}): Response {
  return new Response(null, { status, headers });
}

function http(status: number, code: string): ApiError {
  return { ...networkError(), kind: 'http', status, code };
}

describe('parseHttpError', () => {
  it('maps a known problem body to status, code and the request id of the header', () => {
    // Arrange
    const body = { code: 'name_conflict', title: 'name conflict', request_id: 'body-id' };

    // Act
    const error = parseHttpError(respond(409, { 'X-Request-Id': 'header-id' }), body);

    // Assert
    expect(error).toMatchObject({
      kind: 'http',
      status: 409,
      code: 'name_conflict',
      title: 'name conflict',
      requestId: 'header-id',
    });
  });

  it('reads retry-after from a 429 problem body', () => {
    // Act
    const error = parseHttpError(respond(429), { code: 'rate_limited', retry_after: 7 });

    // Assert
    expect(error).toMatchObject({ status: 429, code: 'rate_limited', retryAfterSeconds: 7 });
  });

  it('falls back to the Retry-After header when the body has none', () => {
    // Act
    const error = parseHttpError(respond(503, { 'Retry-After': '12' }), { code: 'overloaded' });

    // Assert
    expect(error.retryAfterSeconds).toBe(12);
  });

  it('reads the upgrade fields of a 426', () => {
    // Act
    const error = parseHttpError(respond(426), {
      code: 'upgrade_required',
      min_version: '1.2.0',
      update_url: 'https://example.org/update',
    });

    // Assert
    expect(error).toMatchObject({ minVersion: '1.2.0', updateUrl: 'https://example.org/update' });
  });

  it('reads the missing block indexes of need_blocks', () => {
    // Act
    const error = parseHttpError(respond(409), { code: 'need_blocks', need_blocks: [3, 5] });

    // Assert
    expect(error.needBlocks).toEqual([3, 5]);
  });

  it('turns an unknown code into internal', () => {
    // Act
    const error = parseHttpError(respond(400), { code: 'brand_new_code' });

    // Assert
    expect(error.code).toBe('internal');
  });

  it('turns an HTML body of a proxy error into internal and keeps the request id of the header', () => {
    // Act
    const error = parseHttpError(
      respond(502, { 'X-Request-Id': 'r-9' }),
      '<html>Bad gateway</html>',
    );

    // Assert
    expect(error).toMatchObject({ status: 502, code: 'internal', requestId: 'r-9', title: null });
  });
});

describe('isAppLevelError', () => {
  it.each([
    { name: 'a dropped connection', error: networkError(), expected: true },
    { name: '401 unauthenticated', error: http(401, 'unauthenticated'), expected: true },
    { name: '401 password_required', error: http(401, 'password_required'), expected: false },
    { name: '426 upgrade_required', error: http(426, 'upgrade_required'), expected: true },
    { name: '429 rate_limited', error: http(429, 'rate_limited'), expected: true },
    { name: '507 quota_exceeded', error: http(507, 'quota_exceeded'), expected: true },
    { name: '500 internal', error: http(500, 'internal'), expected: true },
    { name: 'an unknown code on a 400', error: http(400, 'internal'), expected: true },
    { name: '409 name_conflict', error: http(409, 'name_conflict'), expected: false },
    { name: '400 invalid_request', error: http(400, 'invalid_request'), expected: false },
  ])('$name: app-level is $expected', ({ error, expected }) => {
    // Act
    const result = isAppLevelError(error);

    // Assert
    expect(result).toBe(expected);
  });
});
