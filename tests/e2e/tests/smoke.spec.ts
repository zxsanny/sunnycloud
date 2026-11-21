import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { setupToken } from "../support/env.ts";
import { generateMedia, hashFile, HOSTILE_NAME, LARGE_MIB, writeRandom, type MediaFile } from "../support/media.ts";

interface Dimensions {
  naturalWidth: number;
}

const SHOTS = process.env.E2E_SHOTS ?? "test-results/shots";
const THUMB_WAIT_MS = 150_000;
const FOLDER = "Holiday 2026";

test.describe.configure({ mode: "serial" });

let context: BrowserContext;
let page: Page;
let media: MediaFile[];
let workDir: string;
const consoleProblems: string[] = [];

test.beforeAll(async ({ browser }) => {
  workDir = await mkdtemp(join(tmpdir(), "sunnycloud-e2e-"));
  media = await generateMedia(workDir);
  context = await browser.newContext();
  page = await context.newPage();
  page.on("pageerror", (error) => consoleProblems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") consoleProblems.push(`console.${message.type()}: ${message.text()}`);
  });
});

test.afterAll(async () => {
  await context.close();
  await rm(workDir, { recursive: true, force: true });
});

const files = () => page.getByRole("region", { name: "Files" });
const rowOf = (name: string): Locator => files().locator("li", { has: page.getByTitle(name, { exact: true }) });
const byExpect = (kind: string) => media.filter((m) => m.expect === kind);

async function sha256OfDownload(link: Locator): Promise<string> {
  const [download] = await Promise.all([page.waitForEvent("download"), link.click()]);
  const path = await download.path();
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

/** Names of the previewable rows whose small thumbnail is a loaded image, plus its width. */
async function loadedThumbs(): Promise<Record<string, number>> {
  const loaded: Record<string, number> = {};
  for (const item of media.filter((m) => m.expect !== "none")) {
    const row = rowOf(item.name);
    if ((await row.count()) === 0) continue;
    const img = row.locator('img[src*="/thumbnail?size=small"]');
    if ((await img.count()) === 0) continue;
    await img.scrollIntoViewIfNeeded();
    const width = await img.evaluate((element) => (element as unknown as Dimensions).naturalWidth);
    if (width > 0) loaded[item.name] = width;
  }
  return loaded;
}

test("first-run setup: the setup token and the mock IdP lead to the file browser", async () => {
  // Arrange
  const token = await setupToken();

  // Act
  await page.goto("/setup");
  await page.screenshot({ path: `${SHOTS}/01-setup-token.png` });
  await page.getByLabel("Setup token").fill(token);
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: /OpenID Connect/ }).click();
  await expect(page.getByRole("heading", { name: "Files", level: 1 })).toBeVisible();

  // Assert: a Secure, HttpOnly __Host- session cookie works on http://localhost, and the setup page is gone for good.
  const session = (await context.cookies()).find((cookie) => cookie.name === "__Host-session");
  expect(session, "the session cookie was kept by the browser").toBeDefined();
  expect(session).toMatchObject({ secure: true, httpOnly: true, sameSite: "Lax", path: "/" });
  await page.goto("/setup");
  await expect(page.getByLabel("Setup token")).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/02-signed-in.png` });
});

test("creates a folder and opens it", async () => {
  // Arrange
  await page.goto("/");
  await page.getByRole("button", { name: "Photo grid" }).click();

  // Act
  await page.getByRole("button", { name: "New folder" }).click();
  await page.getByLabel("Folder name").fill(FOLDER);
  await page.getByRole("button", { name: "Create" }).click();
  await files().getByRole("link", { name: FOLDER, exact: true }).click();

  // Assert
  await expect(page).toHaveURL(/\/folders\/[0-9a-f-]+$/);
  await expect(page.getByText("This folder is empty")).toBeVisible();
});

test("uploads the media set through the real file input", async () => {
  // Act
  await page.locator('input[type="file"]').setInputFiles(media.map((m) => m.path));

  // Assert: every file lands in the folder, the transfers panel settles and nothing failed.
  for (const item of media) await expect(rowOf(item.name), `${item.name} appears`).toBeVisible({ timeout: 180_000 });
  await expect(page.getByRole("heading", { name: /Transfers/ })).toContainText("0 active", { timeout: 60_000 });
  await expect(page.getByText("Not uploaded")).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/03-uploaded.png` });
});

test("after a reload every image and video shows a real thumbnail and there is no <video>", async () => {
  // Act: the previews come from the real worker and decoder, so reload until they are ready (bounded).
  const expected = byExpect("image").concat(byExpect("video")).map((m) => m.name).sort();
  let loaded: Record<string, number> = {};
  await expect
    .poll(
      async () => {
        await page.reload();
        await expect(rowOf(media[0].name)).toBeVisible();
        loaded = await loadedThumbs();
        return Object.keys(loaded).sort();
      },
      { intervals: [3000], timeout: THUMB_WAIT_MS, message: "previews: every image and video row loads its small thumbnail" },
    )
    .toEqual(expected);

  // Assert
  for (const [name, width] of Object.entries(loaded)) expect(width, `${name} thumbnail width`).toBeLessThanOrEqual(320);
  await expect(page.locator("video")).toHaveCount(0);
  for (const item of byExpect("video")) await expect(rowOf(item.name).getByText("Video", { exact: true })).toBeVisible();
  for (const item of byExpect("none").filter((m) => m.name === "large.bin")) {
    await expect(rowOf(item.name).getByText("No preview")).toHaveCount(1);
  }
  await page.screenshot({ path: `${SHOTS}/04-thumbnails-after-reload.png`, fullPage: true });
});

