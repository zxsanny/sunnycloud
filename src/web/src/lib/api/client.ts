import createClient, { type Middleware } from 'openapi-fetch';
import { type ApiError, type Result, networkError, parseHttpError } from './errors';
import type { components, paths } from './generated/schema';
import { type ServerLimits, serverLimitsOf } from './limits';
import { uuidV7 } from './requestId';
import { withRenewal } from './renewal';

export type Account = components['schemas']['Account'];
export type ProviderList = components['schemas']['ProviderList'];
export type LoginAttemptRequest = components['schemas']['LoginAttemptRequest'];
export type Quota = components['schemas']['Quota'];
export type NodeView = components['schemas']['NodeView'];
export type NodeListing = components['schemas']['NodeListing'];
export type StartUploadRequest = components['schemas']['StartUploadRequest'];
export type StartUploadOutcome = components['schemas']['StartUploadOutcome'];
export type UploadState = components['schemas']['UploadState'];
export type Staged = components['schemas']['Staged'];
export type CommitUploadRequest = components['schemas']['CommitUploadRequest'];
export type Committed = components['schemas']['Committed'];
export type BatchCommitRequest = components['schemas']['BatchCommitRequest'];
export type BatchCommitResult = components['schemas']['BatchCommitResult'];
export type ThumbnailStates = components['schemas']['ThumbnailStates'];
export type RevisionView = components['schemas']['RevisionView'];
export type ShareOpen = components['schemas']['ShareOpen'];
export type DeviceList = components['schemas']['DeviceList'];
export type IdentityLinkAttemptRequest = components['schemas']['IdentityLinkAttemptRequest'];
export type AccountDeletionAccepted = components['schemas']['AccountDeletionAccepted'];

/** A content route that answers Range: the node's current revision, or one named revision. */
export type ContentRoute = { kind: 'node' | 'revision'; id: string };

/** The answer of one ranged content request: the status (206 for a range) and the body as a stream. */
export interface ContentAnswer {
  status: number;
  body: ReadableStream<Uint8Array> | null;
}

type Raw<T> = { data?: T; error?: unknown; response: Response };

