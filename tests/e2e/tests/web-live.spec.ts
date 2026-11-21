// Live behaviour of the web client (contract realtime.md): rows, thumbnails, deletes and moves arrive over the
// WebSocket without a reload, the chip follows the connection, and an idle page asks the server for nothing.
// Runs after smoke.spec.ts (alice exists). Every wait is a bounded poll.
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "../support/env.ts";
import { generateMedia, type MediaFile } from "../support/media.ts";
import { apiUnit, signInAlice } from "../support/session.ts";

const SHOTS = process.env.E2E_SHOTS ?? "test-results/shots";
const LIVE_FOLDER = "Live";
const ARCHIVE = "Archive";
const IDLE_MS = 70_000;
const PING = 0x02;
const PONG = 0x24;
const WAIT = { timeout: 60_000 };

interface Dimensions {
  naturalWidth: number;
}

test.describe.configure({ mode: "serial" });

let contextA: BrowserContext;
let contextB: BrowserContext;
let a: Page;
let b: Page;
let media: MediaFile[];
let workDir: string;
let origin: string;
/** Requests B makes while armed, as "METHOD path". */
const bRequests: string[] = [];
let armed = false;
const bErrors: string[] = [];
const frames: { dir: "sent" | "received"; type: number; at: number }[] = [];

const region = (page: Page) => page.getByRole("region", { name: "Files" });
const rowOf = (page: Page, name: string): Locator => region(page).locator("li", { has: page.getByTitle(name, { exact: true }) });
const chip = (page: Page) => page.locator('[aria-label^="Live updates:"]');
const mediaNamed = (name: string) => media.find((m) => m.name === name)!;
const titles = (page: Page) => region(page).locator("a[title], button[title]").evaluateAll((els) => els.map((e) => e.getAttribute("title")).filter((t) => t !== "Download" && t !== "Download and check" && t !== "Delete" && t !== "Share link…" && !t?.startsWith("Folder sharing is not available")));

async function openGrid(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await page.getByRole("button", { name: "Photo grid" }).click();
}

async function thumbWidth(page: Page, name: string): Promise<number> {
  const img = rowOf(page, name).locator('img[src*="/thumbnail?size=small"]');
  if ((await img.count()) === 0) return 0;
  return img.evaluate((element) => (element as unknown as Dimensions).naturalWidth);
}

test.beforeAll(async ({ browser }) => {
  origin = readAcceptanceEnv().ACC_ORIGIN;
  workDir = await mkdtemp(join(tmpdir(), "sunnycloud-live-"));
  media = await generateMedia(workDir);
  contextA = await browser.newContext();
  a = await signInAlice(contextA);
});

test.afterAll(async () => {
  await contextA?.close();
  await contextB?.close();
  await rm(workDir, { recursive: true, force: true });
});

test("B opens the same folder as A, is Live, and stays idle", async ({ browser }) => {
  // Arrange: A makes the folder.
  await a.getByRole("button", { name: "Photo grid" }).click();
  await a.getByRole("button", { name: "New folder" }).click();
  await a.getByLabel("Folder name").fill(LIVE_FOLDER);
  await a.getByRole("button", { name: "Create" }).click();
  await region(a).getByRole("link", { name: LIVE_FOLDER, exact: true }).click();
  await expect(a.getByText("This folder is empty")).toBeVisible();
  const folderUrl = a.url();

  // Act: B is a second context carrying alice's session; it only watches.
  contextB = await browser.newContext({ storageState: await contextA.storageState() });
  b = await contextB.newPage();
  b.on("pageerror", (error) => bErrors.push(`pageerror: ${error.message}`));
  b.on("console", (message) => {
    if (message.type() === "error") bErrors.push(`console.error: ${message.text()}`);
  });
  b.on("request", (request) => {
    if (armed) bRequests.push(`${request.method()} ${new URL(request.url()).pathname}${new URL(request.url()).search}`);
  });
  b.on("websocket", (socket) => {
    const note = (dir: "sent" | "received") => (event: { payload: string | Buffer }) => {
      if (typeof event.payload !== "string") frames.push({ dir, type: event.payload[0], at: Date.now() });
    };
    socket.on("framesent", note("sent"));
    socket.on("framereceived", note("received"));
  });
  await openGrid(b, folderUrl);

  // Assert
  await expect(b.getByText("This folder is empty")).toBeVisible();
  await expect(chip(b)).toHaveAttribute("aria-label", "Live updates: Live", WAIT);
  await b.screenshot({ path: `${SHOTS}/10-live-b-idle.png` });
});

