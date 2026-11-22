// Share dialog and public share page (AZ-3943), in a real Chromium against the real stack. Runs after smoke.spec.ts (alice).
// Expiry (AC-7) uses a link of 2 seconds and the server's own clock: the acceptance stack has no test clock.
// Not here: AC-12 (PT-16-5, 100 opens over shape-wan: no shaping profile on this host), AC-11 in Firefox and WebKit
// (only Chromium is installed), and heap/engine items that web-download.spec.ts and the unit tests already cover.
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "../support/env.ts";
import { HOSTILE_NAME } from "../support/media.ts";
import { signInAlice } from "../support/session.ts";
import { PASSWORD, SECRET_BYTES, SECRET_NAME, UNAVAILABLE, createShare, createShareApi, downloadHash, dumpStorage, revokeShare, visitor, writeShareFiles, type Created, type ShareFile } from "../support/share.ts";

const FOLDER = `Share ${Date.now()}`;
const WAIT = { timeout: 120_000 };
const origin = readAcceptanceEnv().ACC_ORIGIN as string;

test.describe.configure({ mode: "serial" });

let owner: BrowserContext;
let main: Page;
let workDir: string;
let folderUrl: string;
let folderId: string;
let secretUrl: string;
let files: Record<string, ShareFile>;
const nodes = new Map<string, { node_id: string; media_type: string | null }>();
const problems: string[] = [];

const shareButton = (page: Page, name: string) => page.getByRole("button", { name: `Share link for ${name}`, exact: true });
const dialogOf = (page: Page, name: string) => page.getByRole("dialog", { name: `Share ${name}` });
const created = (dialog: ReturnType<typeof dialogOf>) => dialog.getByRole("region", { name: "Link created" });

async function openDialog(page: Page, name: string) {
  await shareButton(page, name).click();
  const dialog = dialogOf(page, name);
  await expect(dialog.getByText("Links for this file")).toBeVisible();
  await expect(dialog.getByText("Loading")).toHaveCount(0);
  return dialog;
}

async function closeDialog(dialog: ReturnType<typeof dialogOf>) {
  await dialog.getByRole("button", { name: "Close share dialog" }).click();
  await expect(dialog).toHaveCount(0);
}

/** The text of what a visitor sees: the page body, one string. */
const mainText = (page: Page) => page.getByRole("main").innerText();

async function visit(browser: import("@playwright/test").Browser, url: string) {
  const { context, page } = await visitor(browser, problems);
  await page.goto(url);
  return { context, page };
}

test.beforeAll(async ({ browser }) => {
  workDir = await mkdtemp(join(tmpdir(), "sunnycloud-share-"));
  files = await writeShareFiles(workDir);
  owner = await browser.newContext({ acceptDownloads: true });
  await owner.grantPermissions(["clipboard-read", "clipboard-write"]);
});

test.afterAll(async () => {
  await owner?.close();
  await rm(workDir, { recursive: true, force: true });
});

test("a folder with a JPEG, an MP4, two PDFs, a movable PDF, an HTML file, an SVG file and a hostile name", async () => {
  // Arrange
  main = await signInAlice(owner);
  main.on("pageerror", (error) => problems.push(`owner pageerror: ${error.message}`));
  await main.getByRole("button", { name: "New folder" }).click();
  await main.getByLabel("Folder name").fill(FOLDER);
  await main.getByRole("button", { name: "Create" }).click();
  await main.getByRole("region", { name: "Files" }).getByRole("link", { name: FOLDER, exact: true }).click();
  await expect(main.getByText("This folder is empty")).toBeVisible();
  folderUrl = main.url();
  folderId = folderUrl.split("/").pop() as string;

  // Act
  await main.locator('input[type="file"]').setInputFiles(Object.values(files).map((file) => file.path));

  // Assert
  await expect(main.getByRole("complementary").getByRole("heading", { name: /Transfers/ })).toContainText("0 active", WAIT);
  for (const file of Object.values(files)) await expect(main.getByRole("region", { name: "Files" }).getByTitle(file.name, { exact: true }), `${file.name} appears`).toBeVisible(WAIT);
  // The panel of finished transfers floats over the rightmost column of the rows, where the row buttons are.
  await main.getByRole("button", { name: "Clear finished" }).click();
  await expect(main.getByRole("complementary")).toHaveCount(0);
  const listing = (await (await owner.request.get(`/api/v1/nodes/${folderId}/children?limit=200`)).json()) as { items: { node_id: string; name: string; media_type: string | null }[] };
  for (const file of Object.values(files)) {
    const node = listing.items.find((item) => item.name === file.name);
    expect(node, `${file.name} was uploaded`).toBeDefined();
    nodes.set(file.name, node as { node_id: string; media_type: string | null });
  }
  expect(nodes.get(files.mp4.name)?.media_type).toBe("video/mp4");
  expect(nodes.get(files.jpeg.name)?.media_type).toBe("image/jpeg");
});