export interface ApiOptions {
  /** Receives the finished Request (openapi-fetch's contract); the default is the global fetch. */
  fetch?: (request: Request) => Promise<Response>;
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
  const origin = options.origin ?? globalThis.location.origin;
  const client = createClient<paths>({
    baseUrl: origin,
    credentials: 'same-origin',
    fetch: options.fetch ?? withRenewal((request) => globalThis.fetch(request), origin, version),
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

  /** Raw bytes as the request body: the generated type says string, the serializer passes bytes through. */
  const bytes = { bodySerializer: (body: unknown) => body as BodyInit };
  const octets = { 'Content-Type': 'application/octet-stream' };

  return {
    /** True once the server answered 426: nothing more is sent until the page reloads. */
    upgradeRequired: () => upgradeRequired !== null,
    /** Runs the error handler for a failure that did not come through this client (a Worker's). */
    report(error: ApiError) {
      fail(error);
    },
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
    deleteAccountsMe: (confirmation: string) =>
      send(() => client.DELETE('/api/v1/accounts/me', { body: { confirmation } })),
    getDevices: () => send(() => client.GET('/api/v1/devices')),
    deleteDevice: (id: string) =>
      send(() => client.DELETE('/api/v1/devices/{id}', { params: { path: { id } } })),
    postIdentityLinkAttempt: (body: IdentityLinkAttemptRequest) =>
      send(() => client.POST('/api/v1/identity-link-attempts', { body })),
    getServerLimits: async (): Promise<Result<ServerLimits>> => {
      const result = await send(() => client.GET('/api/v1/server-info'));
      return result.ok ? { ok: true, value: serverLimitsOf(result.value) } : result;
    },
    getChangesHead: () => send(() => client.GET('/api/v1/changes/head')),
    getNodesRoot: () => send(() => client.GET('/api/v1/nodes/root')),
    getNode: (id: string) =>
      send(() => client.GET('/api/v1/nodes/{id}', { params: { path: { id } } })),
    getRevision: (id: string) =>
      send(() => client.GET('/api/v1/revisions/{id}', { params: { path: { id } } })),
    getPublicShare: (token: string) =>
      send(() => client.GET('/api/v1/public-shares/{token}', { params: { path: { token } } })),
    /** Bytes `start..=end` of a file's content, as a stream: the body is never read into one buffer. */
    getContentRange: (route: ContentRoute, start: number, end: number, signal?: AbortSignal) =>
      send<ContentAnswer>(async () => {
        const init = {
          headers: { Range: `bytes=${start}-${end}` },
          parseAs: 'stream' as const,
          signal,
        };
        const raw =
          route.kind === 'node'
            ? await client.GET('/api/v1/nodes/{node_id}/content', {
                params: { path: { node_id: route.id } },
                ...init,
              })
            : await client.GET('/api/v1/revisions/{revision_id}/content', {
                params: { path: { revision_id: route.id } },
                ...init,
              });
        return {
          error: raw.error,
          response: raw.response,
          data: { status: raw.response.status, body: raw.response.body },
        };
      }),
    getNodeChildren: (id: string, after?: string | null, limit?: number) =>
      send(() =>
        client.GET('/api/v1/nodes/{id}/children', {
          params: { path: { id }, query: { after: after ?? undefined, limit } },
        }),
      ),
    postNode: (parentId: string, name: string) =>
      send(() => client.POST('/api/v1/nodes', { body: { parent_id: parentId, name } })),
    deleteNode: (id: string, baseRevision?: string | null) =>
      send(() =>
        client.DELETE('/api/v1/nodes/{id}', {
          params: { path: { id }, query: { base_revision: baseRevision ?? undefined } },
        }),
      ),
    postThumbnailStates: (contentHashes: string[]) =>
      send(() =>
        client.POST('/api/v1/thumbnail-states', { body: { content_hashes: contentHashes } }),
      ),
    postUploads: (body: StartUploadRequest, signal?: AbortSignal) =>
      send(() => client.POST('/api/v1/uploads', { body, signal })),
    getUpload: (uploadId: string, signal?: AbortSignal) =>
      send(() =>
        client.GET('/api/v1/uploads/{upload_id}', {
          params: { path: { upload_id: uploadId } },
          signal,
        }),
      ),
    deleteUpload: (uploadId: string) =>
      send(() =>
        client.DELETE('/api/v1/uploads/{upload_id}', { params: { path: { upload_id: uploadId } } }),
      ),
    putBlock: (
      uploadId: string,
      index: number,
      block: Uint8Array,
      hashHex: string,
      signal?: AbortSignal,
    ) =>
      send(() =>
        client.PUT('/api/v1/uploads/{upload_id}/blocks/{index}', {
          params: { path: { upload_id: uploadId, index }, header: { 'x-blake3': hashHex } },
          body: block as unknown as string,
          headers: octets,
          signal,
          ...bytes,
        }),
      ),
    postCommits: (body: CommitUploadRequest, signal?: AbortSignal) =>
      send(() => client.POST('/api/v1/commits', { body, signal })),
    putStaged: (key: string, body: Uint8Array, hashHex: string, signal?: AbortSignal) =>
      send(() =>
        client.PUT('/api/v1/staged-uploads/{staged_key}', {
          params: { path: { staged_key: key }, header: { 'x-blake3': hashHex } },
          body: body as unknown as string,
          headers: octets,
          signal,
          ...bytes,
        }),
      ),
    postCommitsBatch: (body: BatchCommitRequest, signal?: AbortSignal) =>
      send(() => client.POST('/api/v1/commits:batch', { body, signal })),
  };
}

export type Api = ReturnType<typeof createApi>;

export const api: Api = createApi();
