/** Whole seconds from `now` to `at` (both epoch milliseconds), never below 0. */
export function secondsUntil(at: number, now: number): number {
  return Math.max(0, Math.ceil((at - now) / 1000));
}

/** Whole seconds left until the server's expiry; an unreadable expiry counts as already over. */
export function remainingSeconds(expiresAt: string, now: number): number {
  const at = Date.parse(expiresAt);
  return Number.isNaN(at) ? 0 : secondsUntil(at, now);
}

/** `m:ss`, the form of the countdown. */
export function countdownText(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
}

export interface ActionState {
  /** Deny is the primary action on a mismatch; Approve is primary only when the places match. */
  primary: 'approve' | 'deny';
  approveEnabled: boolean;
}

/** Approve is direct on a match; on a mismatch it needs the explicit tick. */
export function actionState(mismatch: boolean, confirmed: boolean): ActionState {
  return mismatch
    ? { primary: 'deny', approveEnabled: confirmed }
    : { primary: 'approve', approveEnabled: true };
}

/** The place the server resolved, or a plain statement that it could not. */
export function placeText(location: string | null): string {
  return location === null || location === '' ? 'Location unknown' : location;
}