test("AC-1: the link is shown once, the clipboard holds that URL, it opens the file, and the reopened dialog has no token", async ({ browser }) => {
  // Arrange
  const name = files.jpeg.name;
  const dialog = await openDialog(main, name);

  // Act
  await dialog.getByRole("button", { name: "Create link" }).click();
  const panel = created(dialog);
  await expect(panel).toBeVisible();
  await panel.getByRole("button", { name: "Copy link" }).click();

  // Assert (one URL, the clipboard)
  await expect(panel.getByText("Link copied")).toBeVisible();
  const url = (await panel.locator("#share-url").innerText()).trim();
  expect(url).toMatch(new RegExp(`^${origin}/s/sc_shr_[A-Za-z0-9_-]+$`));
  expect(((await dialog.innerText()).match(/\/s\/sc_shr_/g) ?? []).length, "exactly one URL in the dialog").toBe(1);
  expect(await main.evaluate(`navigator.clipboard.readText()`)).toBe(url);
  const token = url.split("/s/")[1] as string;

  // Assert (the URL opens the file, the download equals the source)
  const { context, page } = await visit(browser, url);
  await expect(page.getByRole("heading", { name })).toBeVisible();
  await expect(page.getByRole("img", { name })).toBeVisible();
  expect(await page.getByRole("img", { name }).evaluate((img) => (img as unknown as { naturalWidth: number }).naturalWidth)).toBeGreaterThan(0);
  const saved = await downloadHash(page, () => page.getByRole("button", { name: "Download" }).click());
  expect(saved).toEqual({ sha256: files.jpeg.sha256, name });
  await context.close();

  // Act (close, reopen)
  await dialog.getByRole("button", { name: "Done" }).click();
  const again = await openDialog(main, name);

  // Assert (created, expiry and password flag listed; the token nowhere)
  await expect(again.getByText(/^Expires /)).toHaveCount(1);
  await expect(again.getByText(/Created /)).toBeVisible();
  await expect(again.getByText("No password")).toBeVisible();
  expect(await main.content(), "the DOM holds no token").not.toContain(token);
  const listed = await (await owner.request.get(`/api/v1/shares?node_id=${nodes.get(name)?.node_id}`)).text();
  expect(listed, "the list answer holds no token").not.toContain(token);
  await closeDialog(again);
});

test("AC-3: a folder cannot be shared: the control is off with its reason, and the API answers TargetNotShareable", async () => {
  // Arrange
  const trips = `Trips ${Date.now()}`;
  const made = await owner.request.post("/api/v1/nodes", { headers: { origin }, data: { parent_id: folderId, name: trips } });
  const folder = (await made.json()) as { node_id: string };
  await main.reload();
  const button = shareButton(main, trips);
  await expect(button).toBeVisible();
  const before = await (await owner.request.get(`/api/v1/shares?node_id=${nodes.get(files.jpeg.name)?.node_id}`)).json();

  // Act
  await button.click({ force: true });
  const refused = await createShareApi(owner, { node_id: folder.node_id });

  // Assert
  await expect(button).toBeDisabled();
  await expect(button).toHaveAttribute("title", "Folder sharing is not available in this version");
  await expect(main.getByRole("dialog")).toHaveCount(0);
  expect(refused.status).toBe(422);
  expect(refused.body.code).toBe("target_not_shareable");
  expect(await (await owner.request.get(`/api/v1/shares?node_id=${nodes.get(files.jpeg.name)?.node_id}`)).json()).toEqual(before);
});

