// Browser legs of the vector gate: serves the package, this directory and the vector file on 127.0.0.1,
// opens browser-host.html in each Playwright browser and reads the Worker's result.
// Usage: node browser-runner.mjs <pkg dir> <vector file> <playwright package dir or ""> [browser...]
// Prints `LEG <browser> PASS|FAIL(...)|NOT-RUN(<reason>)`; a browser that cannot start is NOT-RUN, never
// PASS. Exit 1 when any browser FAILs.

import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const RESULT_TIMEOUT_MS = 240_000;
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".txt": "text/plain" };

const [pkgDir, vectorPath, playwrightDir, ...requested] = process.argv.slice(2);
const browsers = requested.length > 0 ? requested : ["chromium", "firefox", "webkit"];
const jsDir = dirname(fileURLToPath(import.meta.url));

function fileFor(urlPath) {
  if (urlPath === "/") return join(jsDir, "browser-host.html");
  if (urlPath === "/vectors.txt") return vectorPath;
  const [, root, ...rest] = urlPath.split("/");
  const base = { js: jsDir, pkg: pkgDir }[root];
  const relative = normalize(rest.join("/"));
  return base && !relative.startsWith("..") ? join(base, relative) : null;
}

function serve() {
  const server = createServer(async (request, response) => {
    const path = fileFor(new URL(request.url, "http://127.0.0.1").pathname);
    try {
      if (!path) throw Object.assign(new Error("outside the served roots"), { code: "ENOENT" });
      const body = await readFile(path);
      response.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream" });
      response.end(body);
    } catch (error) {
      if (error.code !== "ENOENT") console.error(`[browsers] serving ${request.url}: ${error.message}`);
      response.writeHead(404);
      response.end();
    }
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

const firstLine = (text) => String(text).split("\n").find((line) => line.trim() !== "")?.trim() ?? "unknown";

async function runBrowser(playwright, name, url) {
  let browser;
  try {
    browser = await playwright[name].launch();
  } catch (error) {
    return { status: `NOT-RUN(launch: ${firstLine(error.message)})` };
  }
  try {
    const page = await browser.newPage();
    await page.goto(url);
    const handle = await page.waitForFunction(() => window.vectorResult, null, { timeout: RESULT_TIMEOUT_MS });
    const result = await handle.jsonValue();
    if (result.error) return { status: `FAIL(${result.error})`, diffs: [] };
    console.log(
      `[${name}] ${browser.version()}: ${result.cases} cases in ${result.ms} ms; memory after 6 MiB ` +
        `${result.memory.afterSixMib}, after 100 MiB ${result.memory.afterHundredMib}, growth ${result.memory.growth}`,
    );
    const status = result.diffs.length === 0 ? "PASS" : `FAIL(${result.diffs.length} differences)`;
    return { status, diffs: result.diffs };
  } catch (error) {
    return { status: `FAIL(${firstLine(error.message)})`, diffs: [] };
  } finally {
    await browser.close();
  }
}

let playwright = null;
let missing = "no-playwright";
if (playwrightDir) {
  try {
    playwright = createRequire(join(playwrightDir, "package.json"))(playwrightDir);
  } catch (error) {
    missing = `no-playwright: ${firstLine(error.message)}`;
  }
}

let failed = false;
if (!playwright) {
  for (const name of browsers) console.log(`LEG ${name} NOT-RUN(${missing})`);
} else {
  const server = await serve();
  const url = `http://127.0.0.1:${server.address().port}/`;
  for (const name of browsers) {
    const { status, diffs = [] } = await runBrowser(playwright, name, url);
    for (const diff of diffs) console.log(`DIFF ${name} ${diff}`);
    console.log(`LEG ${name} ${status}`);
    failed ||= status.startsWith("FAIL");
  }
  server.close();
}
process.exit(failed ? 1 : 0);