test("(1) uploads in A appear in B without a reload, and their thumbnails turn ready by themselves", async () => {
  // Arrange: from here on B may make no listing, thumbnail-state or changes request; a marker proves no reload.
  // Plain script: it records the rows that were ever shown as "Preview pending".
  await b.evaluate(`(() => {
    window.__marker = "no-reload";
    window.__pending = [];
    new MutationObserver(() => {
      for (const row of document.querySelectorAll("li")) {
        if (row.textContent.includes("Preview pending")) {
          const name = row.querySelector("[title]")?.getAttribute("title") ?? "?";
          if (!window.__pending.includes(name)) window.__pending.push(name);
        }
      }
    }).observe(document.body, { childList: true, subtree: true, characterData: true });
  })()`);
  bRequests.length = 0;
  armed = true;
  const names = ["photo.jpg", "shot.heic", "clip.mp4"];

  // Act
  await a.locator('input[type="file"]').setInputFiles(names.map((n) => mediaNamed(n).path));

  // Assert: the rows, then the thumbnails, in B, without B asking.
  for (const name of names) await expect(rowOf(b, name), `${name} appears in B`).toBeVisible(WAIT);
  for (const name of names) {
    await expect.poll(() => thumbWidth(b, name), { ...WAIT, message: `${name} thumbnail in B` }).toBeGreaterThan(0);
  }
  await expect(a.getByRole("heading", { name: /Transfers/ })).toContainText("0 active", WAIT);
  expect(await b.evaluate(() => (globalThis as unknown as { __marker?: string }).__marker), "B was not reloaded").toBe("no-reload");
  const pending = await b.evaluate(() => (globalThis as unknown as { __pending: string[] }).__pending);
  console.log(`LIVE (1) rows seen as "Preview pending" in B before turning ready: ${pending.join(", ") || "none"}; B requests: ${JSON.stringify(bRequests)}`);
  const other = bRequests.filter((r) => !/^GET \/api\/v1\/nodes\/[0-9a-f-]+\/thumbnail\?size=small$/.test(r));
  expect(other, "B's requests besides loading the ready thumbnail images").toEqual([]);
  expect(bErrors, "B console errors").toEqual([]);
  await b.screenshot({ path: `${SHOTS}/11-live-b-rows-and-thumbs.png`, fullPage: true });
});

test("(2) a delete and a move-out in A remove the row in B without a reload", async () => {
  // Arrange: a sub-folder, made in A, shows up in B live.
  await a.getByRole("button", { name: "New folder" }).click();
  await a.getByLabel("Folder name").fill(ARCHIVE);
  await a.getByRole("button", { name: "Create" }).click();
  await expect(rowOf(b, ARCHIVE), "the new folder appears in B").toBeVisible(WAIT);
  const archiveId = (await rowOf(a, ARCHIVE).getByRole("link", { name: ARCHIVE, exact: true }).getAttribute("href"))!.split("/").pop()!;
  const heicId = (await rowOf(a, "shot.heic").getByRole("link", { name: "Download shot.heic" }).getAttribute("href"))!.split("/")[4];

  // Act 1: delete through the UI in A.
  await rowOf(a, "clip.mp4").getByRole("button", { name: "Delete clip.mp4" }).click();
  await rowOf(a, "clip.mp4").getByRole("group", { name: "Confirm delete" }).getByRole("button", { name: "Delete", exact: true }).click();

  // Assert 1
  await expect(rowOf(b, "clip.mp4"), "the deleted file leaves B").toHaveCount(0, WAIT);

  // Act 2: move a file out of the open folder with the API (the web UI has no move yet), as another client would.
  const moved = await contextA.request.patch(`/api/v1/nodes/${heicId}`, { headers: { origin }, data: { parent_id: archiveId } });
  expect(moved.status(), await moved.text()).toBe(200);

  // Assert 2
  await expect(rowOf(b, "shot.heic"), "the moved file leaves B").toHaveCount(0, WAIT);
  await expect(rowOf(b, "photo.jpg")).toBeVisible();
  await expect(rowOf(b, ARCHIVE)).toBeVisible();
  expect(await b.evaluate(() => (globalThis as unknown as { __marker?: string }).__marker), "B was not reloaded").toBe("no-reload");
  const other = bRequests.filter((r) => !/^GET \/api\/v1\/nodes\/[0-9a-f-]+\/thumbnail\?size=small$/.test(r));
  expect(other, "B's requests besides thumbnail images").toEqual([]);
  expect(bErrors, "B console errors").toEqual([]);
  await b.screenshot({ path: `${SHOTS}/12-live-b-after-delete-and-move.png` });
});

