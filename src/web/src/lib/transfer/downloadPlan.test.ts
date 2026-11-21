import { describe, expect, it } from 'vitest';
import {
  HASH_VERIFY_MAX_FF_SAFARI,
  chooseStrategy,
  type DownloadEnv,
  type Strategy,
} from './downloadPlan';

const GIB = 1024 ** 3;

const env = (picker: boolean, opfsVerified: boolean): DownloadEnv => ({
  pickFile: picker ? () => Promise.reject(new Error('not used')) : undefined,
  opfsVerified: () => opfsVerified,
  freeStorage: () => Promise.resolve(null),
  saveFile: () => {},
});
const chromium = env(true, false);
const firefox = env(false, true);
const safari = env(false, false);

describe('the strategy per browser capability and size (E-26/c02 mechanism, 16 section 5)', () => {
  it('has the threshold of web.hash_verify_max_ff_safari: 2 GiB', () => {
    // Assert
    expect(HASH_VERIFY_MAX_FF_SAFARI).toBe(2 * GIB);
  });

  it.each<[string, DownloadEnv, number, Strategy]>([
    ['Chromium, 1 byte', chromium, 1, { kind: 'picker' }],
    ['Chromium, 2 GiB', chromium, 2 * GIB, { kind: 'picker' }],
    ['Chromium, 50 GiB', chromium, 50 * GIB, { kind: 'picker' }],
    ['Firefox, 1 GiB', firefox, GIB, { kind: 'opfs' }],
    ['Firefox, exactly 2 GiB', firefox, 2 * GIB, { kind: 'opfs' }],
    [
      'Firefox, 2 GiB + 1 byte',
      firefox,
      2 * GIB + 1,
      { kind: 'native', reason: 'OverBrowserVerifyLimit' },
    ],
    ['Firefox, 3 GiB', firefox, 3 * GIB, { kind: 'native', reason: 'OverBrowserVerifyLimit' }],
    [
      'Safari, 1 GiB (no mechanism recorded)',
      safari,
      GIB,
      { kind: 'native', reason: 'NoVerifiedPathInBrowser' },
    ],
    ['Safari, 3 GiB', safari, 3 * GIB, { kind: 'native', reason: 'OverBrowserVerifyLimit' }],
  ])('%s', (_name, capabilities, size, expected) => {
    // Act
    const strategy = chooseStrategy(capabilities, size, HASH_VERIFY_MAX_FF_SAFARI);

    // Assert
    expect(strategy).toEqual(expected);
  });

  it('reads the threshold from the config value it is given, not from a literal', () => {
    // Act
    const at = chooseStrategy(firefox, 5000, 5000);
    const over = chooseStrategy(firefox, 5001, 5000);

    // Assert
    expect(at).toEqual({ kind: 'opfs' });
    expect(over).toEqual({ kind: 'native', reason: 'OverBrowserVerifyLimit' });
  });
});
