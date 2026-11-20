export type ViewMode = 'list' | 'grid';

const STORAGE_KEY = 'sunnycloud.view';

/** The file view (list or photo grid). Blocked storage means the list view and no error. */
class ViewPreference {
  mode = $state<ViewMode>('list');

  init() {
    try {
      const stored = globalThis.localStorage.getItem(STORAGE_KEY);
      if (stored === 'list' || stored === 'grid') this.mode = stored;
    } catch (cause) {
      console.warn('The view choice cannot be read from browser storage; using the list.', cause);
    }
  }

  set(mode: ViewMode) {
    this.mode = mode;
    try {
      globalThis.localStorage.setItem(STORAGE_KEY, mode);
    } catch (cause) {
      console.warn('The view choice cannot be stored in browser storage.', cause);
    }
  }
}

export const viewPreference = new ViewPreference();