test("(3) stopping the API turns B's chip to Reconnecting, starting it returns it to Live and catches up", async () => {
  // Arrange
  armed = false;
  await expect(chip(b)).toHaveAttribute("aria-label", "Live updates: Live");

  // Act 1: the API goes away.
  await apiUnit("stop");

  // Assert 1
  await expect(chip(b), "chip while the API is down").toHaveAttribute("aria-label", "Live updates: Reconnecting", WAIT);
  await b.screenshot({ path: `${SHOTS}/13-live-b-reconnecting.png` });

  // Act 2: it comes back, then A uploads after the restart.
  await apiUnit("start");
  await expect(chip(b), "chip after the API is back").toHaveAttribute("aria-label", "Live updates: Live", { timeout: 90_000 });
  await a.locator('input[type="file"]').setInputFiles({ name: "after-restart.png", mimeType: "image/png", buffer: await readFile(mediaNamed("drawing.png").path) });

  // Assert 2: the file reaches B by itself, once, with a thumbnail, and nothing else was duplicated.
  await expect(rowOf(b, "after-restart.png"), "after-restart.png appears in B").toBeVisible(WAIT);
  await expect.poll(() => thumbWidth(b, "after-restart.png"), WAIT).toBeGreaterThan(0);
  const shown = await titles(b);
  expect(new Set(shown).size, `no duplicate rows in B: ${shown.join(", ")}`).toBe(shown.length);
  expect(shown.sort()).toEqual([ARCHIVE, "after-restart.png", "photo.jpg"].sort());
  expect(await b.evaluate(() => (globalThis as unknown as { __marker?: string }).__marker), "B was not reloaded").toBe("no-reload");
  await b.screenshot({ path: `${SHOTS}/14-live-b-after-restart.png` });
});

test("(4) an idle B makes no REST request for 70 s, keeps pinging and stays Live", async () => {
  test.setTimeout(180_000);
  // Arrange
  await expect(chip(b)).toHaveAttribute("aria-label", "Live updates: Live");
  bRequests.length = 0;
  const started = Date.now();
  frames.length = 0;
  armed = true;
  const notLive: string[] = [];

  // Act: observe once a second for 70 s (a bounded poll, not a sleep).
  await expect
    .poll(
      async () => {
        const label = await chip(b).getAttribute("aria-label");
        if (label !== "Live updates: Live") notLive.push(`${Math.round((Date.now() - started) / 1000)} s: ${label}`);
        return Date.now() - started;
      },
      { intervals: [1000], timeout: IDLE_MS + 20_000, message: "idle observation" },
    )
    .toBeGreaterThanOrEqual(IDLE_MS);
  armed = false;

  // Assert
  expect(bRequests, "REST requests of an idle page").toEqual([]);
  expect(notLive, "the chip left Live").toEqual([]);
  const pings = frames.filter((f) => f.dir === "sent" && f.type === PING).length;
  const pongs = frames.filter((f) => f.dir === "received" && f.type === PONG).length;
  console.log(`LIVE (4) idle ${IDLE_MS / 1000} s: ${pings} Ping sent, ${pongs} Pong received, ${bRequests.length} REST requests, chip not Live ${notLive.length} times`);
  expect(pings, "client Pings in 70 s (every 30 s)").toBeGreaterThanOrEqual(2);
  expect(pongs, "server Pongs").toBe(pings);
  expect(bErrors.filter((e) => !e.includes("WebSocket connection")), "B console errors while idle").toEqual([]);
  await b.screenshot({ path: `${SHOTS}/15-live-b-after-idle.png` });
});

test("(5) a session deleted server-side ends the live page: B signs out", async ({ browser }) => {
  // Arrange: a third context signs in as alice for real, so it has a session of its own.
  const contextC = await browser.newContext();
  const c = await signInAlice(contextC);
  await openGrid(c, a.url());
  await expect(chip(c)).toHaveAttribute("aria-label", "Live updates: Live", WAIT);
  const list = await contextA.request.get("/api/v1/sessions");
  expect(list.status()).toBe(200);
  const sessions = ((await list.json()) as { items: { id: string; created_at: string }[] }).items;
  const newest = [...sessions].sort((x, y) => y.created_at.localeCompare(x.created_at))[0];
  expect(sessions.length, "alice has more than one session").toBeGreaterThan(1);

  // Act: A's cookie revokes C's session.
  const revoked = await contextA.request.delete(`/api/v1/sessions/${newest.id}`, { headers: { origin } });
  expect(revoked.status(), await revoked.text()).toBeLessThan(300);

  // Assert: C leaves the file browser for sign-in by itself (close code 4401), A is untouched.
  await expect(c, "C goes to the sign-in page").toHaveURL(/\/login/, WAIT);
  await c.screenshot({ path: `${SHOTS}/16-live-c-signed-out.png` });
  expect((await contextA.request.get("/api/v1/accounts/me")).status()).toBe(200);
  await contextC.close();
});
