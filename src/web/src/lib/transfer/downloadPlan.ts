import type { DownloadFailure } from './protocol';

/** `web.hash_verify_max_ff_safari`: the largest file Firefox and Safari check in the browser (a build constant). */
export const HASH_VERIFY_MAX_FF_SAFARI = 2 * 1024 * 1024 * 1024;

export type DownloadTarget = { nodeId: string; revisionId?: string } | { shareToken: string };

/** Why a file was saved without a check; the panel words each one. */
export type UnverifiedReason =
  'OverBrowserVerifyLimit' | 'NoVerifiedPathInBrowser' | 'NotEnoughBrowserStorage' | 'NoServerHash';

export type DownloadOutcome =
  | { kind: 'VerifiedSaved' }
  | { kind: 'SavedUnverified'; reason: UnverifiedReason; serverHash: string | null }
  | { kind: 'Failed'; reason: DownloadFailure };

/**
 * What this browser offers (16 section 5, spike S12 in AZ-3931). The save dialog is Chromium's. `opfsVerified` is true
 * only for an engine where the OPFS ranged pull was measured to pass (Firefox); Safari was not measured, so it is
 * false there and a file ends labelled unverified.
 */
export interface DownloadEnv {
  pickFile?: (name: string) => Promise<FileSystemFileHandle>;
  opfsVerified: () => boolean;
  /** Free space for a private browser file, or null when the browser does not say. */
  freeStorage: () => Promise<number | null>;
  /** Hands a URL (the browser's own download) or a verified file to the browser's download manager. */
  saveFile: (source: string | File, name: string) => void;
}

export type Strategy =
  { kind: 'picker' } | { kind: 'opfs' } | { kind: 'native'; reason: UnverifiedReason };

/**
 * Chromium streams into the chosen file and verifies at any size. Where the verified mechanism is the OPFS pull, a
 * file up to `max` is verified; anything else is the browser's own download, labelled "not verified".
 */
export function chooseStrategy(env: DownloadEnv, size: number, max: number): Strategy {
  if (env.pickFile !== undefined) return { kind: 'picker' };
  if (size > max) return { kind: 'native', reason: 'OverBrowserVerifyLimit' };
  if (env.opfsVerified()) return { kind: 'opfs' };
  return { kind: 'native', reason: 'NoVerifiedPathInBrowser' };
}
