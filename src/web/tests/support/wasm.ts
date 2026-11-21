import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import * as glue from '$wasm/sync_core_wasm';

// The module the Worker loads, run in Node (the pattern of src/sync-core-wasm/js/node-runner.mjs). Built by
// `bun run build:wasm`.
glue.initSync({ module: readFileSync(join(process.cwd(), 'wasm/pkg/sync_core_wasm_bg.wasm')) });

export const wasm = glue;

export const hex = (bytes: Uint8Array) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
