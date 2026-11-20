import type * as SyncCoreWasm from '$wasm/sync_core_wasm';

/** The exports of sync-core-wasm the transfer engine uses: the one implementation of the hashing and planning rules. */
export type WasmApi = Pick<
  typeof SyncCoreWasm,
  'block_size' | 'hash_block' | 'rollup' | 'new_commit_id' | 'UploadPlanner'
>;
