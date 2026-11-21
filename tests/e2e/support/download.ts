import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import type { Page } from "@playwright/test";

export const MIB = 1024 * 1024;
const BLOCK = 2 * MIB;

export const FIREFOX_UA = "Mozilla/5.0 (X11; Linux x86_64; rv:159.0) Gecko/20100101 Firefox/159.0";
export const SAFARI_UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15";

/** What Chromium's save dialog returns, minus the dialog (it cannot be driven headless): a real file handle, in OPFS. */
export const PICKER_TO_OPFS = `window.showSaveFilePicker = async ({ suggestedName }) => {
  const root = await navigator.storage.getDirectory();
  window.__pickedName = "saved-" + crypto.randomUUID().slice(0, 8) + "-" + suggestedName;
  return root.getFileHandle(window.__pickedName, { create: true });
};`;

/** A browser that has no save dialog (Firefox, Safari). */
export const NO_PICKER = `Object.defineProperty(window, "showSaveFilePicker", { value: undefined, configurable: true });`;

/** SHA-256 of every 2 MiB block of a file on disk. */
export async function blockHashesOfPath(path: string): Promise<string[]> {
  const hashes: string[] = [];
  let pending = Buffer.alloc(0);
  for await (const chunk of createReadStream(path, { highWaterMark: BLOCK })) {
    pending = Buffer.concat([pending, chunk as Buffer]);
    while (pending.length >= BLOCK) {
      hashes.push(createHash("sha256").update(pending.subarray(0, BLOCK)).digest("hex"));
      pending = pending.subarray(BLOCK);
    }
  }
  if (pending.length > 0) hashes.push(createHash("sha256").update(pending).digest("hex"));
  return hashes;
}

/** Runs a script in the page and returns its value (Playwright's `page.evaluate` or the raw DevTools one). */
export type Evaluate = (script: string) => Promise<unknown>;

/** The file the stubbed dialog handed out, hashed per 2 MiB block inside the page, or null when it does not exist. */
export async function pickedFile(evaluate: Evaluate): Promise<{ size: number; blocks: string[] } | null> {
  return evaluate(`(async () => {
    const root = await navigator.storage.getDirectory();
    let file;
    try {
      file = await (await root.getFileHandle(window.__pickedName)).getFile();
    } catch (error) {
      if (error.name === "NotFoundError") return null;
      throw error;
    }
    const blocks = [];
    for (let at = 0; at < file.size; at += ${BLOCK}) {
      const digest = await crypto.subtle.digest("SHA-256", await file.slice(at, at + ${BLOCK}).arrayBuffer());
      blocks.push(Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join(""));
    }
    return { size: file.size, blocks };
  })()`) as Promise<{ size: number; blocks: string[] } | null>;
}
