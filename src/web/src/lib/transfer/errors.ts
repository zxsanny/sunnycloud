import type { Failure } from './protocol';

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

export function uploadErrorOf(failure: Failure): UploadError {
  if (failure.kind === 'local') {
    return failure.reason === 'FileUnreadable' ? 'FileUnreadable' : 'Failed';
  }
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
  Network: 'The connection was lost.',
  Failed: 'The upload failed.',
};

/** Wording fixed by the design (16 section 5, "Selection limit"). */
export function tooManyFilesText(count: number, limit: number): string {
  const n = new Intl.NumberFormat('en-US');
  return `This selection has ${n.format(count)} files. The web client takes at most ${n.format(limit)} files at once. For larger folders use the desktop client.`;
}
