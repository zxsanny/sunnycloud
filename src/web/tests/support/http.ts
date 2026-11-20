import { vi } from 'vitest';

export interface RecordedCall {
  method: string;
  url: URL;
  headers: Headers;
  credentials: RequestCredentials;
  body: unknown;
}

export type Reply = Response | (() => Response | Promise<Response>);

export function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', ...headers },
  });
}

export function problem(
  status: number,
  code: string,
  extra: Record<string, unknown> = {},
  headers: Record<string, string> = {},
): Response {
  const body = {
    type: `urn:sunnycloud:error:${code}`,
    title: code,
    status,
    code,
    request_id: '0190aaaa-0000-7000-8000-000000000001',
    ...extra,
  };
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/problem+json', ...headers },
  });
}

/**
 * Replaces the global fetch with a stub that answers by "METHOD /path". A request nobody planned for
 * fails the test: the unit tests never reach a real server.
 */
export function stubServer(replies: Record<string, Reply>) {
  const calls: RecordedCall[] = [];
  const stub = vi.fn(async (request: Request) => {
    const url = new URL(request.url);
    const text = await request.clone().text();
    calls.push({
      method: request.method,
      url,
      headers: request.headers,
      credentials: request.credentials,
      body: text === '' ? undefined : JSON.parse(text),
    });
    const reply = replies[`${request.method} ${url.pathname}`];
    if (reply === undefined) {
      throw new Error(`unplanned request: ${request.method} ${url.pathname}`);
    }
    return typeof reply === 'function' ? await reply() : reply.clone();
  });
  vi.stubGlobal('fetch', stub);
  return { calls, stub };
}

export const account = {
  id: '0190aaaa-0000-7000-8000-0000000000aa',
  display_name: 'Marta Hryn',
  role: 'user',
  identities: [{ provider: 'oidc', email: 'marta@example.org' }],
  quota: { limit_bytes: null, effective_limit: 100_000_000_000, used: 38_200_000_000, reserved: 0 },
  root_node_id: '0190aaaa-0000-7000-8000-0000000000bb',
};
