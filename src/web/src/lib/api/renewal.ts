import { uuidV7 } from './requestId';

export const RENEWAL_PATH = '/api/v1/auth/session-renewals';

type Send = (request: Request) => Promise<Response>;

/** Routes that answer 401 as part of signing in; renewing for them would loop or hide a real refusal. */
const NOT_RENEWED = new Set([RENEWAL_PATH, '/api/v1/auth/login-attempts']);

async function isUnauthenticated(response: Response): Promise<boolean> {
  if (response.status !== 401) return false;
  try {
    const problem: unknown = await response.clone().json();
    return (problem as { code?: unknown } | null)?.code === 'unauthenticated';
  } catch {
    // Not a problem body (a proxy's page): the session is not what failed, so nothing is renewed.
    return false;
  }
}

/**
 * Wraps the fetch of the API client. A request answered 401 `unauthenticated` is repeated once after the
 * browser's refresh cookie has been traded for a new session (the server sets both cookies; the page never
 * sees a token). Calls that fail together share one renewal. When the renewal fails, the first 401 is the answer,
 * so the app's own 401 path (back to sign-in) runs as before. This is a reaction to a refusal, not a timer.
 */
export function withRenewal(send: Send, origin: string, version: string): Send {
  let renewing: Promise<boolean> | null = null;

  function renew(): Promise<boolean> {
    renewing ??= (async () => {
      try {
        const answer = await send(
          new Request(new URL(RENEWAL_PATH, origin), {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'X-Request-Id': uuidV7(), 'X-Sunnycloud-Client': `web/${version}` },
          }),
        );
        return answer.status === 204;
      } catch {
        // No answer arrived: the original 401 stands and the caller's own error path runs.
        return false;
      }
    })().finally(() => {
      renewing = null;
    });
    return renewing;
  }

  return async (request) => {
    const again = request.clone();
    const first = await send(request);
    if (NOT_RENEWED.has(new URL(request.url).pathname) || !(await isUnauthenticated(first))) {
      return first;
    }
    return (await renew()) ? send(again) : first;
  };
}
