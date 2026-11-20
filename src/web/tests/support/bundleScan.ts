export interface Finding {
  file: string;
  problem: string;
}

/** String constants of the Svelte runtime and the SVG namespace: never requested, only compared or printed. */
const ALLOWED_URL_PREFIXES = ['https://svelte.dev/e/', 'http://www.w3.org/'];

const ABSOLUTE_URL = /https?:\/\/[^\s"'`)\\<>]+/g;

/** What the CSP `script-src 'self'` and `style-src 'self'` forbid in the entry page. */
export function scanHtml(html: string, file: string): Finding[] {
  const findings: Finding[] = [];
  const add = (problem: string) => findings.push({ file, problem });

  for (const script of html.matchAll(/<script\b[^>]*>/gi)) {
    if (!/\bsrc\s*=/i.test(script[0])) add(`inline script: ${script[0]}`);
  }
  for (const tag of html.matchAll(/<[a-z][^>]*>/gi)) {
    if (/\son[a-z]+\s*=/i.test(tag[0])) add(`inline event handler: ${tag[0]}`);
    if (/\sstyle\s*=/i.test(tag[0])) add(`inline style attribute: ${tag[0]}`);
    if (/\s(?:href|src|action)\s*=\s*["']?\s*javascript:/i.test(tag[0])) {
      add(`javascript: URL: ${tag[0]}`);
    }
    if (/\s(?:href|src|action)\s*=\s*["']?\s*(?:https?:)?\/\//i.test(tag[0])) {
      add(`URL to another host: ${tag[0]}`);
    }
  }
  if (/<style\b/i.test(html)) add('inline style element');
  return findings;
}

/** Absolute URLs in a built script or stylesheet, apart from the runtime constants above. */
export function scanAsset(raw: string, file: string): Finding[] {
  const findings: Finding[] = [];
  // Comments are never requested (the Tailwind banner names its home page).
  const text = file.endsWith('.css') ? raw.replace(/\/\*[\s\S]*?\*\//g, '') : raw;
  for (const url of text.matchAll(ABSOLUTE_URL)) {
    if (!ALLOWED_URL_PREFIXES.some((prefix) => url[0].startsWith(prefix))) {
      findings.push({ file, problem: `URL to another host: ${url[0]}` });
    }
  }
  if (/url\(\s*["']?data:/i.test(text)) {
    findings.push({ file, problem: 'data: URL in a stylesheet (blocked by the CSP for fonts)' });
  }
  return findings;
}
