import { defineConfig } from 'vitest/config';
import base from './vite.config';

// The manual checks (bun run check:large) are not part of `bun run test`: they move gigabytes.
export default defineConfig({
  ...base,
  test: {
    ...base.test,
    include: ['tests/manual/*.manual.ts'],
    testTimeout: 600_000,
    hookTimeout: 600_000,
  },
});
