import * as wasm from '$wasm/sync_core_wasm';
import wasmUrl from '$wasm/sync_core_wasm_bg.wasm?url';
import { createApi } from '$lib/api';
import { UploadEngine } from './engine';
import type { FromWorker, ToWorker } from './protocol';

// The module Worker: loads sync-core-wasm (the .wasm is a separate content-hashed file under the page's own
// origin, instantiated with 'wasm-unsafe-eval'), then runs the engine for every message from the page.
const post = (message: FromWorker) => self.postMessage(message);

try {
  await wasm.default({ module_or_path: wasmUrl });
  const engine = new UploadEngine({ wasm, api: createApi(), post });
  self.onmessage = (event: MessageEvent<ToWorker>) => {
    void engine.handle(event.data);
  };
  post({ type: 'ready' });
} catch (cause) {
  console.error('The transfer worker could not load sync-core-wasm.', cause);
  post({ type: 'fatal', message: cause instanceof Error ? cause.message : 'wasm load failed' });
}
