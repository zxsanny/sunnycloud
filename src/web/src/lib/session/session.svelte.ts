import { api, type Account } from '$lib/api';

export type SessionStatus = 'idle' | 'loading' | 'ready' | 'signed-out' | 'failed';

/** The signed-in account (the session profile), read from the server; the cookie itself is never touched. */
class SessionState {
  status = $state<SessionStatus>('idle');
  account = $state<Account | null>(null);

  async load() {
    this.status = 'loading';
    const result = await api.getAccountsMe();
    if (result.ok) {
      this.account = result.value;
      this.status = 'ready';
    } else {
      this.account = null;
      this.status = result.error.status === 401 ? 'signed-out' : 'failed';
    }
  }

  async signOut(): Promise<boolean> {
    const result = await api.deleteSessionsCurrent();
    if (result.ok) {
      this.account = null;
      this.status = 'signed-out';
    }
    return result.ok;
  }
}

export const session = new SessionState();
