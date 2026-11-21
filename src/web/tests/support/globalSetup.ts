import { existsSync } from 'node:fs';
import { join } from 'node:path';

export default function setup() {
  if (!existsSync(join(process.cwd(), 'wasm/pkg/sync_core_wasm_bg.wasm'))) {
    throw new Error(
      'wasm/pkg is missing: run `bun run build:wasm` (needs wasm-pack) before the tests.',
    );
  }
}
