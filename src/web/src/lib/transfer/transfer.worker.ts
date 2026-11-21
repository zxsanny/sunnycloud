import * as wasm from '$wasm/sync_core_wasm';
import wasmUrl from '$wasm/sync_core_wasm_bg.wasm?url';
import { createApi } from '$lib/api';
import { DownloadEngine } from './download';
import { UploadEngine } from './engine';
import type { FromWorker, ToWorker } from './protocol';
import { openSink } from './sinks';

// The module Worker: loads sync-core-wasm (the .wasm is a separate content-hashed file under the page's own
// origin, instantiated with 'wasm-unsafe-eval'), then runs the engine for every message from the page.
const post = (message: FromWorker) => self.postMessage(message);

try {
  await wasm.default({ module_or_path: wasmUrl });
  const api = createApi();
  const engine = new UploadEngine({ wasm, api, post });
  const downloads = new DownloadEngine({ wasm, api, post, openSink });
  self.onmessage = (event: MessageEvent<ToWorker>) => {
    if (event.data.type === 'download') void downloads.handle(event.data);
    else void engine.handle(event.data);
  };
  post({ type: 'ready' });
} catch (cause) {
  console.error('The transfer worker could not load sync-core-wasm.', cause);
  post({ type: 'fatal', message: cause instanceof Error ? cause.message : 'wasm load failed' });
}