test("the lightbox opens the 1440 px preview of an image", async () => {
  // Act
  const photo = media.find((m) => m.name === "photo.jpg")!;
  await rowOf(photo.name).getByRole("button", { name: photo.name, exact: true }).click();
  const dialog = page.getByRole("dialog", { name: photo.name });
  const preview = dialog.locator("img");

  // Assert
  await expect(preview).toHaveAttribute("src", /\/thumbnail\?size=preview$/);
  await expect.poll(() => preview.evaluate((img) => (img as unknown as Dimensions).naturalWidth)).toBe(1440);
  await page.screenshot({ path: `${SHOTS}/05-lightbox-image.png` });
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("a download has the same SHA-256 as the source", async () => {
  for (const name of ["large.bin", "drawing.png", "shot.heic"]) {
    // Act
    const source = media.find((m) => m.name === name)!;
    const digest = await sha256OfDownload(page.getByRole("link", { name: `Download ${name}` }));

    // Assert
    expect(digest, `${name} (${source.bytes} bytes)`).toBe(source.sha256);
  }
});

test("a hostile file name is shown as text, never as markup", async () => {
  // Arrange
  await page.reload();
  const row = rowOf(HOSTILE_NAME);
  await expect(row).toBeVisible();
  const link = row.getByRole("link", { name: HOSTILE_NAME, exact: true });

  // Assert
  await expect(link).toHaveText(HOSTILE_NAME);
  expect(await link.evaluate((element) => element.innerHTML)).toContain("&lt;img src=x");
  expect(await page.locator('img[src="x"]').count()).toBe(0);
  expect(await page.evaluate(() => (globalThis as unknown as { __xss?: number }).__xss)).toBeUndefined();
  const digest = await sha256OfDownload(page.getByRole("link", { name: `Download ${HOSTILE_NAME}` }));
  expect(digest).toBe(media.find((m) => m.name === HOSTILE_NAME)!.sha256);
});

test("the run produced no CSP violation, page error or console error", async () => {
  // Assert
  expect(consoleProblems, consoleProblems.join("\n")).toEqual([]);
});

// Last on purpose: a failure here skips nothing after it. A video has only its poster (a `thumb`, 320 px).
test("the lightbox shows the poster of a video", async () => {
  // Arrange
  const clip = media.find((m) => m.name === "clip.mp4")!;
  await page.reload();

  // Act
  await rowOf(clip.name).getByRole("button", { name: clip.name, exact: true }).click();

  // Assert
  const poster = page.getByRole("dialog", { name: clip.name }).locator("img");
  await expect(poster).toHaveAttribute("src", /\/thumbnail\?size=small$/);
  await expect.poll(() => poster.evaluate((img) => (img as unknown as Dimensions).naturalWidth)).toBeGreaterThan(0);
  await expect(page.getByText("Video playback is not available yet")).toBeVisible();
  await expect(page.locator("video")).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/06-lightbox-video-poster.png` });
});

const EDGE_FOLDER = "Edge cases";
const LONG_NAME = `${"n".repeat(251)}.txt`;
const UNICODE_NAME = "Фото відпустки 🌴 — Київ.txt";
const DUPLICATE_NAME = "same-name.txt";
const FAKE_JPG = "not-really.jpg";

async function openNewFolder(name: string): Promise<void> {
  await page.goto("/");
  await page.getByRole("button", { name: "New folder" }).click();
  await page.getByLabel("Folder name").fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  await files().getByRole("link", { name, exact: true }).click();
  await expect(page).toHaveURL(/\/folders\/[0-9a-f-]+$/);
  // Picks made before the folder has loaded are dropped by the page, so wait for the loaded (empty) state.
  await expect(page.getByText("This folder is empty")).toBeVisible();
}

test("edge-case files end in a clear UI state: uploaded, or a typed error; never a hang or a blank page", async ({}, testInfo) => {
  // Arrange
  await openNewFolder(EDGE_FOLDER);
  const text = (content: string) => ({ mimeType: "text/plain", buffer: Buffer.from(content) });
  const payloads = [
    { name: "empty.dat", mimeType: "application/octet-stream", buffer: Buffer.alloc(0) },
    { name: LONG_NAME, ...text("255 character name\n") },
    { name: UNICODE_NAME, ...text("Привіт, світе 🌍\n") },
    { name: DUPLICATE_NAME, ...text("first copy\n") },
    { name: DUPLICATE_NAME, ...text("second copy, different bytes\n") },
    { name: FAKE_JPG, mimeType: "image/jpeg", buffer: Buffer.from("this is plain text renamed to .jpg\n") },
  ];
  const errorsBefore = consoleProblems.length;

  // Act
  await page.locator('input[type="file"]').setInputFiles(payloads);
  await expect(page.getByRole("heading", { name: /Transfers/ })).toContainText("0 active", { timeout: 120_000 });

  // Assert: what each name ended as. A name is fine when it is a row, or a failed transfer shows its error text.
  const outcome: Record<string, { rows: number; failedTransfers: string[] }> = {};
  for (const name of new Set(payloads.map((p) => p.name))) {
    const failed = page.getByRole("complementary").locator("li", { has: page.getByTitle(name, { exact: true }) }).getByText(/Not uploaded/);
    outcome[name] = { rows: await rowOf(name).count(), failedTransfers: await failed.allTextContents() };
  }
  await testInfo.attach("edge-outcomes.json", { body: JSON.stringify(outcome, null, 2), contentType: "application/json" });
  console.log(`EDGE OUTCOMES ${JSON.stringify(outcome)}`);
  for (const [name, state] of Object.entries(outcome)) {
    expect(state.rows + state.failedTransfers.length, `${name.slice(0, 40)}: neither a row nor an error`).toBeGreaterThan(0);
  }
  // Same name twice in one folder: the product keeps both, the second under a "conflict copy" name.
  const sameNames = await files().locator('a[title^="same-name"], button[title^="same-name"]').evaluateAll((els) => els.map((e) => e.getAttribute("title")));
  console.log(`DUPLICATE NAME RESULT ${JSON.stringify(sameNames)}`);
  expect(sameNames, "both same-name files are listed").toHaveLength(2);
  expect(sameNames).toContain(DUPLICATE_NAME);
  expect(sameNames.find((name) => name !== DUPLICATE_NAME)).toMatch(/^same-name \(conflict copy .+\)\.txt$/);
  // Unicode names survive the round trip, in the file name and the bytes. The transfers panel covers the grid's corner.
  await page.getByRole("button", { name: "Clear finished" }).click();
  const [unicode] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: `Download ${UNICODE_NAME}` }).click()]);
  expect(unicode.suggestedFilename()).toBe(UNICODE_NAME);
  expect(await readFile(await unicode.path(), "utf8")).toBe("Привіт, світе 🌍\n");
  await expect(files()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/07-edge-cases-uploaded.png`, fullPage: true });

  // The fake JPEG is not decodable: it ends as a no-preview icon (bounded wait for the job to give up), never a spinner.
  if (outcome[FAKE_JPG].rows === 1) {
    await expect
      .poll(
        async () => {
          await page.reload();
          await expect(rowOf(FAKE_JPG)).toBeVisible();
          return await rowOf(FAKE_JPG).getByText("No preview").count();
        },
        { intervals: [3000], timeout: THUMB_WAIT_MS, message: `${FAKE_JPG} ends as "No preview"` },
      )
      .toBe(1);
  }
  await page.screenshot({ path: `${SHOTS}/08-edge-cases-final.png`, fullPage: true });
  expect(consoleProblems.slice(errorsBefore).filter((m) => m.startsWith("pageerror")), "uncaught page errors").toEqual([]);
});

