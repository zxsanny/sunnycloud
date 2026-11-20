export type ThemeChoice = 'system' | 'light' | 'dark';

const STORAGE_KEY = 'sunnycloud.theme';

function isChoice(value: string | null): value is ThemeChoice {
  return value === 'system' || value === 'light' || value === 'dark';
}

function apply(choice: ThemeChoice) {
  const root = document.documentElement;
  if (choice === 'system') {
    root.removeAttribute('data-theme');
  } else {
    root.dataset.theme = choice;
  }
}

/** The theme follows the OS (`color-scheme`); a stored choice forces one. Blocked storage means: follow the OS. */
class ThemeState {
  choice = $state<ThemeChoice>('system');

  init() {
    try {
      const stored = globalThis.localStorage.getItem(STORAGE_KEY);
      if (isChoice(stored)) this.choice = stored;
    } catch (cause) {
      console.warn(
        'The theme choice cannot be read from browser storage; following the OS.',
        cause,
      );
    }
    apply(this.choice);
  }

  set(choice: ThemeChoice) {
    this.choice = choice;
    apply(choice);
    try {
      globalThis.localStorage.setItem(STORAGE_KEY, choice);
    } catch (cause) {
      console.warn('The theme choice cannot be stored in browser storage.', cause);
    }
  }
}

export const theme = new ThemeState();
