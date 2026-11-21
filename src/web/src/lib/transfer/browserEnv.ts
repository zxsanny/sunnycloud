import type { DownloadEnv } from './downloadPlan';

interface PickerWindow {
  showSaveFilePicker?: (options: { suggestedName: string }) => Promise<FileSystemFileHandle>;
}

/** The URL of a verified file is released this long after the click: the browser has started its save by then. */
const RELEASE_AFTER_MS = 60_000;

/**
 * What this browser can do for a download. Chromium has the save dialog. The OPFS ranged pull is the mechanism the
 * spike S12 (AZ-3931) measured as passing in Firefox; no run in Safari exists yet, so Safari is not counted.
 */
export function browserDownloadEnv(): DownloadEnv {
  const scope = globalThis as PickerWindow;
  return {
    pickFile:
      typeof scope.showSaveFilePicker === 'function'
        ? (name) =>
            (scope.showSaveFilePicker as NonNullable<PickerWindow['showSaveFilePicker']>)({
              suggestedName: name,
            })
        : undefined,
    opfsVerified: () =>
      typeof navigator.storage?.getDirectory === 'function' &&
      /\bFirefox\//.test(navigator.userAgent),
    async freeStorage() {
      const estimate = await navigator.storage?.estimate?.();
      return estimate?.quota !== undefined && estimate.usage !== undefined
        ? estimate.quota - estimate.usage
        : null;
    },
    saveFile(source, name) {
      const url = typeof source === 'string' ? source : URL.createObjectURL(source);
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      document.body.append(link);
      link.click();
      link.remove();
      if (typeof source !== 'string') setTimeout(() => URL.revokeObjectURL(url), RELEASE_AFTER_MS);
    },
  };
}
