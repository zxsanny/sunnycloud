import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import tailwindcss from '@tailwindcss/vite';
import { loadEnv } from 'vite';
import { defineConfig } from 'vitest/config';
import pkg from './package.json' with { type: 'json' };

// Where the dev server proxies /api: VITE_PROXY_TARGET from the environment or a .env file (Bun and Vite both read it).
const proxyTarget =
  process.env.VITE_PROXY_TARGET ??
  loadEnv('development', process.cwd(), 'VITE_').VITE_PROXY_TARGET ??
  'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [tailwindcss(), svelte(), svelteTesting()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
      // Output of scripts/build-wasm.sh (gitignored).
      $wasm: fileURLToPath(new URL('./wasm/pkg', import.meta.url)),
    },
  },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  build: {
    target: 'esnext',
    // Fonts must stay separate files: a data: URL would break the CSP.
    assetsInlineLimit: 0,
  },
  worker: { format: 'es' },
  server: {
    // No changeOrigin: the browser's own Origin and Host must stay equal for the server's CSRF check.
    proxy: { '/api': proxyTarget },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    globalSetup: ['./tests/support/globalSetup.ts'],
    include: ['src/**/*.test.ts', 'tests/**/*.test.ts'],
    // The transfer tests hash megabytes with WASM and wait up to 15 s for a state, while the CI job runs a production
    // build beside them on four shared vCPUs: the 5 s default failed one 160 ms test there. The bound is for a hang.
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
