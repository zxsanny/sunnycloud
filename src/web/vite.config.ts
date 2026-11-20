import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';
import pkg from './package.json' with { type: 'json' };

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
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    globalSetup: ['./tests/support/globalSetup.ts'],
    include: ['src/**/*.test.ts', 'tests/**/*.test.ts'],
  },
});