test("AC-2 (what the API allows): the dialog offers nothing above the server's longest lifetime and preselects its default", async () => {
  // Arrange
  const info = (await (await owner.request.get("/api/v1/server-info")).json()) as { share_default_ttl: number; share_max_ttl: number };
  const dialog = await openDialog(main, files.pdf.name);

  // Act
  const select = dialog.getByLabel("Expires", { exact: true });
  const offered = (await select.locator("option").evaluateAll((options) => options.map((option) => Number((option as unknown as { value: string }).value)))).sort((a, b) => a - b);

  // Assert
  expect(Math.max(...offered), "the longest choice is the server's maximum").toBe(info.share_max_ttl);
  expect(offered.every((seconds) => seconds > 0 && seconds <= info.share_max_ttl), `choices ${offered}`).toBe(true);
  await expect(select).toHaveValue(String(info.share_default_ttl));
  await expect(dialog.getByText("This file has no links yet.")).toBeVisible();
  await closeDialog(dialog);
});

test("AC-6: a JPEG shows inline, an MP4 plays inline, a PDF is a download with the same bytes", async ({ browser }) => {
  // Arrange
  const jpeg = await createShare(owner, { node_id: nodes.get(files.jpeg.name)?.node_id });
  const mp4 = await createShare(owner, { node_id: nodes.get(files.mp4.name)?.node_id });
  const pdf = await createShare(owner, { node_id: nodes.get(files.pdf.name)?.node_id });

  // Act and Assert: the JPEG
  const a = await visit(browser, jpeg.url);
  await expect(a.page.getByRole("img", { name: files.jpeg.name })).toBeVisible();
  await a.context.close();

  // Act and Assert: the MP4 plays
  const b = await visit(browser, mp4.url);
  const video = b.page.locator("video");
  await expect(video).toHaveAttribute("controls", "");
  await expect.poll(() => video.evaluate((v) => (v as unknown as { readyState: number }).readyState), WAIT).toBeGreaterThanOrEqual(1);
  await video.evaluate((v) => (v as unknown as { play(): Promise<void> }).play());
  await expect.poll(() => video.evaluate((v) => (v as unknown as { currentTime: number }).currentTime), WAIT).toBeGreaterThan(0);
  await b.context.close();

  // Act and Assert: the PDF
  const c = await visit(browser, pdf.url);
  await expect(c.page.getByRole("heading", { name: files.pdf.name })).toBeVisible();
  await expect(c.page.getByText("Preview isn't available for this file type. Download it to open it on your device.")).toBeVisible();
  expect(await c.page.locator("img, video, iframe, object, embed").count()).toBe(0);
  const saved = await downloadHash(c.page, () => c.page.getByRole("button", { name: "Download" }).click());
  expect(saved).toEqual({ sha256: files.pdf.sha256, name: files.pdf.name });
  await c.context.close();
});

