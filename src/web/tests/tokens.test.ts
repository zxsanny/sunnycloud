import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { tokenDrift } from './support/tokens';

// Vitest runs from the project root.
const source = readFileSync(
  join(process.cwd(), '../../_docs/02_document/ui_mockups/assets/tokens.css'),
  'utf8',
);
const copy = readFileSync(join(process.cwd(), 'src/styles/tokens.css'), 'utf8');

describe('the design tokens', () => {
  it('are the same values in the web copy and in the design source', () => {
    // Act
    const drift = tokenDrift(copy, source);

    // Assert
    expect(drift).toEqual([]);
  });

  it('report a value that changed on one side only', () => {
    // Arrange
    const edited = copy.replace(
      '--action: light-dark(var(--b-600), var(--b-400));',
      '--action: red;',
    );

    // Act
    const drift = tokenDrift(edited, source);

    // Assert
    expect(drift).toHaveLength(1);
    expect(drift[0]).toContain('--action differs');
  });

  it('report a token that exists on one side only', () => {
    // Arrange
    const extra = `${copy}\n:root { --brand-new: #123456; }`;

    // Act
    const drift = tokenDrift(extra, source);

    // Assert
    expect(drift).toEqual(['--brand-new is not in the source']);
  });
});
