import type { UnverifiedReason } from './downloadPlan';
import type { DownloadFailure, Failure, LocalReason } from './protocol';

export type UploadError =
  | 'InvalidName'
  | 'QuotaExceeded'
  | 'SizeTooLarge'
  | 'SessionCapReached'
  | 'FreeSpaceFloor'
  | 'ParentNotFound'
  | 'AccountNotActive'
  | 'TooManyFiles'
  | 'StagedCapReached'
  | 'FileUnreadable'
  | 'FileChanged'
  | 'UploadSessionGone'
  | 'Network'
  | 'Failed';

const BY_CODE: Record<string, UploadError> = {
  invalid_name: 'InvalidName',
  quota_exceeded: 'QuotaExceeded',
  size_too_large: 'SizeTooLarge',
  vault_file_too_large: 'SizeTooLarge',
  session_cap_exceeded: 'SessionCapReached',
  free_space_floor: 'FreeSpaceFloor',
  parent_not_found: 'ParentNotFound',
  parent_deleted: 'ParentNotFound',
  not_a_folder: 'ParentNotFound',
  account_not_active: 'AccountNotActive',
  staged_cap_reached: 'StagedCapReached',
};

export function uploadErrorOfCode(code: string): UploadError {
  return BY_CODE[code] ?? 'Failed';
}

const LOCAL: Record<LocalReason, UploadError> = {
  FileUnreadable: 'FileUnreadable',
  FileChanged: 'FileChanged',
  SessionGone: 'UploadSessionGone',
  BlockRetriesExhausted: 'Failed',
  RestartLimit: 'Failed',
  Internal: 'Failed',
};

export function uploadErrorOf(failure: Failure): UploadError {
  if (failure.kind === 'local') return LOCAL[failure.reason];
  return failure.error.kind === 'network' ? 'Network' : uploadErrorOfCode(failure.error.code);
}

export const UPLOAD_ERROR_TEXT: Record<UploadError, string> = {
  InvalidName: 'This file name is not allowed.',
  QuotaExceeded: 'Storage quota reached.',
  SizeTooLarge: 'This file is larger than the server accepts.',
  SessionCapReached: 'Too many uploads are open. Try again when some have finished.',
  FreeSpaceFloor: 'The server is almost out of disk space.',
  ParentNotFound: 'The folder no longer exists.',
  AccountNotActive: 'This account is not active.',
  TooManyFiles: 'Too many files in one selection.',
  StagedCapReached: 'The server holds too many uploads waiting to be committed.',
  FileUnreadable: 'The file could not be read. It may have been moved or deleted.',
  FileChanged: 'The file changed while it was uploading.',
  UploadSessionGone:
    'This upload expired on the server. Choose Retry to upload the file again from the start.',
  Network: 'The connection was lost.',
  Failed: 'The upload failed.',
};

/** Wording fixed by the design (16 section 5, "Selection limit"). */
export function tooManyFilesText(count: number, limit: number): string {
  const n = new Intl.NumberFormat('en-US');
  return `This selection has ${n.format(count)} files. The web client takes at most ${n.format(limit)} files at once. For larger folders use the desktop client.`;
}

/** A large upload that stopped but keeps its resume record: what the row says, and what to do next. */
export const STOPPED_TEXT = {
  PageHidden: 'Stopped when the tab closed. Select the same file to continue.',
  FileUnreadable:
    'Stopped: the file could not be read. It may have been changed, moved or deleted. Select the same file to continue.',
  FileChanged:
    'Stopped: the file changed while it was uploading. Select it again to upload it from the start.',
} as const;

export const CHANGED_SINCE_LAST_TIME_TEXT =
  'This file changed since last time, so it is uploading again from the start.';

export const DOWNLOAD_FAILURE_TEXT: Record<DownloadFailure, string> = {
  HashMismatch:
    'Not saved: the downloaded bytes did not match the file on the server, so no file was kept as good.',
  Forbidden: 'You cannot download this item.',
  NotFound: 'This item no longer exists.',
  Network: 'The download did not finish. Nothing was kept as good.',
  SaveFailed: 'The file could not be written to the place you chose.',
  Cancelled: 'Cancelled',
};

/** The line under a download that finished without a check (wording of the mockup `web-files.html`). */
export function unverifiedText(reason: UnverifiedReason, max: number): string {
  switch (reason) {
    case 'OverBrowserVerifyLimit':
      return `Saved by the browser without a check (over ${Math.round(max / 1024 ** 3)} GB in this browser).`;
    case 'NoVerifiedPathInBrowser':
      return 'Saved by the browser without a check (this browser cannot check downloads).';
    case 'NotEnoughBrowserStorage':
      return 'Saved by the browser without a check (this browser has no room to check it).';
    case 'NoServerHash':
      return 'Saved by the browser without a check (the server gives no file hash for this link).';
  }
}

export const VERIFIED_TEXT = 'Downloaded and checked in the browser.';