test("AC-7: an expired, a revoked and an unknown link render the same page text with the same HTTP answer", async ({ browser }) => {
  // Arrange
  const nodeId = nodes.get(files.pdf.name)?.node_id;
  const expired = await createShare(owner, { node_id: nodeId, expires_in: 2 });
  const revoked = await createShare(owner, { node_id: nodeId });
  await revokeShare(owner, revoked.id);
  const unknown = `${origin}/s/sc_shr_${Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url")}`;
  await expect
    .poll(async () => (await owner.request.get(`/api/v1/public-shares/${expired.token}`)).status(), { timeout: 30_000 })
    .toBe(404);

  // Act
  const seen: { status: number; body: string; text: string; html: string }[] = [];
  for (const url of [expired.url, revoked.url, unknown]) {
    const { context, page } = await visitor(browser, problems);
    const answer = page.waitForResponse((response) => /\/api\/v1\/public-shares\/[^/]+$/.test(new URL(response.url()).pathname));
    await page.goto(url);
    const response = await answer;
    await expect(page.getByRole("heading", { name: UNAVAILABLE })).toBeVisible();
    seen.push({
      status: response.status(),
      body: (await response.text()).replace(/"request_id":"[^"]*"/, ""),
      text: await mainText(page),
      html: await page.getByRole("main").innerHTML(),
    });
    await context.close();
  }

  // Assert
  expect(seen[0].status).toBe(404);
  expect(seen[1]).toEqual(seen[0]);
  expect(seen[2]).toEqual(seen[0]);
  expect(seen[0].text).toContain(UNAVAILABLE);
});

test("AC-4: revoke inline in the row: one call, the row leaves, and within 5 s the link is not available", async ({ browser }) => {
  // Arrange
  const name = files.second.name;
  const link = await createShare(owner, { node_id: nodes.get(name)?.node_id });
  const open = await visit(browser, link.url);
  await expect(open.page.getByRole("heading", { name })).toBeVisible();
  await open.context.close();
  const dialog = await openDialog(main, name);
  const calls: string[] = [];
  main.on("request", (request) => request.method() === "DELETE" && calls.push(request.url()));

  // Act
  await dialog.getByRole("button", { name: /^Revoke link created/ }).click();
  await expect(dialog.getByText("People who have this link lose access.")).toBeVisible();
  await expect(main.getByRole("dialog")).toHaveCount(1);
  await dialog.getByRole("button", { name: "Revoke link", exact: true }).click();
  const started = Date.now();

  // Assert
  await expect(dialog.getByText("This file has no links yet.")).toBeVisible();
  expect(calls).toHaveLength(1);
  const after = await visit(browser, link.url);
  await expect(after.page.getByRole("heading", { name: UNAVAILABLE })).toBeVisible({ timeout: 5_000 });
  expect(Date.now() - started).toBeLessThan(5_000);
  await after.context.close();
  await closeDialog(dialog);
});

test("AC-5: a link follows the file when another client renames and moves it", async ({ browser }) => {
  // Arrange
  const name = files.movable.name;
  const nodeId = nodes.get(name)?.node_id;
  const link = await createShare(owner, { node_id: nodeId });
  const target = `Moved ${Date.now()}`;
  const targetNode = (await (await owner.request.post("/api/v1/nodes", { headers: { origin }, data: { parent_id: folderId, name: target } })).json()) as { node_id: string };
  const other = await browser.newContext({ storageState: await owner.storageState() });
  const renamedTo = "renamed-by-other-client.pdf";

  // Act
  expect((await other.request.patch(`/api/v1/nodes/${nodeId}`, { headers: { origin }, data: { name: renamedTo } })).status()).toBe(200);
  expect((await other.request.patch(`/api/v1/nodes/${nodeId}`, { headers: { origin }, data: { parent_id: targetNode.node_id } })).status()).toBe(200);
  await other.close();
  await main.goto(`/folders/${targetNode.node_id}`);
  const dialog = await openDialog(main, renamedTo);

  // Assert
  await expect(dialog.getByText(/Created /)).toBeVisible();
  await expect(dialog.getByRole("button", { name: /^Revoke link created/ })).toHaveCount(1);
  const visitorPage = await visit(browser, link.url);
  await expect(visitorPage.page.getByRole("heading", { name: renamedTo })).toBeVisible();
  await visitorPage.context.close();
  await closeDialog(dialog);
  await main.goto(folderUrl);
});