test("two browser contexts of the same user upload the same 40 MiB file at once and both finish", async ({ browser }) => {
  // Arrange: a fresh 40 MiB file, two names of the same bytes, and a second context that carries alice's session.
  const first = join(workDir, "concurrent-a.bin");
  const second = join(workDir, "concurrent-b.bin");
  await writeRandom(first, LARGE_MIB);
  await copyFile(first, second);
  const { sha256 } = await hashFile(first);
  await openNewFolder("Concurrent");
  const folderUrl = page.url();
  const otherContext = await browser.newContext({ storageState: await context.storageState() });
  const other = await otherContext.newPage();
  await other.goto(folderUrl);
  await expect(other.getByText("This folder is empty")).toBeVisible();

  // Act
  await Promise.all([
    page.locator('input[type="file"]').setInputFiles(first),
    other.locator('input[type="file"]').setInputFiles(second),
  ]);

  // Assert: both transfers settle in both contexts, and the listing shows both files with the right bytes.
  for (const view of [page, other]) {
    await expect(view.getByRole("heading", { name: /Transfers/ })).toContainText("0 active", { timeout: 180_000 });
    await expect(view.getByText("Not uploaded")).toHaveCount(0);
  }
  await page.reload();
  await other.reload();
  for (const view of [page, other]) {
    const region = view.getByRole("region", { name: "Files" });
    await expect(region.getByTitle("concurrent-a.bin", { exact: true })).toBeVisible();
    await expect(region.getByTitle("concurrent-b.bin", { exact: true })).toBeVisible();
  }
  const [download] = await Promise.all([other.waitForEvent("download"), other.getByRole("link", { name: "Download concurrent-a.bin" }).click()]);
  const bytes = createHash("sha256").update(await readFile(await download.path())).digest("hex");
  expect(bytes).toBe(sha256);
  await other.screenshot({ path: `${SHOTS}/09-concurrent-second-context.png` });
  await otherContext.close();
});
