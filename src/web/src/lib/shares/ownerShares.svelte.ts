import type { Api, ApiError, CreatedShare, ShareList } from '$lib/api';
import type { ShareTtl } from './expiry';

export type ShareInfo = ShareList['items'][number];

type OwnerApi = Pick<Api, 'getShares' | 'postShare' | 'deleteShare' | 'getServerLimits'>;

export interface NewLink {
  /** Seconds for `expires_in`; undefined leaves it out and the server applies its default. */
  expiresIn: number | undefined;
  /** Null for a link without a password. */
  password: string | null;
}

export interface CreateFailure {
  /** `expiry` is shown next to the expiry field, `form` above the buttons. */
  place: 'expiry' | 'form';
  text: string;
}

function createFailureOf(error: ApiError, sentExpiry: boolean): CreateFailure {
  if (error.status === 422) {
    return { place: 'form', text: 'This item cannot be shared. Links work for files only.' };
  }
  if (error.status === 404) return { place: 'form', text: 'This file no longer exists.' };
  if (error.status === 403) return { place: 'form', text: 'You cannot share this item.' };
  // The only field of the request that the server can refuse with 400 is the expiry (ExpiryOutOfRange).
  if (error.status === 400 && error.code === 'invalid_request' && sentExpiry) {
    return {
      place: 'expiry',
      text: 'The server does not accept this expiry. Choose a shorter time.',
    };
  }
  return { place: 'form', text: 'The link could not be created. Try again.' };
}

/** The links of one file for the owner's dialog. The token exists here only in `created`, from the create answer. */
export class OwnerShares {
  items = $state<ShareInfo[]>([]);
  /** The server's default and longest link lifetime; null until `loadLimits` has an answer. */
  ttl = $state<ShareTtl | null>(null);
  status = $state<'loading' | 'ready' | 'failed'>('loading');
  /** The one-time answer of the last create: the full URL is shown from this and nowhere else. */
  created = $state<{ share: CreatedShare; url: string } | null>(null);
  creating = $state(false);
  failure = $state<CreateFailure | null>(null);
  /** The link whose revoke call is in flight, and the line of one that failed. */
  revoking = $state<string | null>(null);
  revokeFailure = $state<{ id: string; text: string } | null>(null);

  constructor(
    private readonly api: OwnerApi,
    private readonly nodeId: string,
    private readonly origin: string,
  ) {}

  async load() {
    const result = await this.api.getShares(this.nodeId);
    if (result.ok) {
      this.items = result.value.items;
      this.status = 'ready';
    } else {
      this.status = 'failed';
    }
  }

  /** A failed read leaves `ttl` null: the form then offers "Server default" only, and the server still decides. */
  async loadLimits() {
    const result = await this.api.getServerLimits();
    if (result.ok) {
      this.ttl = { defaultTtl: result.value.shareDefaultTtl, maxTtl: result.value.shareMaxTtl };
    }
  }

  async create(link: NewLink) {
    if (this.creating) return;
    this.creating = true;
    this.failure = null;
    const result = await this.api.postShare({
      node_id: this.nodeId,
      ...(link.expiresIn === undefined ? {} : { expires_in: link.expiresIn }),
      ...(link.password === null ? {} : { password: link.password }),
    });
    if (result.ok) {
      this.created = { share: result.value, url: `${this.origin}${result.value.url_path}` };
      await this.load();
    } else {
      this.failure = createFailureOf(result.error, link.expiresIn !== undefined);
    }
    this.creating = false;
  }

  /** One call; the row leaves the list when the server says the link is gone (204, or 404 for an already gone one). */
  async revoke(id: string) {
    if (this.revoking !== null) return;
    this.revoking = id;
    this.revokeFailure = null;
    const result = await this.api.deleteShare(id);
    if (result.ok || result.error.status === 404) {
      this.items = this.items.filter((item) => item.id !== id);
    } else {
      this.revokeFailure = { id, text: 'The link could not be revoked. Try again.' };
    }
    this.revoking = null;
  }
}
