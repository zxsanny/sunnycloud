import { PROBLEM_CODES } from './generated/problem-codes';

export type Result<T, E = ApiError> = { ok: true; value: T } | { ok: false; error: E };

export interface ApiError {
  /** 'network' means no HTTP answer at all (dropped connection, DNS, abort). */
  kind: 'http' | 'network';
  status: number | null;
  /** Stable snake_case code; an unknown or missing code is 'internal'. */
  code: string;
  title: string | null;
  requestId: string | null;
  retryAfterSeconds: number | null;
  minVersion: string | null;
  updateUrl: string | null;
  needBlocks: number[] | null;
}

const EMPTY: ApiError = {
  kind: 'http',
  status: null,
  code: 'internal',
  title: null,
  requestId: null,
  retryAfterSeconds: null,
  minVersion: null,
  updateUrl: null,
  needBlocks: null,
};

function text(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function seconds(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** `body` is the parsed problem+json body, or whatever else the server (or a proxy) answered. */
export function parseHttpError(response: Response, body: unknown): ApiError {
  const problem = isRecord(body) ? body : {};
  const code = text(problem.code);
  const needBlocks = Array.isArray(problem.need_blocks)
    ? problem.need_blocks.filter((n): n is number => Number.isInteger(n))
    : null;
  const headerRetry = Number(response.headers.get('Retry-After'));
  return {
    ...EMPTY,
    status: response.status,
    code: code !== null && PROBLEM_CODES.has(code) ? code : 'internal',
    title: text(problem.title),
    requestId: response.headers.get('X-Request-Id') ?? text(problem.request_id),
    retryAfterSeconds:
      seconds(problem.retry_after) ??
      (response.headers.has('Retry-After') ? seconds(headerRetry) : null),
    minVersion: text(problem.min_version),
    updateUrl: text(problem.update_url),
    needBlocks,
  };
}

export function networkError(): ApiError {
  return { ...EMPTY, kind: 'network', code: 'network' };
}

/**
 * Failures the app reports itself in the notice area (contract api_client.md, mapping table).
 * Everything else is returned to the calling page, which shows it in place.
 */
export function isAppLevelError(error: ApiError): boolean {
  if (error.kind === 'network') return true;
  const status = error.status ?? 0;
  if (status === 401) return error.code !== 'password_required';
  return (
    status === 426 || status === 429 || status === 507 || status >= 500 || error.code === 'internal'
  );
}

/** A failure that may go away by itself: no answer, 429, or a server fault. */
export function isTransientError(error: ApiError): boolean {
  const status = error.status ?? 0;
  return error.kind === 'network' || status === 429 || status >= 500;
}
