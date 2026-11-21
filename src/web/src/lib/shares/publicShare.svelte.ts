import type { Api, ShareOpen } from '$lib/api';

export type SharePhase = 'loading' | 'available' | 'password' | 'limited' | 'unavailable';

type PublicApi = Pick<Api, 'getPublicShare' | 'postSharePasswordVerification'>;

/**
 * What a visitor sees of one link. Every failure except "password needed" and "too many attempts" is the same
 * `unavailable` phase: the server does not tell an unknown, expired or revoked link apart, and neither does this page.
 * Name, size, type and expiry exist in `share` only after the server answered 200.
 */
export class PublicShareState {
  phase = $state<SharePhase>('loading');
  share = $state<ShareOpen | null>(null);
  /**
   * Set when the last password was refused (`wrong`), could not be checked (`failed`), or was accepted (204) while the
   * server still answers `password_required` to the metadata read (`unseen`).
   */
  failure = $state<'wrong' | 'failed' | 'unseen' | null>(null);
  checking = $state(false);
  /** Epoch milliseconds at which the server's rate limit ends, from `Retry-After`. */
  blockedUntil = $state<number | null>(null);

  constructor(
    private readonly api: PublicApi,
    private readonly token: string | null,
    private readonly clock: () => number = Date.now,
  ) {}

  async open() {
    if (this.token === null) {
      this.phase = 'unavailable';
      return;
    }
    this.phase = 'loading';
    const result = await this.api.getPublicShare(this.token);
    if (result.ok) {
      this.share = result.value;
      this.blockedUntil = null;
      this.phase = 'available';
    } else if (result.error.status === 401 && result.error.code === 'password_required') {
      this.phase = 'password';
    } else if (result.error.status === 429) {
      this.block(result.error.retryAfterSeconds);
      this.phase = 'limited';
    } else {
      this.phase = 'unavailable';
    }
  }

  /** The password goes into the one verification request and is not kept. */
  async unlock(password: string) {
    if (this.token === null || this.checking || this.blockedUntil !== null) return;
    this.checking = true;
    this.failure = null;
    const result = await this.api.postSharePasswordVerification(this.token, password);
    if (result.ok) {
      await this.open();
      // The server took the password but still refuses the metadata read: `GET /public-shares/{token}` does not
      // read the grant cookie, only the content routes do. The page shows nothing it does not know.
      if (this.phase === 'password') this.failure = 'unseen';
    } else if (result.error.status === 401) {
      this.failure = 'wrong';
    } else if (result.error.status === 404) {
      this.phase = 'unavailable';
    } else if (result.error.status === 429) {
      this.block(result.error.retryAfterSeconds);
    } else {
      this.failure = 'failed';
    }
    this.checking = false;
  }

  /** Called by the countdown when `Retry-After` has passed. */
  endBlock() {
    this.blockedUntil = null;
  }

  private block(seconds: number | null) {
    this.blockedUntil = this.clock() + (seconds ?? 0) * 1000;
  }
}
