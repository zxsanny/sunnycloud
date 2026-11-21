import { api } from '$lib/api';
import { notices } from '$lib/notices';
import { session } from '$lib/session';
import { ResumeStore } from './resume';
import type { WorkerLike } from './jobs';
import { TransferService } from './service.svelte';

export { UPLOAD_ERROR_TEXT, tooManyFilesText } from './errors';
export type { UploadError } from './errors';
export { MAX_SELECTION_FILES } from './jobs';
export type { EnqueueResult, JobProgress, ResumeOutcome, ResumeResult, WorkerLike } from './jobs';
export { matchesRecord } from './resume';
export type { ResumeRecord } from './resume';
export { TransferService } from './service.svelte';

/** The page-facing transfer service. Its Worker starts with the first upload. */
export const transfer = new TransferService({
  api,
  connect: () =>
    new Worker(new URL('./transfer.worker.ts', import.meta.url), {
      type: 'module',
    }) as unknown as WorkerLike,
  resume: new ResumeStore(() =>
    notices.show({
      key: 'resume-unavailable',
      variant: 'info',
      title: 'Interrupted uploads cannot be resumed in this browser.',
      text: 'Storage is blocked here. Uploads still work, but one that is interrupted has to start again.',
      dismissible: true,
    }),
  ),
  accountId: () => session.account?.id ?? null,
});

// A page that is closed, frozen or sent to the back-forward cache takes its Worker with it: the uploads stop and
// keep their resume records.
if (typeof window !== 'undefined') window.addEventListener('pagehide', () => transfer.suspend());
