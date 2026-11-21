import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

// The frames module alone, built and minified as the app is (same target, default minifier), so a test can run it:
// packbin reads member names from the source text of the accessor lambdas, and minification must keep them.
export default defineConfig({
  resolve: { alias: { $lib: fileURLToPath(new URL('../../src/lib', import.meta.url)) } },
  build: {
    target: 'esnext',
    minify: true,
    lib: {
      entry: fileURLToPath(new URL('../../src/lib/realtime/frames.ts', import.meta.url)),
      formats: ['es'],
      fileName: 'frames',
    },
  },
});
