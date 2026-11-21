import { describe, expect, it } from 'vitest';
import type { ApiError, Result, ShareOpen } from '$lib/api';
import { PublicShareState } from './publicShare.svelte';

const OPEN: ShareOpen = {
  name: 'f.jpg',
  size: 10,
  media_type: 'image/jpeg',
  expires_at: '2026-11-30T00:00:00Z',
};

function failure(
  status: number | null,
  code: string,
  retryAfterSeconds: number | null = null,
): Result<never> {
  const error: ApiError = {
    kind: status === null ? 'network' : 'http',
    status,
    code,
    title: null,
    requestId: null,
    retryAfterSeconds,
    minVersion: null,
    updateUrl: null,
    needBlocks: null,
  };
  return { ok: false, error };
}

function setup(
  open: Result<ShareOpen>[],
  verify: Result<void>[] = [],
  token: string | null = 'tok',
) {
  const calls: string[] = [];
  const api = {
    getPublicShare: async () => {
      calls.push('open');
      return open.shift() as Result<ShareOpen>;
    },
    postSharePasswordVerification: async (_token: string, password: string) => {
      calls.push(`verify:${password}`);
      return verify.shift() as Result<void>;
    },
  };
  return { calls, state: new PublicShareState(api as never, token, () => 1_000) };
}

describe('what a visitor can see of a link', () => {
  it.each([
    ['not found', failure(404, 'not_found')],
    ['a server fault', failure(500, 'internal')],
    ['no answer', failure(null, 'network')],
  ])('ends in the one unavailable phase for %s', async (_name, answer) => {
    // Arrange
    const { state } = setup([answer]);

    // Act
    await state.open();

    // Assert
    expect(state.phase).toBe('unavailable');
    expect(state.share).toBeNull();
  });

  it('is unavailable without asking when the address carries no token', async () => {
    // Arrange
    const { state, calls } = setup([], [], null);

    // Act
    await state.open();

    // Assert
    expect(state.phase).toBe('unavailable');
    expect(calls).toEqual([]);
  });

  it('holds no file data while the password is missing', async () => {
    // Arrange
    const { state } = setup([failure(401, 'password_required')]);

    // Act
    await state.open();

    // Assert
    expect(state.phase).toBe('password');
    expect(state.share).toBeNull();
  });

  it('shows a wrong password as one message and stays locked', async () => {
    // Arrange
    const { state, calls } = setup(
      [failure(401, 'password_required')],
      [failure(401, 'password_required')],
    );
    await state.open();

    // Act
    await state.unlock('nope');

    // Assert
    expect(state.phase).toBe('password');
    expect(state.failure).toBe('wrong');
    expect(state.share).toBeNull();
    expect(calls).toEqual(['open', 'verify:nope']);
  });

  it('opens the file after the right password and keeps the password nowhere', async () => {
    // Arrange
    const { state } = setup(
      [failure(401, 'password_required'), { ok: true, value: OPEN }],
      [{ ok: true, value: undefined }],
    );
    await state.open();

    // Act
    await state.unlock('right');

    // Assert
    expect(state.phase).toBe('available');
    expect(state.share).toEqual(OPEN);
    expect(JSON.stringify(state)).not.toContain('right');
  });

  it('reports a password that was accepted while the server still hides the file', async () => {
    // Arrange
    const { state } = setup(
      [failure(401, 'password_required'), failure(401, 'password_required')],
      [{ ok: true, value: undefined }],
    );
    await state.open();

    // Act
    await state.unlock('right');

    // Assert
    expect(state.phase).toBe('password');
    expect(state.failure).toBe('unseen');
    expect(state.share).toBeNull();
  });

  it('blocks Unlock until Retry-After has passed and sends nothing meanwhile', async () => {
    // Arrange
    const { state, calls } = setup(
      [failure(401, 'password_required')],
      [failure(429, 'rate_limited', 42)],
    );
    await state.open();

    // Act
    await state.unlock('guess');
    await state.unlock('guess again');

    // Assert
    expect(state.blockedUntil).toBe(1_000 + 42_000);
    expect(state.failure).toBeNull();
    expect(calls).toEqual(['open', 'verify:guess']);

    // Act (the countdown ended)
    state.endBlock();

    // Assert
    expect(state.blockedUntil).toBeNull();
  });

  it('shows the limited phase when the first look is rate limited', async () => {
    // Arrange
    const { state } = setup([failure(429, 'rate_limited', 5), { ok: true, value: OPEN }]);

    // Act
    await state.open();

    // Assert
    expect(state.phase).toBe('limited');
    expect(state.blockedUntil).toBe(6_000);

    // Act (try again after the countdown)
    state.endBlock();
    await state.open();

    // Assert
    expect(state.phase).toBe('available');
    expect(state.blockedUntil).toBeNull();
  });
});
