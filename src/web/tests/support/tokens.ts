/** The custom properties (`--name: value;`) a token stylesheet declares, by name. */
export function declarations(css: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of css.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/gi)) {
    const name = match[1];
    const value = match[2];
    if (name !== undefined && value !== undefined) {
      found.set(name, value.replace(/\s+/g, ' ').trim());
    }
  }
  return found;
}

/** Differences between the web copy and the design source: one line per missing, extra or changed token. */
export function tokenDrift(copy: string, source: string): string[] {
  const mine = declarations(copy);
  const theirs = declarations(source);
  const drift: string[] = [];
  for (const [name, value] of theirs) {
    const copied = mine.get(name);
    if (copied === undefined) drift.push(`${name} is missing from the copy`);
    else if (copied !== value) drift.push(`${name} differs: copy "${copied}", source "${value}"`);
  }
  for (const name of mine.keys()) {
    if (!theirs.has(name)) drift.push(`${name} is not in the source`);
  }
  return drift;
}
