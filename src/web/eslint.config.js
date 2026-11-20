import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';

export default defineConfig(
  { ignores: ['dist/', 'node_modules/', 'openapi/', 'src/lib/api/generated/'] },
  js.configs.recommended,
  ts.configs.recommended,
  svelte.configs.recommended,
  svelte.configs.prettier,
  {
    languageOptions: { globals: { ...globals.browser, __APP_VERSION__: 'readonly' } },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: {
        projectService: true,
        extraFileExtensions: ['.svelte'],
        parser: ts.parser,
        svelteConfig,
      },
    },
  },
  {
    files: ['scripts/**/*.mjs', 'vite.config.ts', 'eslint.config.js', 'svelte.config.js'],
    languageOptions: { globals: globals.node },
  },
  {
    // Bundle rules of the CSP and of the facade layout (task AZ-3935, AC-8).
    files: ['src/**/*.{ts,svelte}'],
    rules: {
      'no-restricted-properties': [
        'error',
        {
          object: 'document',
          property: 'cookie',
          message: 'The session is an HttpOnly cookie; the page never reads or writes cookies.',
        },
      ],
      'no-restricted-globals': [
        'error',
        { name: 'fetch', message: 'Call the server through the facade of $lib/api.' },
        { name: 'XMLHttpRequest', message: 'Call the server through the facade of $lib/api.' },
        { name: 'EventSource', message: 'Call the server through the facade of $lib/api.' },
      ],
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['$lib/*/*'],
              message: 'Import a module through its barrel: $lib/<module>.',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['src/lib/api/**', 'src/**/*.test.ts'],
    rules: { 'no-restricted-globals': 'off' },
  },
);
