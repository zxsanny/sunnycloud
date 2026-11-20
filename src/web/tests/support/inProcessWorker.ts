import { createApi } from '$lib/api';
import { UploadEngine } from '$lib/transfer/engine';
import type { FromWorker, ToWorker } from '$lib/transfer/protocol';
import type { WorkerLike } from '$lib/transfer';
import type { UploadServer } from './uploadServer';
import { wasm } from './wasm';

/** The Worker's engine in this process, talking to a fake server: the same code path as the real Worker. */
export function inProcessWorker(server: UploadServer) {
  const sent: ToWorker[] = [];
  const worker: WorkerLike = {
    onmessage: null,
    onerror: null,
    terminate() {},
    postMessage(message) {
      sent.push(message);
      void engine.handle(message);
    },
  };
  const post = (data: FromWorker) => worker.onmessage?.({ data });
  const engine = new UploadEngine({
    wasm,
    api: createApi({ fetch: (request) => server.fetch(request) }),
    post,
    sleep: () => Promise.resolve(),
  });
  const connect = () => {
    queueMicrotask(() => post({ type: 'ready' }));
    return worker;
  };
  return { connect, sent };
}

/** The page-side API client of the same fake server. */
export function pageApi(server: UploadServer) {
  return createApi({ fetch: (request) => server.fetch(request) });
}
