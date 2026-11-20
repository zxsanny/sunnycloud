import { ESLint } from 'eslint';
import { describe, expect, it } from 'vitest';
import baseConfig from '../eslint.config.js';

// The seeds are not files of the project, so the type-aware project service is switched off for them;
// the rules under test do not use types.
const config = baseConfig.map((entry) => {
  const parserOptions = entry.languageOptions?.parserOptions;
  if (parserOptions === undefined) return entry;
  return {
    ...entry,
    languageOptions: {
      ...entry.languageOptions,
      parserOptions: { ...parserOptions, projectService: false },
    },
  };
});
const eslint = new ESLint({ overrideConfigFile: true, overrideConfig: config, cwd: process.cwd() });

async function violations(code: string, filePath: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath });
  return (result?.messages ?? []).map((m) => m.ruleId ?? 'parse-error');
}

describe('the lint rules that keep the bundle inside the CSP and the facade layout', () => {
  it('forbids {@html} in a component', async () => {
    // Act
    const rules = await violations(
      '<script lang="ts">let raw = "<b>x</b>";</script>\n{@html raw}\n',
      'src/pages/Seed.svelte',
    );

    // Assert
    expect(rules).toContain('svelte/no-at-html-tags');
  });

  it('forbids reading document.cookie', async () => {
    // Act
    const rules = await violations('export const all = document.cookie;\n', 'src/pages/seed.ts');

    // Assert
    expect(rules).toContain('no-restricted-properties');
  });

  it.each(['fetch("/x")', 'new XMLHttpRequest()', 'new EventSource("/x")'])(
    'forbids %s outside the API module',
    async (call) => {
      // Act
      const rules = await violations(`export const x = ${call};\n`, 'src/pages/seed.ts');

      // Assert
      expect(rules).toContain('no-restricted-globals');
    },
  );

  it('allows fetch inside the API module', async () => {
    // Act
    const rules = await violations('export const x = () => fetch("/x");\n', 'src/lib/api/seed.ts');

    // Assert
    expect(rules).not.toContain('no-restricted-globals');
  });

  it('forbids importing a module file past its barrel', async () => {
    // Act
    const rules = await violations(
      "import { createApi } from '$lib/api/client';\nexport const x = createApi;\n",
      'src/pages/seed.ts',
    );

    // Assert
    expect(rules).toContain('no-restricted-imports');
  });

  it('allows importing a module through its barrel', async () => {
    // Act
    const rules = await violations(
      "import { api } from '$lib/api';\nexport const x = api;\n",
      'src/pages/seed.ts',
    );

    // Assert
    expect(rules).not.toContain('no-restricted-imports');
  });
});
