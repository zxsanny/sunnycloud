import { execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { decodeClientFrame, decodeServerFrame } from '../src/lib/realtime/frames';
import { scanAsset, scanHtml, type Finding } from './support/bundleScan';
import { hexOf, loadVectors } from './support/realtimeVectors';

// Vitest runs from the project root.
const root = process.cwd();

describe('the production bundle under the CSP (script-src and style-src self)', () => {
  let out: string;

  beforeAll(() => {
    out = mkdtempSync(join(tmpdir(), 'sunnycloud-web-dist-'));
    // The real build in its own process, as `bun run build` runs it.
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST')),
    );
    execFileSync(
      process.execPath,
      [join(root, 'node_modules/vite/bin/vite.js'), 'build', '--outDir', out, '--emptyOutDir'],
      { cwd: root, env: { ...env, NODE_ENV: 'production' }, stdio: 'pipe' },
    );
  }, 120_000);

  afterAll(() => rmSync(out, { recursive: true, force: true }));

  it('has an index.html with no inline script, no inline event handler and no inline style', () => {
    // Act
    const findings = scanHtml(readFileSync(join(out, 'index.html'), 'utf8'), 'index.html');

    // Assert
    expect(findings).toEqual([]);
  });

  it('loads its script and stylesheet from its own origin', () => {
    // Act
    const html = readFileSync(join(out, 'index.html'), 'utf8');

    // Assert
    expect(html).toMatch(
      /<script type="module" crossorigin src="\/assets\/index-[\w-]+\.js"><\/script>/,
    );
    expect(html).toMatch(/<link rel="stylesheet" crossorigin href="\/assets\/index-[\w-]+\.css">/);
  });

  it('keeps every script and stylesheet free of URLs to other hosts and of data: fonts', () => {
    // Arrange
    const assets = readdirSync(join(out, 'assets')).filter((name) => /\.(js|css)$/.test(name));
    const findings: Finding[] = [];

    // Act
    for (const name of assets) {
      findings.push(...scanAsset(readFileSync(join(out, 'assets', name), 'utf8'), name));
    }

    // Assert
    expect(assets.length).toBeGreaterThanOrEqual(2);
    expect(findings).toEqual([]);
  });

  it('serves sync-core-wasm as a content-hashed file that the Worker loads by URL, never inlined', () => {
    // Arrange
    const names = readdirSync(join(out, 'assets'));
    const wasm = names.find((name) => /^sync_core_wasm_bg-[\w-]+\.wasm$/.test(name));
    const worker = names.find((name) => /^transfer\.worker-[\w-]+\.js$/.test(name));

    // Act
    const workerSource = worker ? readFileSync(join(out, 'assets', worker), 'utf8') : '';
    const inlined = names
      .filter((name) => name.endsWith('.js'))
      .some((name) =>
        /data:application\/wasm|new Uint8Array\(atob\(/.test(
          readFileSync(join(out, 'assets', name), 'utf8'),
        ),
      );

    // Assert
    expect(wasm).toBeDefined();
    expect(worker).toBeDefined();
    expect(workerSource).toContain(`/assets/${wasm}`);
    expect(inlined).toBe(false);
    expect(readFileSync(join(out, 'assets', wasm ?? '')).subarray(0, 4)).toEqual(
      Buffer.from([0, 0x61, 0x73, 0x6d]),
    );
  });

  it('serves Manrope and JetBrains Mono as separate woff2 files from its own origin', () => {
    // Act
    const fonts = readdirSync(join(out, 'assets')).filter((name) => name.endsWith('.woff2'));

    // Assert
    expect(fonts.some((name) => name.startsWith('manrope-'))).toBe(true);
    expect(fonts.some((name) => name.startsWith('jetbrains-mono-'))).toBe(true);
  });
});

describe('the bundle scanner, on seeded violations', () => {
  it.each([
    { name: 'an inline script', html: '<script>alert(1)</script>', expected: 'inline script' },
    {
      name: 'an inline event handler',
      html: '<img src="/a.png" onerror="x()">',
      expected: 'inline event handler',
    },
    {
      name: 'an inline style attribute',
      html: '<div style="color:red"></div>',
      expected: 'inline style attribute',
    },
    {
      name: 'an inline style element',
      html: '<style>a{}</style>',
      expected: 'inline style element',
    },
    {
      name: 'a script from another host',
      html: '<script src="https://evil.test/x.js"></script>',
      expected: 'URL to another host',
    },
    {
      name: 'a protocol-relative stylesheet',
      html: '<link rel="stylesheet" href="//evil.test/x.css">',
      expected: 'URL to another host',
    },
    {
      name: 'a javascript: link',
      html: '<a href="javascript:alert(1)">x</a>',
      expected: 'javascript: URL',
    },
  ])('finds $name in the page', ({ html, expected }) => {
    // Act
    const findings = scanHtml(html, 'seed.html');

    // Assert
    expect(findings.map((f) => f.problem).join('\n')).toContain(expected);
  });

  it('finds a URL to another host in a script, but not the Svelte runtime constants', () => {
    // Arrange
    const script =
      'fetch("https://evil.test/x");const a="https://svelte.dev/e/state_unsafe_mutation";const b="http://www.w3.org/2000/svg";';

    // Act
    const findings = scanAsset(script, 'seed.js');

    // Assert
    expect(findings.map((f) => f.problem)).toEqual(['URL to another host: https://evil.test/x']);
  });

  it('ignores a URL inside a stylesheet comment', () => {
    // Act
    const findings = scanAsset(
      '/*! tailwindcss | https://tailwindcss.com */a{color:red}',
      'seed.css',
    );

    // Assert
    expect(findings).toEqual([]);
  });

  it('finds a data: font in a stylesheet', () => {
    // Act
    const findings = scanAsset('@font-face{src:url(data:font/woff2;base64,AAAA)}', 'seed.css');

    // Assert
    expect(findings).toHaveLength(1);
  });

  it('passes a clean page', () => {
    // Arrange
    const html =
      '<script type="module" crossorigin src="/assets/index-x.js"></script><link rel="stylesheet" href="/assets/index-x.css">';

    // Act
    const findings = scanHtml(html, 'clean.html');

    // Assert
    expect(findings).toEqual([]);
  });
});

describe('the realtime frames in a minified production build', () => {
  it('decode and encode every shared vector exactly as the unminified module does', () => {
    // Arrange
    const out = mkdtempSync(join(tmpdir(), 'sunnycloud-frames-'));
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST')),
    );
    execFileSync(
      process.execPath,
      [
        join(root, 'node_modules/vite/bin/vite.js'),
        'build',
        '--config',
        'tests/support/frames.bundle.config.ts',
        '--outDir',
        out,
        '--emptyOutDir',
      ],
      { cwd: root, env: { ...env, NODE_ENV: 'production' }, stdio: 'pipe' },
    );
    const bundle = join(out, 'frames.js');
    const vectors = loadVectors();
    const input = JSON.stringify(
      Object.fromEntries([...vectors].map(([name, bytes]) => [name, hexOf(bytes)])),
    );

    // Act
    const run = execFileSync(
      process.execPath,
      [join(root, 'tests/support/runBundledFrames.mjs'), bundle],
      {
        input,
        env,
      },
    ).toString();
    rmSync(out, { recursive: true, force: true });

    // Assert
    const built = JSON.parse(run) as {
      server: Record<string, string>;
      client: Record<string, string>;
      encoded: Record<string, string>;
    };
    const text = (value: unknown) =>
      JSON.stringify(value, (_k, v) => (typeof v === 'bigint' ? v.toString() : v));
    for (const [name, bytes] of vectors) {
      expect(built.server[name], `server decode of ${name}`).toBe(text(decodeServerFrame(bytes)));
      expect(built.client[name], `client decode of ${name}`).toBe(text(decodeClientFrame(bytes)));
    }
    expect(built.server.change_file).toContain('"nodeKind":"file"');
    expect(built.server.change_file).toContain('"name":"IMG_0001.HEIC"');
    expect(built.server.bad_server_hello_truncated).toContain('"ok":false');
    expect(built.encoded.subscribe).toBe(hexOf(vectors.get('subscribe') ?? new Uint8Array()));
    expect(built.encoded.ping).toBe(hexOf(vectors.get('ping') ?? new Uint8Array()));
  }, 120_000);
});
