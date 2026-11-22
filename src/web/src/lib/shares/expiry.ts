import { formatDate } from '$lib/format';

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Offered only when the server states a longest lifetime they fit under. */
const PRESETS = [DAY, 7 * DAY, 30 * DAY];

export interface ShareTtl {
  /** Whole seconds, or null when the server did not say. */
  defaultTtl: number | null;
  maxTtl: number | null;
}

export interface ExpiryOption {
  /** Seconds for `expires_in`; undefined leaves it out, so the server applies its own default. */
  seconds: number | undefined;
  label: string;
}

export interface ExpiryOptions {
  options: ExpiryOption[];
  /** The option selected when the dialog opens: the server's default. */
  preselected: ExpiryOption;
  helper: string;
}

function unit(seconds: number, size: number, name: string): string | null {
  if (seconds % size !== 0) return null;
  const count = seconds / size;
  return `${count} ${name}${count === 1 ? '' : 's'}`;
}

/** `7 days`, `36 hours`; the largest whole unit the lifetime divides into. */
export function durationText(seconds: number): string {
  return (
    unit(seconds, DAY, 'day') ??
    unit(seconds, HOUR, 'hour') ??
    unit(seconds, MINUTE, 'minute') ??
    unit(seconds, 1, 'second') ??
    ''
  );
}

const SERVER_DEFAULT: ExpiryOption = { seconds: undefined, label: 'Server default' };

/**
 * The expiry choices of the owner's dialog. Nothing above the server's longest lifetime is offered, and the
 * server's default is preselected. Without the limits only "Server default" is offered: `expires_in` is then left out.
 */
export function expiryOptions(ttl: ShareTtl, now: number = Date.now()): ExpiryOptions {
  const { defaultTtl, maxTtl } = ttl;
  const label = (seconds: number) =>
    `${durationText(seconds)}, until ${formatDate(new Date(now + seconds * 1000).toISOString())}`;
  if (maxTtl === null) {
    if (defaultTtl === null) {
      return {
        options: [SERVER_DEFAULT],
        preselected: SERVER_DEFAULT,
        helper: 'The server chooses how long the link lives.',
      };
    }
    const only = { seconds: defaultTtl, label: label(defaultTtl) };
    return {
      options: [only],
      preselected: only,
      helper: 'The server chooses the longest lifetime.',
    };
  }
  const preferred = defaultTtl === null ? null : Math.min(defaultTtl, maxTtl);
  const seconds = [...new Set([...PRESETS, maxTtl, ...(preferred === null ? [] : [preferred])])]
    .filter((value) => value > 0 && value <= maxTtl)
    .sort((a, b) => a - b);
  const options = seconds.map((value) => ({ seconds: value, label: label(value) }));
  if (options.length === 0) {
    return { options: [SERVER_DEFAULT], preselected: SERVER_DEFAULT, helper: '' };
  }
  const preselected = options.find((option) => option.seconds === preferred) ?? SERVER_DEFAULT;
  return {
    options: preferred === null ? [SERVER_DEFAULT, ...options] : options,
    preselected,
    helper: `This server allows links for up to ${durationText(maxTtl)}.`,
  };
}
