import type { components } from './generated/schema';

const PROTOCOL_BLOCK_SIZE = 2 * 1024 * 1024;

/** Build defaults; what the server says wins (contract api_client.md, ServerLimits). */
const WEB_LANES = 4;

export interface ServerLimits {
  blockSize: number;
  /** Effective lanes: the smaller of the web default (4) and the server's limit, at least one. */
  lanes: number;
  smallFileMax: number;
  batchMaxEntries: number;
  maxStagedPerAccount: number;
  /** False when the server's block size is not the 2 MiB this client implements. */
  uploadsEnabled: boolean;
}

export function serverLimitsOf(info: components['schemas']['ServerInfo']): ServerLimits {
  return {
    blockSize: info.block_size,
    lanes: Math.max(1, Math.min(WEB_LANES, info.max_lanes)),
    smallFileMax: info.small_file_max,
    batchMaxEntries: info.batch_max_entries,
    maxStagedPerAccount: info.max_staged_per_account,
    uploadsEnabled: info.block_size === PROTOCOL_BLOCK_SIZE,
  };
}
