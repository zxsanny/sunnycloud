/**
 * Rejects the way a real fetch does: at once when the signal is already aborted, otherwise when it aborts.
 * A fake that only listens for the event never settles when the abort came before the request reached it.
 */
export function rejectWhenAborted(signal: AbortSignal, reject: (reason: unknown) => void): void {
  const abort = () => reject(new DOMException('aborted', 'AbortError'));
  if (signal.aborted) abort();
  else signal.addEventListener('abort', abort, { once: true });
}
