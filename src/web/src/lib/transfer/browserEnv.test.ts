import { afterEach, describe, expect, it, vi } from 'vitest';
import { browserDownloadEnv } from './browserEnv';

const FIREFOX = 'Mozilla/5.0 (X11; Linux x86_64; rv:159.0) Gecko/20100101 Firefox/159.0';
const SAFARI =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15';
const CHROME =
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/158.0.0.0 Safari/537.36';

function browser(userAgent: string, storage: object | undefined, picker = false) {
  vi.stubGlobal('navigator', { userAgent, storage });
  if (picker) vi.stubGlobal('showSaveFilePicker', () => Promise.resolve({}));
}

afterEach(() => vi.unstubAllGlobals());

describe('what the browser offers for a download', () => {
  it.each([
    { name: 'Chromium', ua: CHROME, picker: true, opfsVerified: false },
    { name: 'Firefox', ua: FIREFOX, picker: false, opfsVerified: true },
    { name: 'Safari', ua: SAFARI, picker: false, opfsVerified: false },
  ])(
    '$name: save dialog $picker, verified OPFS pull $opfsVerified',
    ({ ua, picker, opfsVerified }) => {
      // Arrange
      browser(ua, { getDirectory: () => Promise.resolve({}) }, picker);

      // Act
      const env = browserDownloadEnv();

      // Assert
      expect(env.pickFile !== undefined).toBe(picker);
      expect(env.opfsVerified()).toBe(opfsVerified);
    },
  );

  it('counts no OPFS in a Firefox window that has none (a private window)', () => {
    // Arrange
    browser(FIREFOX, undefined);

    // Act
    const env = browserDownloadEnv();

    // Assert
    expect(env.opfsVerified()).toBe(false);
  });

  it('reads the free space from the storage estimate, or says it does not know', async () => {
    // Arrange
    browser(FIREFOX, { estimate: () => Promise.resolve({ quota: 1000, usage: 400 }) });
    const env = browserDownloadEnv();

    // Act
    const free = await env.freeStorage();
    browser(FIREFOX, { estimate: () => Promise.resolve({}) });
    const none = await env.freeStorage();

    // Assert
    expect(free).toBe(600);
    expect(none).toBeNull();
  });
});
