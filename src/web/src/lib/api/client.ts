import createClient, { type Middleware } from 'openapi-fetch';
import { type ApiError, type Result, networkError, parseHttpError } from './errors';
import type { components, paths } from './generated/schema';
import { uuidV7 } from './requestId';

export type Account = components['schemas']['Account'];
export type ProviderList = components['schemas']['ProviderList'];
export type LoginAttemptRequest = components['schemas']['LoginAttemptRequest'];
export type Quota = components['schemas']['Quota'];

type Raw<T> = { data?: T; error?: unknown; response: Response };

export interface ApiOptions {
  fetch?: typeof fetch;
  /** Same origin as the page. A Request needs an absolute URL outside a browser page (tests). */
  origin?: string;
  version?: string;
}

/**
 * The one way the app talks to the server. Every call returns a Result; nothing throws except
 * programming errors. After a 426 the client sends nothing more until the page is reloaded.
 */
export function createApi(options: ApiOptions = {}) {
  const version = options.version ?? __APP_VERSION__;
  const client = createClient<paths>({
    baseUrl: options.origin ?? globalThis.location.origin,
    credentials: 'same-origin',
    fetch: options.fetch ?? ((request) => globalThis.fetch(request)),
  });
  const headers: Middleware = {
    onRequest({ request }) {
      request.headers.set('X-Request-Id', uuidV7());
      request.headers.set('X-Sunnycloud-Client', `web/${version}`);
      return request;
    },
  };
  client.use(headers);

  let errorHandler: (error: ApiError) => void = () => {};
  let upgradeRequired: ApiError | null = null;

  function fail<T>(error: ApiError): Result<T> {
    if (error.status === 426) upgradeRequired = error;
    errorHandler(error);
    return { ok: false, error };
  }

  async function send<T>(run: () => Promise<Raw<T>>): Promise<Result<T>> {
    if (upgradeRequired !== null) return { ok: false, error: upgradeRequired };
    let raw: Raw<T>;
    try {
      raw = await run();
    } catch (cause) {
      // fetch rejects with TypeError when no answer arrived; an abort is a DOMException.
      if (cause instanceof TypeError || cause instanceof DOMException) {
        return fail(networkError());
      }
      throw cause;
    }
    if (raw.response.ok && raw.error === undefined) {
      return { ok: true, value: raw.data as T };
    }
    return fail(parseHttpError(raw.response, raw.error));
  }

  return {
    /** Called for every failed call, before the result is returned. One handler: the app-level notices. */
    setErrorHandler(handler: (error: ApiError) => void) {
      errorHandler = handler;
    },
    getAuthProviders: () => send(() => client.GET('/api/v1/auth/providers')),
    postLoginAttempt: (body: LoginAttemptRequest) =>
      send(() => client.POST('/api/v1/auth/login-attempts', { body })),
    getAccountsMe: () => send(() => client.GET('/api/v1/accounts/me')),
    getAccountsMeQuota: () => send(() => client.GET('/api/v1/accounts/me/quota')),
    deleteSessionsCurrent: () => send(() => client.DELETE('/api/v1/sessions/current')),
  };
}

export type Api = ReturnType<typeof createApi>;

export const api: Api = createApi();
