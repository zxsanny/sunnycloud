import { api, type ApiError } from '$lib/api';
import { isFollowableUrl, safeReturnPath } from './returnPath';

export interface LoginStart {
  provider: string;
  returnTo: string | null;
  /** Tokens travel only in this request body, never in a URL or in storage. */
  inviteToken?: string | null;
  setupToken?: string | null;
}

/** `error: null` means the server answered with a URL the app refuses to follow. */
export type LoginOutcome =
  { ok: true; authorizationUrl: string } | { ok: false; error: ApiError | null };

export async function beginLogin(start: LoginStart): Promise<LoginOutcome> {
  const result = await api.postLoginAttempt({
    provider: start.provider,
    return_to: safeReturnPath(start.returnTo),
    client_kind: 'web',
    invite_token: start.inviteToken ?? null,
    setup_token: start.setupToken ?? null,
  });
  if (!result.ok) return { ok: false, error: result.error };
  const url = result.value.authorization_url;
  return isFollowableUrl(url) ? { ok: true, authorizationUrl: url } : { ok: false, error: null };
}
