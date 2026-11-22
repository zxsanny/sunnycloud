// Download strategies and outcome labels (AZ-3942), in a real Chromium against the real stack. Scaled sizes: files here
// are 64 to 256 MiB; the 2 GiB boundary is proven in the unit tests with the limit as a config value, and the heap bound
// by `bun run check:large` (the engine at 1 and 5 GiB). A browser-level heap number is not taken here: Playwright's
// network tracking keeps copies of response bodies in the renderer (about 240 MiB for 256 MiB) and would be read as the
// app's memory; the browser-level runs belong to AZ-3952. Firefox and Safari cannot run here: the "Firefox" and "Safari" contexts below
// are Chromium with the user agent of that browser and no save dialog, so they run the Firefox mechanism (OPFS ranged
// pull) and the labelled-unverified path on the Chromium engine, not on those engines. Runs after smoke.spec.ts (alice).
import { createHash } from "node:crypto";
import { mkdtemp, open, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { FIREFOX_UA, MIB, NO_PICKER, PICKER_TO_OPFS, SAFARI_UA, blockHashesOfPath, pickedFile } from "../support/download.ts";
import { writeRandomFile } from "../support/resume.ts";
import { ACC_DATA } from "../support/env.ts";
import { filesPage, signInAlice } from "../support/session.ts";

const FOLDER = `Download ${Date.now()}`;
const WAIT = { timeout: 120_000 };
const BIG = "dl-big.bin";
const MID = "dl-mid.bin";
const SMALL = "dl-small.bin";

test.describe.configure({ mode: "serial" });

let owner: BrowserContext;
let workDir: string;
let folderUrl: string;
const files = new Map<string, { path: string; sha256: string; mib: number }>();
const problems: string[] = [];

const fileRow = (page: Page, name: string) => page.getByRole("region", { name: "Files" }).getByTitle(name, { exact: true });
const panel = (page: Page) => page.getByRole("complementary");
const panelRow = (page: Page, name: string) => panel(page).locator("li", { hasText: name });
const verifyButton = (page: Page, name: string) => page.getByRole("button", { name: `Download ${name} with check`, exact: true });

function watch(page: Page): Page {
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console.error: ${message.text()}`);
  });
  return page;
}

/** A page of alice's session with the browser identity of the test: its user agent, and the save dialog it has. */
async function openAs(options: { userAgent?: string; picker: boolean; cutRealtime?: boolean }): Promise<{ context: BrowserContext; page: Page }> {
  const state = await owner.storageState();
  const browser = owner.browser();
  if (!browser) throw new Error("the owner context has no browser");
  const context = await browser.newContext({ acceptDownloads: true, storageState: state, userAgent: options.userAgent });
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await context.addInitScript(options.picker ? PICKER_TO_OPFS : NO_PICKER);
  const page = watch(await context.newPage());
  if (options.cutRealtime) await page.routeWebSocket(/\/api\/v1\/realtime/, (socket) => socket.close());
  await page.goto(folderUrl);
  await expect(filesPage(page)).toBeVisible();
  await expect(fileRow(page, SMALL)).toBeVisible(WAIT);
  return { context, page };
}

/** The server's file hash of a node, from the API the page itself uses. */
async function serverHash(page: Page, name: string): Promise<string> {
  const folderId = folderUrl.split("/").pop();
  const children = (await (await page.request.get(`/api/v1/nodes/${folderId}/children?limit=200`)).json()) as { items: { name: string; content_hash: string }[] };
  const found = children.items.find((item) => item.name === name);
  if (!found) throw new Error(`no node named ${name}`);
  return found.content_hash;
}

test.beforeAll(async ({ browser }) => {
  workDir = await mkdtemp(join(tmpdir(), "sunnycloud-download-"));
  owner = await browser.newContext({ acceptDownloads: true });
  await owner.grantPermissions(["clipboard-read", "clipboard-write"]);
});

test.afterAll(async () => {
  await owner?.close();
  await rm(workDir, { recursive: true, force: true });
});

test("a folder with a 256 MiB, a 64 MiB and a 6 MiB file", async () => {
  // Arrange
  const main = watch(await signInAlice(owner));
  await main.getByRole("button", { name: "Photo grid" }).click();
  await main.getByRole("button", { name: "New folder" }).click();
  await main.getByLabel("Folder name").fill(FOLDER);
  await main.getByRole("button", { name: "Create" }).click();
  await main.getByRole("region", { name: "Files" }).getByRole("link", { name: FOLDER, exact: true }).click();
  await expect(main.getByText("This folder is empty")).toBeVisible();
  folderUrl = main.url();
  for (const [name, mib] of [[BIG, 256], [MID, 64], [SMALL, 6]] as const) {
    files.set(name, { ...(await writeRandomFile(workDir, name, mib)), mib });
  }

  // Act
  await main.locator('input[type="file"]').setInputFiles([...files.values()].map((file) => file.path));

  // Assert
  await expect(panel(main).getByRole("heading", { name: /Transfers/ })).toContainText("0 active", WAIT);
  for (const name of files.keys()) await expect(fileRow(main, name)).toBeVisible(WAIT);
  await main.close();
});

test("AC-1: Chromium streams 256 MiB into the chosen file, verified, and the saved bytes equal the source", async () => {
  // Arrange
  const { context, page } = await openAs({ picker: true });
  const source = files.get(BIG)!;
  const ranges: string[] = [];
  page.on("request", (request) => {
    const range = request.headers()["range"];
    if (/\/api\/v1\/nodes\/[0-9a-f-]+\/content$/.test(new URL(request.url()).pathname) && range) ranges.push(range);
  });

  // Act
  await verifyButton(page, BIG).click();

  // Assert
  await expect(panelRow(page, BIG)).toContainText("Verified", WAIT);
  await expect(panelRow(page, BIG)).toContainText("Downloaded and checked in the browser.");
  const saved = await pickedFile((script) => page.evaluate(script));
  expect(saved?.size).toBe(source.mib * MIB);
  expect(saved?.blocks, "the saved file equals the source, block by block").toEqual(await blockHashesOfPath(source.path));
  expect(ranges.length, "ranged pulls of four blocks").toBe(Math.ceil((source.mib * MIB) / (8 * MIB)));
  await context.close();
});

test("the Firefox mechanism (OPFS pull, run on the Chromium engine): verified, then the browser saves the file", async () => {
  // Arrange
  const { context, page } = await openAs({ userAgent: FIREFOX_UA, picker: false });
  const source = files.get(MID)!;

  // Act
  const [download] = await Promise.all([page.waitForEvent("download", WAIT), verifyButton(page, MID).click()]);
  const saved = createHash("sha256").update(await readFile(await download.path())).digest("hex");

  // Assert
  await expect(panelRow(page, MID)).toContainText("Verified", WAIT);
  expect(download.suggestedFilename()).toBe(MID);
  expect(saved).toBe(source.sha256);
  await context.close();
});

test("a browser with no verified mechanism (the Safari path): its own download, labelled not verified, with the server hash and a copy action", async () => {
  // Arrange
  const { context, page } = await openAs({ userAgent: SAFARI_UA, picker: false });
  const source = files.get(MID)!;
  const hash = await serverHash(page, MID);
  const ranged: string[] = [];
  page.on("request", (request) => {
    if (request.headers()["range"]) ranged.push(request.url());
  });

  // Act
  const [download] = await Promise.all([page.waitForEvent("download", WAIT), verifyButton(page, MID).click()]);
  const saved = createHash("sha256").update(await readFile(await download.path())).digest("hex");
  await page.getByRole("button", { name: "Copy file hash" }).click();

  // Assert
  const row = panelRow(page, MID);
  await expect(row).toContainText("Not verified");
  await expect(row).toContainText("Saved by the browser without a check");
  await expect(row).toContainText("Compare its hash:");
  await expect(row.getByText(hash, { exact: true })).toBeVisible();
  expect(await page.evaluate(`navigator.clipboard.readText()`)).toBe(hash);
  expect(saved, "the browser's own download is the file").toBe(source.sha256);
  expect(ranged.filter((url) => url.includes("/content")), "the page read no byte itself").toEqual([]);
  await expect(row).not.toContainText("Verified");
  await context.close();
});

test("AC-5: a block changed on the way ends as a mismatch in every verified path: nothing is called good, nothing is left", async () => {
  // Arrange: one bit of the third block flips in the answer that carries it (the server copy itself is tested next).
  const flip = 2 * 2 * MIB + 17;
  const tamper = (page: Page) =>
    page.route(/\/api\/v1\/nodes\/[0-9a-f-]+\/content$/, async (route) => {
      const response = await route.fetch();
      const range = /bytes=(\d+)-(\d+)/.exec(route.request().headers()["range"] ?? "");
      const body = await response.body();
      if (range && Number(range[1]) <= flip && flip <= Number(range[2])) body[flip - Number(range[1])] ^= 1;
      await route.fulfill({ response, body });
    });
  const chromium = await openAs({ picker: true });
  const firefox = await openAs({ userAgent: FIREFOX_UA, picker: false });
  await tamper(chromium.page);
  await tamper(firefox.page);
  const downloads: string[] = [];
  firefox.page.on("download", (download) => downloads.push(download.suggestedFilename()));

  // Act
  await verifyButton(chromium.page, MID).click();
  await verifyButton(firefox.page, MID).click();

  // Assert
  for (const { page } of [chromium, firefox]) {
    await expect(panelRow(page, MID)).toContainText("did not match the file on the server", WAIT);
    await expect(panelRow(page, MID)).not.toContainText("Verified");
    await expect(panelRow(page, MID)).not.toContainText("Not verified");
  }
  expect(await pickedFile((script) => chromium.page.evaluate(script)), "the file of the save dialog is removed").toBeNull();
  await firefox.page.waitForTimeout(1500);
  expect(downloads, "the browser was never handed a file").toEqual([]);
  await chromium.context.close();
  await firefox.context.close();
});

test("AC-5 on the volume: one byte of the stored copy changed on disk is never saved as good", async () => {
  // Arrange: the ETag of the content answer is the blob id, which names the file on the volume.
  const { context, page } = await openAs({ picker: true });
  const folderId = folderUrl.split("/").pop();
  const children = (await (await page.request.get(`/api/v1/nodes/${folderId}/children?limit=200`)).json()) as { items: { node_id: string; name: string }[] };
  const nodeId = children.items.find((item) => item.name === MID)!.node_id;
  const head = await page.request.get(`/api/v1/nodes/${nodeId}/content`, { headers: { Range: "bytes=0-0" } });
  const blob = (head.headers()["etag"] ?? "").replaceAll('"', "");
  const path = join(ACC_DATA, "blobs", "blobs", blob.slice(0, 2), blob.slice(2, 4), blob);
  const handle = await open(path, "r+");
  const at = 20 * MIB;
  const original = Buffer.alloc(1);
  await handle.read(original, 0, 1, at);
  try {
    await handle.write(Buffer.from([original[0] ^ 0x01]), 0, 1, at);

    // Act
    await verifyButton(page, MID).click();

    // Assert
    const row = panelRow(page, MID);
    await expect(row.locator("p.text-danger")).toBeVisible(WAIT);
    console.log(`[AC-5 volume] the row says: ${await row.locator("p.text-danger").innerText()}`);
    await expect(row).not.toContainText("Verified");
    expect(await pickedFile((script) => page.evaluate(script)), "the file of the save dialog is removed").toBeNull();
  } finally {
    await handle.write(original, 0, 1, at);
    await handle.close();
    await context.close();
  }
});

test("AC-6: a file that was deleted meanwhile ends as NotFound with its message and nothing is saved", async () => {
  // Arrange: the first tab keeps its row because its live connection is cut; the second tab deletes the file.
  const first = await openAs({ picker: true, cutRealtime: true });
  const second = await openAs({ picker: true });
  const name = SMALL;
  await second.page.getByRole("button", { name: `Delete ${name}`, exact: true }).click();
  await second.page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(fileRow(second.page, name)).toHaveCount(0);

  // Act
  await verifyButton(first.page, name).click();

  // Assert
  await expect(panel(first.page).getByText("This item no longer exists.")).toBeVisible(WAIT);
  await expect(panel(first.page)).not.toContainText("Verified");
  expect(await first.page.evaluate("window.__pickedName"), "the save dialog was never opened, so no file exists").toBeUndefined();
  await first.context.close();
  await second.context.close();
});

test("the run produced no page error or console error", async () => {
  // Assert: failed downloads are answered requests, never script errors.
  const unexpected = problems.filter((line) => !/Failed to load resource|net::ERR|ERR_ABORTED|WebSocket/.test(line));
  expect(unexpected).toEqual([]);
});
