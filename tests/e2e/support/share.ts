import { execFile } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import { expect, type BrowserContext, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "./env.ts";
import { HOSTILE_NAME } from "./media.ts";

const run = promisify(execFile);

export const PASSWORD = "sunset-at-lake";
export const SECRET_NAME = "secret-plan.pdf";
export const SECRET_BYTES = 5_000_000;
export const UNAVAILABLE = "This link isn't available";

export interface ShareFile {
  name: string;
  path: string;
  sha256: string;
}

const sha256Of = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

async function put(dir: string, name: string, bytes: Buffer): Promise<ShareFile> {
  const path = join(dir, name);
  await writeFile(path, bytes);
  return { name, path, sha256: sha256Of(bytes) };
}

/**
 * The files the share tests link to. The MP4 is VP9 in an MP4 box: the Chromium of Playwright has no H.264 decoder, and
 * the server decides `video/mp4` from the box. The PDFs and the HTML/SVG files carry random tails so every hash differs.
 */
export async function writeShareFiles(dir: string): Promise<Record<string, ShareFile>> {
  const frame = join(dir, "frame.v");
  await run("vips", ["gaussnoise", frame, "800", "600", "--mean", "128", "--sigma", "25"]);
  await run("vips", ["jpegsave", frame, join(dir, "share-photo.jpg"), "--Q", "85"]);
  await run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x180:rate=25", "-c:v", "libvpx-vp9", "-f", "mp4", join(dir, "share-clip.mp4")]);
  const files: Record<string, ShareFile> = {};
  for (const [key, name] of [["jpeg", "share-photo.jpg"], ["mp4", "share-clip.mp4"]] as const) {
    const path = join(dir, name);
    files[key] = { name, path, sha256: sha256Of(await readFile(path)) };
  }
  files.pdf = await put(dir, "itinerary.pdf", Buffer.concat([Buffer.from("%PDF-1.4\n"), randomBytes(300_000)]));
  files.secret = await put(dir, SECRET_NAME, Buffer.concat([Buffer.from("%PDF-1.4\n"), randomBytes(SECRET_BYTES - 9)]));
  files.movable = await put(dir, "movable.pdf", Buffer.concat([Buffer.from("%PDF-1.4\n"), randomBytes(50_000)]));
  files.second = await put(dir, "second-secret.pdf", Buffer.concat([Buffer.from("%PDF-1.4\n"), randomBytes(200_000)]));
  files.html = await put(dir, "evil.html", Buffer.from(`<!doctype html><script>window.__executed = 1; document.title = "pwned"</script><p>hostile</p>${randomBytes(8).toString("hex")}`));
  files.svg = await put(dir, "evil.svg", Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" onload="window.__executed = 1"><script>window.__executed = 1</script></svg><!--${randomBytes(8).toString("hex")}-->`));
  files.hostile = await put(dir, HOSTILE_NAME, Buffer.from(`plain text ${randomBytes(8).toString("hex")}\n`));
  return files;
}

export interface Created {
  id: string;
  token: string;
  url: string;
}

const origin = () => readAcceptanceEnv().ACC_ORIGIN as string;

/** `POST /shares` as the owner's browser session would send it (a cookie request needs the origin for the CSRF check). */
export async function createShareApi(owner: BrowserContext, body: Record<string, unknown>): Promise<{ status: number; body: Record<string, unknown> }> {
  const response = await owner.request.post("/api/v1/shares", { headers: { origin: origin() }, data: body });
  return { status: response.status(), body: (await response.json()) as Record<string, unknown> };
}

export async function createShare(owner: BrowserContext, body: Record<string, unknown>): Promise<Created> {
  const { status, body: answer } = await createShareApi(owner, body);
  expect(status, JSON.stringify(answer)).toBe(201);
  return { id: answer.id as string, token: answer.token as string, url: `${origin()}${answer.url_path as string}` };
}

export async function revokeShare(owner: BrowserContext, id: string): Promise<void> {
  const response = await owner.request.delete(`/api/v1/shares/${id}`, { headers: { origin: origin() } });
  expect(response.status()).toBe(204);
}

/** A fresh browser with no session, as a visitor of a link. Problems of the page are collected into `problems`. */
export async function visitor(browser: import("@playwright/test").Browser, problems: string[]): Promise<{ context: BrowserContext; page: Page }> {
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (/Content Security Policy/i.test(message.text())) problems.push(`csp: ${message.text()}`);
  });
  return { context, page };
}

/** SHA-256 of the file the page's Download button saves. */
export async function downloadHash(page: Page, trigger: () => Promise<void>): Promise<{ sha256: string; name: string }> {
  const [download] = await Promise.all([page.waitForEvent("download", { timeout: 60_000 }), trigger()]);
  return { sha256: sha256Of(await readFile(await download.path())), name: download.suggestedFilename() };
}

/** Everything a page can keep: both storages, IndexedDB (every record), Cache Storage and the cookies it can see. */
export async function dumpStorage(page: Page): Promise<string> {
  return page.evaluate(`(async () => {
    const out = [JSON.stringify({ ...localStorage }), JSON.stringify({ ...sessionStorage }), document.cookie, await caches.keys()];
    for (const info of await indexedDB.databases()) {
      if (!info.name) continue;
      const db = await new Promise((resolve, reject) => {
        const request = indexedDB.open(info.name);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      for (const store of Array.from(db.objectStoreNames)) {
        const rows = await new Promise((resolve, reject) => {
          const request = db.transaction(store).objectStore(store).getAll();
          request.onsuccess = () => resolve(request.result);
          request.onerror = () => reject(request.error);
        });
        out.push(info.name, store, JSON.stringify(rows));
      }
      db.close();
    }
    return JSON.stringify(out);
  })()`) as Promise<string>;
}