test("AC-8 and AC-11 (password), before unlock: nothing is revealed, a wrong password adds one line, the right one is taken, and no storage holds the password", async ({ browser }) => {
  // Arrange: the owner makes the link in the dialog, with a password.
  const dialog = await openDialog(main, SECRET_NAME);
  await dialog.getByRole("switch", { name: "Require a password" }).click();
  await dialog.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await dialog.getByRole("button", { name: "Create link" }).click();
  await expect(created(dialog)).toContainText("Password required");
  secretUrl = (await created(dialog).locator("#share-url").innerText()).trim();
  await closeDialog(dialog);
  const { context, page } = await visit(browser, secretUrl);
  await expect(page.getByRole("heading", { name: "This link is protected" })).toBeVisible();
  const before = await mainText(page);
  const html = await page.content();

  // Assert (before unlock)
  for (const text of ["secret-plan", "5.0 MB", "PDF", "application/pdf", "Available until"]) {
    expect(before, text).not.toContain(text);
    expect(html, text).not.toContain(text);
  }

  // Act (a wrong password)
  await page.getByLabel("Password", { exact: true }).fill("not-the-password");
  await page.getByRole("button", { name: "Unlock" }).click();

  // Assert
  await expect(page.getByText("That password isn't right. Check it and try again.")).toBeVisible();
  const squash = (text: string) => text.replace(/\s+/g, " ").trim();
  expect(squash((await mainText(page)).replace("That password isn't right. Check it and try again.", ""))).toBe(squash(before));

  // Act (the right one)
  await page.getByLabel("Password", { exact: true }).fill(PASSWORD);
  const [verified] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/password-verifications")), page.getByRole("button", { name: "Unlock" }).click()]);

  // Assert: the server took it (204 and its grant cookie); the password is in no storage, in the page or in the context
  expect(verified.status()).toBe(204);
  const stored = await dumpStorage(page);
  expect(stored, "the share password is in no browser storage").not.toContain(PASSWORD);
  expect(stored).not.toContain("not-the-password");
  expect(JSON.stringify(await context.storageState())).not.toContain(PASSWORD);
  await context.close();
});

test("AC-8 (after unlock): the right password shows name, size and type, and the download equals the file", async ({ browser }) => {
  const { context, page } = await visit(browser, secretUrl);
  await page.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await page.getByRole("button", { name: "Unlock" }).click();

  await expect(page.getByRole("heading", { name: SECRET_NAME })).toBeVisible();
  await expect(page.getByText(`${(SECRET_BYTES / 1_000_000).toFixed(1)} MB · PDF document · Available until`)).toBeVisible();
  const saved = await downloadHash(page, () => page.getByRole("button", { name: "Download" }).click());
  expect(saved).toEqual({ sha256: files.secret.sha256, name: SECRET_NAME });
  await context.close();
});

test("AC-9: the 11th wrong password gets 429, the page counts down from Retry-After and Unlock stays off", async ({ browser }) => {
  // Arrange
  const link = await createShare(owner, { node_id: nodes.get(files.second.name)?.node_id, password: PASSWORD });
  const { context, page } = await visit(browser, link.url);
  await page.getByLabel("Password", { exact: true }).fill("wrongpass1");
  const verify = (response: import("@playwright/test").Response) => response.url().endsWith("/password-verifications");

  // Act
  let last: import("@playwright/test").Response | undefined;
  for (let attempt = 1; attempt <= 11; attempt += 1) {
    [last] = await Promise.all([page.waitForResponse(verify), page.getByRole("button", { name: "Unlock" }).click()]);
    if (attempt < 11) expect(last.status(), `attempt ${attempt}`).toBe(401);
  }

  // Assert
  expect((last as import("@playwright/test").Response).status()).toBe(429);
  const retryAfter = Number((last as import("@playwright/test").Response).headers()["retry-after"]);
  expect(retryAfter).toBeGreaterThan(0);
  const message = page.getByText(/Too many attempts\./);
  await expect(message).toBeVisible();
  const shown = Number(/in (\d+) second/.exec(await message.locator("..").innerText())?.[1]);
  expect(Math.abs(shown - retryAfter), `shown ${shown}, Retry-After ${retryAfter}`).toBeLessThanOrEqual(1);
  const unlock = page.getByRole("button", { name: "Unlock" });
  await expect(unlock).toBeDisabled();
  await expect(unlock).toHaveAttribute("title", "Wait until the countdown ends");
  await expect(page.getByText("Unlock is off until the countdown ends.")).toBeVisible();
  await context.close();
});

