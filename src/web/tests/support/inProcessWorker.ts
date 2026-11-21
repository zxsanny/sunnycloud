import { createApi } from '$lib/api';
import { DownloadEngine, type Sink } from '$lib/transfer/download';
import { UploadEngine } from '$lib/transfer/engine';
import type { DownloadSink, FromWorker, ToWorker } from '$lib/transfer/protocol';
import type { WorkerLike } from '$lib/transfer';
import type { UploadServer } from './uploadServer';
import { wasm } from './wasm';

/** The Worker's engine in this process, talking to a fake server: the same code path as the real Worker. */
export function inProcessWorker(
  server: Pick<UploadServer, 'fetch'>,
  openSink: (spec: DownloadSink) => Promise<Sink> = () =>
    Promise.reject(new Error('no sink in this test')),
) {
  const sent: ToWorker[] = [];
  const worker: WorkerLike = {
    onmessage: null,
    onerror: null,
    terminate() {},
    postMessage(message) {
      sent.push(message);
      if (message.type === 'download') void downloads.handle(message);
      else void engine.handle(message);
    },
  };
  const post = (data: FromWorker) => worker.onmessage?.({ data });
  const api = createApi({ fetch: (request) => server.fetch(request) });
  const engine = new UploadEngine({ wasm, api, post, sleep: () => Promise.resolve() });
  const downloads = new DownloadEngine({ wasm, api, post, openSink });
  const connect = () => {
    queueMicrotask(() => post({ type: 'ready' }));
    return worker;
  };
  return { connect, sent };
}

/** The page-side API client of the same fake server. */
export function pageApi(server: Pick<UploadServer, 'fetch'>) {
  return createApi({ fetch: (request) => server.fetch(request) });
}
