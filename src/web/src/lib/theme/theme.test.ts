import { afterEach, describe, expect, it, vi } from 'vitest';
import { theme } from './theme.svelte';

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  document.documentElement.removeAttribute('data-theme');
  theme.choice = 'system';
});

describe('the theme', () => {
  it('follows the OS when nothing is stored', () => {
    // Act
    theme.init();

    // Assert
    expect(theme.choice).toBe('system');
    expect(document.documentElement).not.toHaveAttribute('data-theme');
  });

  it('forces the stored choice on the root element', () => {
    // Arrange
    localStorage.setItem('sunnycloud.theme', 'dark');

    // Act
    theme.init();

    // Assert
    expect(document.documentElement).toHaveAttribute('data-theme', 'dark');
  });

  it('keeps the choice across a reload', () => {
    // Arrange
    theme.set('light');
    theme.choice = 'system';

    // Act
    theme.init();

    // Assert
    expect(theme.choice).toBe('light');
  });

  it('goes back to the OS theme when system is chosen', () => {
    // Arrange
    theme.set('dark');

    // Act
    theme.set('system');

    // Assert
    expect(document.documentElement).not.toHaveAttribute('data-theme');
  });

  it('works with blocked storage: it follows the OS, applies the choice and reports the problem', () => {
    // Arrange
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('blocked', 'SecurityError');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('blocked', 'SecurityError');
    });

    // Act
    theme.init();
    theme.set('dark');

    // Assert
    expect(document.documentElement).toHaveAttribute('data-theme', 'dark');
    expect(warn).toHaveBeenCalledTimes(2);
  });
});