test("AC-10: after load the address, the history, the console and later requests hold no token", async ({ browser }) => {
  // Arrange
  const link = await createShare(owner, { node_id: nodes.get(files.jpeg.name)?.node_id });
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  const lines: string[] = [];
  const referers: { url: string; referer: string | undefined }[] = [];
  page.on("console", (message) => lines.push(message.text()));
  page.on("request", (request) => void request.allHeaders().then((headers) => referers.push({ url: request.url(), referer: headers["referer"] })));
  await page.goto("about:blank");

  // Act
  await page.goto(link.url);
  await expect(page.getByRole("img", { name: files.jpeg.name })).toBeVisible();

  // Assert (the address and the history)
  await expect.poll(() => page.url()).toBe(`${origin}/s`);
  await page.goBack();
  expect(page.url()).toBe("about:blank");
  await page.goForward();
  expect(page.url()).not.toContain(link.token);

  // Assert (the console, the referrers of the page's own API requests, and an outgoing link)
  expect(lines.filter((line) => line.includes(link.token))).toEqual([]);
  const api = referers.filter((entry) => entry.url.includes("/api/v1/"));
  expect(api.length).toBeGreaterThan(0);
  expect(api.filter((entry) => entry.referer?.includes(link.token))).toEqual([]);
  expect(api.filter((entry) => entry.referer !== undefined)).toEqual([]);
  await page.route("http://outgoing.test/**", (route) => route.fulfill({ contentType: "text/html", body: "<p>elsewhere</p>" }));
  const outgoing = page.waitForRequest("http://outgoing.test/**");
  await page.evaluate(`(() => { const a = document.createElement("a"); a.href = "http://outgoing.test/x"; document.body.append(a); a.click(); })()`);
  expect((await (await outgoing).allHeaders())["referer"]).toBeUndefined();
  await context.close();
});

test("AC-11 (Chromium): hostile files are a download only, run no script, and a hostile name is text", async ({ browser }) => {
  // Arrange
  const html = await createShare(owner, { node_id: nodes.get(files.html.name)?.node_id });
  const svg = await createShare(owner, { node_id: nodes.get(files.svg.name)?.node_id });
  const hostile = await createShare(owner, { node_id: nodes.get(HOSTILE_NAME)?.node_id });

  // Act and Assert: HTML and SVG
  for (const [link, file] of [[html, files.html], [svg, files.svg]] as [Created, ShareFile][]) {
    const { context, page } = await visit(browser, link.url);
    await expect(page.getByRole("heading", { name: file.name })).toBeVisible();
    await expect(page.getByText("Preview isn't available for this file type.")).toBeVisible();
    expect(await page.locator("img, video, iframe, object, embed").count(), file.name).toBe(0);
    const content = await context.request.get(`/api/v1/public-shares/${link.token}/content`);
    expect(content.headers()["content-disposition"], file.name).toMatch(/^attachment/);
    expect(content.headers()["x-content-type-options"]).toBe("nosniff");
    const saved = await downloadHash(page, () => page.getByRole("button", { name: "Download" }).click());
    expect(saved, file.name).toEqual({ sha256: file.sha256, name: file.name });
    expect(await page.evaluate(() => (globalThis as unknown as { __executed?: number }).__executed), "no script ran").toBeUndefined();
    await context.close();
  }

  // Act and Assert: the hostile name
  const { context, page } = await visit(browser, hostile.url);
  await expect(page.getByRole("heading", { name: HOSTILE_NAME, exact: true })).toBeVisible();
  expect(await page.locator("img[src='x']").count()).toBe(0);
  expect(await page.evaluate(() => (globalThis as unknown as { __xss?: number }).__xss)).toBeUndefined();
  await context.close();
});

test("the run produced no page error or CSP violation", () => {
  expect(problems).toEqual([]);
});
