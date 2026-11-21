import type { ResumePoint } from './client.svelte';

/** The cursor `E.S` of `GET /changes/head`: two unsigned decimal numbers joined by a dot. */
export function parseCursor(cursor: string): ResumePoint | null {
  const match = /^(\d+)\.(\d+)$/.exec(cursor);
  if (!match) return null;
  return { epoch: BigInt(match[1] as string), seq: BigInt(match[2] as string) };
}
