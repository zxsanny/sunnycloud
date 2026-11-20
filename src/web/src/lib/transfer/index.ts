import { api } from '$lib/api';
import { TransferService, type WorkerLike } from './service.svelte';

export { UPLOAD_ERROR_TEXT, tooManyFilesText } from './errors';
export type { UploadError } from './errors';
export { MAX_SELECTION_FILES, TransferService } from './service.svelte';
export type { EnqueueResult, JobProgress, UploadedEvent, WorkerLike } from './service.svelte';

/** The page-facing transfer service. Its Worker starts with the first upload. */
export const transfer = new TransferService({
  api,
  connect: () =>
    new Worker(new URL('./transfer.worker.ts', import.meta.url), {
      type: 'module',
    }) as unknown as WorkerLike,
});
