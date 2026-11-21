import { createHash, randomBytes } from "node:crypto";
import { createWriteStream } from "node:fs";
import { readFile, utimes, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { ACC_DATA } from "./env.ts";
import { apiUnit } from "./session.ts";

const MIB = 1024 * 1024;
export const BLOCK = 2 * MIB;
const BLOCK_URL = /\/api\/v1\/uploads\/([0-9a-f-]+)\/blocks\/(\d+)$/;

/** A file of random bytes, written in 1 MiB pieces; returns its SHA-256. */
export async function writeRandomFile(dir: string, name: string, mib: number): Promise<{ path: string; sha256: string }> {
  const path = join(dir, name);
  const hash = createHash("sha256");
  const out = createWriteStream(path);
  for (let i = 0; i < mib; i += 1) {
    const chunk = randomBytes(MIB);
    hash.update(chunk);
    if (!out.write(chunk)) await new Promise<void>((resolve) => out.once("drain", () => resolve()));
  }
  await new Promise<void>((resolve, reject) => out.end((error?: Error | null) => (error ? reject(error) : resolve())));
  return { path, sha256: hash.digest("hex") };
}

/** Same bytes, a modified time one minute later. */
export async function touch(path: string): Promise<void> {
  const later = new Date(Date.now() + 60_000);
  await utimes(path, later, later);
}

/** Appends 1 MiB: the same name with another size. */
export async function growByOneMib(path: string): Promise<string> {
  const all = Buffer.concat([await readFile(path), randomBytes(MIB)]);
  await writeFile(path, all);
  return createHash("sha256").update(all).digest("hex");
}

export interface BlockTraffic {
  /** Block indices in the order their PUT was first seen (they include the ones held back). */
  attempted: number[];
  /** Block indices the server answered. */
  acked: number[];
  /** Status codes of the block PUTs that were answered. */
  statuses: number[];
  /** Releases the PUTs that are held back; later ones pass at once. */
  release: () => void;
}

/**
 * Lets the first `after` block PUTs of the page through and holds the rest until `release()` or until the page closes,
 * the way a tab that closes while blocks are on the wire leaves them. Requests of the page's Worker are included.
 */
export async function holdBlocksAfter(page: Page, after: number): Promise<BlockTraffic> {
  let release = () => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  let held = true;
  const traffic: BlockTraffic = { attempted: [], acked: [], statuses: [], release: () => ((held = false), release()) };
  await page.route(BLOCK_URL, async (route) => {
    const index = Number(BLOCK_URL.exec(new URL(route.request().url()).pathname)?.[2]);
    traffic.attempted.push(index);
    if (held && traffic.attempted.length > after) await gate;
    const response = await route.fetch();
    traffic.statuses.push(response.status());
    if (response.ok()) traffic.acked.push(index);
    await route.fulfill({ response });
  });
  return traffic;
}

export interface Traffic {
  sessionStarts: number;
  /** DELETE /uploads/{id}: the sessions that were aborted. */
  aborted: string[];
  blockPuts: number[];
  blockStatuses: number[];
  commits: number;
}

/** Counts the upload requests of a page (its Worker included) from now on. */
export function watchUploads(page: Page): Traffic {
  const traffic: Traffic = { sessionStarts: 0, aborted: [], blockPuts: [], blockStatuses: [], commits: 0 };
  page.on("request", (request) => {
    const url = new URL(request.url());
    const block = BLOCK_URL.exec(url.pathname);
    if (block && request.method() === "PUT") traffic.blockPuts.push(Number(block[2]));
    else if (request.method() === "POST" && url.pathname === "/api/v1/uploads") traffic.sessionStarts += 1;
    else if (request.method() === "POST" && url.pathname === "/api/v1/commits") traffic.commits += 1;
    else if (request.method() === "DELETE" && /^\/api\/v1\/uploads\/[0-9a-f-]+$/.test(url.pathname)) traffic.aborted.push(url.pathname.split("/").pop() ?? "");
  });
  page.on("response", (response) => {
    if (BLOCK_URL.test(new URL(response.url()).pathname) && response.request().method() === "PUT") traffic.blockStatuses.push(response.status());
  });
  return traffic;
}

export interface StoredRecord {
  accountId: string;
  fileName: string;
  size: number;
  lastModified: number;
  sessionId: string;
  clientKey: string;
  blocklist: string[];
  parentId: string;
}

// The records live in the page's IndexedDB (src/web/src/lib/transfer/resume.ts). Plain scripts, as the other specs use.
const OPEN = `new Promise((resolve, reject) => {
  const open = indexedDB.open("sunnycloud-transfer", 1);
  open.onupgradeneeded = () => open.result.createObjectStore("resume", { keyPath: ["accountId", "clientKey"] });
  open.onsuccess = () => resolve(open.result);
  open.onerror = () => reject(open.error);
})`;

/** Every resume record in the browser profile, whatever the account. */
export async function dumpRecords(page: Page): Promise<StoredRecord[]> {
  return page.evaluate(`(async () => {
    const db = await ${OPEN};
    return new Promise((resolve, reject) => {
      const all = db.transaction("resume").objectStore("resume").getAll();
      all.onsuccess = () => { db.close(); resolve(all.result); };
      all.onerror = () => reject(all.error);
    });
  })()`);
}

/** Replaces the block hashes of the stored record `edits` names (index to a wrong hash). */
export async function tamperBlocklist(page: Page, edits: Record<number, string>): Promise<void> {
  await page.evaluate(`(async () => {
    const db = await ${OPEN};
    const edits = ${JSON.stringify(edits)};
    await new Promise((resolve, reject) => {
      const tx = db.transaction("resume", "readwrite");
      const store = tx.objectStore("resume");
      const all = store.getAll();
      all.onsuccess = () => {
        for (const record of all.result) {
          for (const [index, hash] of Object.entries(edits)) record.blocklist[Number(index)] = hash;
          store.put(record);
        }
      };
      tx.oncomplete = resolve;
      tx.onerror = () => reject(tx.error);
    });
    db.close();
  })()`);
}

const ENV_FILE = `${ACC_DATA}/secrets/mvp.env`;
const TTL_KEY = "SUNNYCLOUD_UPLOAD_SESSION_TTL";

/** Sets (or, with null, removes) `upload_session_ttl` in the acceptance stack's own environment file and restarts its API. */
export async function setUploadSessionTtl(ttl: string | null): Promise<void> {
  const lines = (await readFile(ENV_FILE, "utf8")).split("\n").filter((line) => !line.startsWith(`${TTL_KEY}=`));
  if (ttl !== null) lines.splice(lines.length - 1, 0, `${TTL_KEY}=${ttl}`);
  await writeFile(ENV_FILE, lines.join("\n"), { mode: 0o600 });
  await apiUnit("stop");
  await apiUnit("start");
}

/** Prints a line every 30 s while a long wait runs, so a quiet wait is not mistaken for a hang. */
export async function waitWithHeartbeat(seconds: number, what: string): Promise<void> {
  const started = Date.now();
  while (Date.now() - started < seconds * 1000) {
    const left = Math.ceil(seconds - (Date.now() - started) / 1000);
    console.log(`[heartbeat] ${what}: ${left} s left`);
    await new Promise((resolve) => setTimeout(resolve, Math.min(30_000, left * 1000)));
  }
}
