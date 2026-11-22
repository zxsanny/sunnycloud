// Resume and edit detection of large uploads (AZ-3941): records in IndexedDB, a closed tab, a changed file, a
// tampered record, an expired session, an edit before commit, two accounts in one browser, blocked storage.
// The 5 GiB and 1 GiB runs of the task spec do not fit this host: files here are 32 to 64 MiB (16 to 32 blocks of
// 2 MiB, four lanes, so the "at most 10 MiB sent again" bound stays meaningful), and 300 MiB for the blocked-storage
// upload. The big runs belong to AZ-3952. Runs after smoke.spec.ts (alice exists). Every wait is a bounded poll.
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";
import {
  BLOCK,
  dumpRecords,
  growByOneMib,
  holdBlocksAfter,
  setUploadSessionTtl,
  tamperBlocklist,
  touch,
  waitWithHeartbeat,
  watchUploads,
  writeRandomFile,
  type BlockTraffic,
} from "../support/resume.ts";
import { filesPage, inviteToken, mockIdpUser, signInAlice } from "../support/session.ts";

// A run-specific name, so the spec can run again on the same stack.
const FOLDER = `Resume ${Date.now()}`;
const MIB = 1024 * 1024;
const WAIT = { timeout: 120_000 };
const CHANGED = /changed since last time/;

test.describe.configure({ mode: "serial" });

let context: BrowserContext;
let main: Page;
let workDir: string;
let folderUrl: string;
const problems: string[] = [];

const region = (page: Page) => page.getByRole("region", { name: "Files" });
const rowOf = (page: Page, name: string): Locator => region(page).locator("li", { has: page.getByTitle(name, { exact: true }) });
const banner = (page: Page) => page.getByText(/uploads? stopped when the tab closed\./);
const transfers = (page: Page) => page.getByRole("heading", { name: /Transfers/ });
const panelRow = (page: Page, name: string) => page.getByRole("complementary").locator("li", { hasText: name });

function watchProblems(page: Page): Page {
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console.error: ${message.text()}`);
  });
  return page;
}

/** A new tab of the same browser profile on the test folder, as after reopening the app. */
async function reopen(): Promise<Page> {
  const page = watchProblems(await context.newPage());
  await page.goto(folderUrl);
  await expect(filesPage(page)).toBeVisible();
  return page;
}

/** Starts an upload, lets `after` block PUTs through, holds the rest, and waits until the record is stored. */
async function uploadAndHold(page: Page, path: string, after: number): Promise<BlockTraffic> {
  const traffic = await holdBlocksAfter(page, after);
  await page.locator('input[type="file"]').setInputFiles(path);
  await expect.poll(() => traffic.acked.length, { timeout: 60_000, message: "the first blocks reach the server" }).toBe(after);
  await expect.poll(async () => (await dumpRecords(page)).length, { timeout: 30_000, message: "the resume record is stored" }).toBeGreaterThan(0);
  return traffic;
}

/** Closes the tab: its page, Worker and in-flight requests are gone, the profile's IndexedDB stays. */
async function closeTab(page: Page): Promise<void> {
  await page.close();
}

async function selectWithBanner(page: Page, paths: string[]): Promise<void> {
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.getByRole("button", { name: /^Select files?$/ }).click()]);
  await chooser.setFiles(paths);
}

async function selectOnRow(page: Page, name: string, path: string): Promise<void> {
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.getByRole("button", { name: `Select file for ${name}` }).click()]);
  await chooser.setFiles(path);
}

async function expectDone(page: Page, name: string): Promise<void> {
  await expect(rowOf(page, name)).toBeVisible(WAIT);
  await expect(transfers(page)).toContainText("0 active", WAIT);
}

async function downloadHash(page: Page, name: string): Promise<string> {
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: `Download ${name}` }).click()]);
  return createHash("sha256").update(await readFile(await download.path())).digest("hex");
}

/** Revisions the account holds for the node named `name` in the folder: one means no second (mixed) revision. */
async function revisionsOf(page: Page, name: string): Promise<number> {
  const children = (await (await page.request.get(`/api/v1/nodes/${folderId()}/children?limit=200`)).json()) as { items: { node_id: string; name: string }[] };
  const nodes = children.items.filter((item) => item.name === name);
  expect(nodes, `exactly one node named ${name}`).toHaveLength(1);
  const revisions = (await (await page.request.get("/api/v1/revisions?limit=1000")).json()) as { items: { node_id: string }[] };
  return revisions.items.filter((item) => item.node_id === nodes[0].node_id).length;
}

/** Signs out and waits until the server no longer accepts the old session (the revocation takes a moment to show). */
async function signOut(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect.poll(async () => (await page.request.get("/api/v1/accounts/me")).status(), { timeout: 30_000, message: "the session is revoked" }).toBe(401);
  await page.goto("/login");
}

const folderId = () => folderUrl.split("/").pop() ?? "";

test.beforeAll(async ({ browser }) => {
  workDir = await mkdtemp(join(tmpdir(), "sunnycloud-resume-"));
  context = await browser.newContext({ acceptDownloads: true });
  main = watchProblems(await signInAlice(context));
});

test.afterAll(async () => {
  await context?.close();
  await rm(workDir, { recursive: true, force: true });
  await setUploadSessionTtl(null);
  await mockIdpUser("alice");
});

test("a folder for the resume tests", async () => {
  // Act
  await main.getByRole("button", { name: "Photo grid" }).click();
  await main.getByRole("button", { name: "New folder" }).click();
  await main.getByLabel("Folder name").fill(FOLDER);
  await main.getByRole("button", { name: "Create" }).click();
  await region(main).getByRole("link", { name: FOLDER, exact: true }).click();

  // Assert
  await expect(main.getByText("This folder is empty")).toBeVisible();
  folderUrl = main.url();
  await main.close();
});

test("AC-1 and AC-7: a tab closed at half resumes with only the missing blocks, the same session and the same bytes", async () => {
  // Arrange: 64 MiB is 32 blocks; 16 reach the server, then the tab closes with four more on the wire.
  const file = await writeRandomFile(workDir, "resume-half.bin", 64);
  const first = watchProblems(await context.newPage());
  await first.goto(folderUrl);
  const before = await uploadAndHold(first, file.path, 16);

  // Assert: the record holds the contract's fields and no token, password or cookie value (AC-7).
  const [record] = await dumpRecords(first);
  expect(Object.keys(record).sort()).toEqual(["accountId", "blocklist", "clientKey", "fileName", "lastModified", "parentId", "sessionId", "size"]);
  expect(record).toMatchObject({ fileName: "resume-half.bin", size: 64 * MIB, parentId: folderId() });
  expect(record.blocklist).toHaveLength(32);
  const dump = JSON.stringify(await dumpRecords(first));
  for (const cookie of await context.cookies()) expect(dump, `the record holds no value of cookie ${cookie.name}`).not.toContain(cookie.value);
  expect(dump).not.toMatch(/token|password|cookie/i);

  // Act: close the tab, open the app again, and select the same file.
  await closeTab(first);
  const second = await reopen();
  const traffic = watchUploads(second);
  await expect(banner(second)).toHaveText("1 upload stopped when the tab closed.");
  await expect(panelRow(second, "resume-half.bin")).toContainText("Stopped when the tab closed. Select the same file to continue.");
  await selectWithBanner(second, [file.path]);
  await expectDone(second, "resume-half.bin");

  // Assert
  expect(traffic.sessionStarts, "the session of the record is continued, no new one").toBe(0);
  expect(traffic.blockPuts).toHaveLength(32 - 16);
  expect(traffic.blockPuts.filter((i) => before.acked.includes(i)), "no block the server already held").toEqual([]);
  expect([...traffic.blockPuts, ...before.acked].sort((a, b) => a - b)).toEqual(Array.from({ length: 32 }, (_, i) => i));
  const resentBlocks = traffic.blockPuts.filter((i) => before.attempted.includes(i));
  expect(resentBlocks.length * BLOCK, "bytes sent again (AC-3, AC-50)").toBeLessThanOrEqual(10 * MIB);
  expect(traffic.commits).toBe(1);
  expect(await downloadHash(second, "resume-half.bin")).toBe(file.sha256);
  expect(await dumpRecords(second), "the record is removed at commit").toEqual([]);
  await closeTab(second);
});

for (const change of ["modified time", "size"] as const) {
  test(`AC-2: the same name with another ${change} restarts from the start with the changed-since-last-time message`, async () => {
    // Arrange
    const name = `changed-${change === "size" ? "size" : "time"}.bin`;
    const file = await writeRandomFile(workDir, name, 32);
    const first = watchProblems(await context.newPage());
    await first.goto(folderUrl);
    await uploadAndHold(first, file.path, 6);
    const [stale] = await dumpRecords(first);
    await closeTab(first);
    const second = await reopen();
    const traffic = watchUploads(second);
    await expect(banner(second)).toBeVisible();
    const sha256 = change === "size" ? await growByOneMib(file.path) : (await touch(file.path), file.sha256);

    // Act
    await selectOnRow(second, name, file.path);

    // Assert
    await expect(panelRow(second, name)).toContainText(CHANGED, WAIT);
    await expectDone(second, name);
    expect(traffic.aborted, "the old session is ended").toContain(stale.sessionId);
    expect(traffic.sessionStarts).toBe(1);
    expect(traffic.blockPuts.length, "every block again").toBe(change === "size" ? 17 : 16);
    expect(await downloadHash(second, name)).toBe(sha256);
    expect(await dumpRecords(second)).toEqual([]);
    await closeTab(second);
  });
}

test("AC-3: a tampered blocklist cannot corrupt the file: the server refuses the hash, the job restarts, the content equals the file", async () => {
  // Arrange: a stopped upload whose stored hashes are wrong for one block the server lacks and one it holds.
  const file = await writeRandomFile(workDir, "tampered.bin", 32);
  const first = watchProblems(await context.newPage());
  await first.goto(folderUrl);
  const before = await uploadAndHold(first, file.path, 8);
  await closeTab(first);
  const needed = Array.from({ length: 32 }, (_, i) => i).find((i) => !before.acked.includes(i)) ?? 0;
  const held = before.acked[0];
  const wrong = "0".repeat(63) + "1";
  const plain = await context.newPage();
  await plain.goto("/__acceptance/health");
  await tamperBlocklist(plain, { [needed]: wrong, [held]: wrong });
  await plain.close();
  const second = await reopen();
  const traffic = watchUploads(second);

  // Act
  await selectWithBanner(second, [file.path]);
  await expectDone(second, "tampered.bin");

  // Assert
  expect(traffic.blockStatuses, "the server refused a wrong block hash").toContain(422);
  expect(traffic.sessionStarts, "the job restarted with a fresh hash pass").toBeGreaterThanOrEqual(1);
  expect(await downloadHash(second, "tampered.bin")).toBe(file.sha256);
  expect(await revisionsOf(second, "tampered.bin")).toBe(1);
  await closeTab(second);
});

test("AC-6: two uploads stopped by closing the tab are listed again for their owner, and another account in the same browser sees none", async () => {
  // Arrange: two stopped uploads from two tabs.
  const one = await writeRandomFile(workDir, "stopped-one.bin", 32);
  const two = await writeRandomFile(workDir, "stopped-two.bin", 32);
  for (const file of [one, two]) {
    const tab = watchProblems(await context.newPage());
    await tab.goto(folderUrl);
    await uploadAndHold(tab, file.path, 4);
    await closeTab(tab);
  }
  const owner = await reopen();
  await expect(banner(owner)).toHaveText("2 uploads stopped when the tab closed.");
  for (const name of ["stopped-one.bin", "stopped-two.bin"]) {
    await expect(owner.getByRole("button", { name: `Select file for ${name}` })).toBeVisible();
  }
  const me = (await (await owner.request.get("/api/v1/accounts/me")).json()) as { id: string };
  const token = await inviteToken(me.id);

  // Act: bob signs in, in the same browser profile.
  await signOut(owner);
  await mockIdpUser("bob");
  // Bob exists from an earlier run on this stack; a new account needs the invite.
  await owner.goto("/login");
  await owner.getByRole("button", { name: /OpenID Connect/ }).click();
  const notInvited = owner.getByText("This account is not invited.");
  await expect(notInvited.or(filesPage(owner))).toBeVisible();
  if (await notInvited.isVisible()) {
    await owner.goto(`/invite#${token}`);
    await expect(owner.getByText("You have an invite.")).toBeVisible();
    await owner.getByRole("button", { name: /OpenID Connect/ }).click();
  }
  await expect(filesPage(owner)).toBeVisible();
  await expect(owner.getByText("This folder is empty")).toBeVisible();
  await expect.poll(async () => (await dumpRecords(owner)).length, { message: "alice's records are still in this profile" }).toBe(2);
  await owner.waitForTimeout(1500);

  // Assert: bob sees neither the banner nor a stopped row.
  await expect(banner(owner)).toHaveCount(0);
  await expect(owner.getByRole("button", { name: /^Select file/ })).toHaveCount(0);

  // Act: alice returns and continues both with one selection.
  await signOut(owner);
  await mockIdpUser("alice");
  await owner.getByRole("button", { name: /OpenID Connect/ }).click();
  await expect(filesPage(owner)).toBeVisible();
  await owner.goto(folderUrl);
  await expect(banner(owner)).toHaveText("2 uploads stopped when the tab closed.");
  await selectWithBanner(owner, [one.path, two.path]);

  // Assert
  await expectDone(owner, "stopped-one.bin");
  await expectDone(owner, "stopped-two.bin");
  expect(await downloadHash(owner, "stopped-one.bin")).toBe(one.sha256);
  expect(await downloadHash(owner, "stopped-two.bin")).toBe(two.sha256);
  expect(await dumpRecords(owner)).toEqual([]);
  await closeTab(owner);
});

for (const edit of ["content", "modified time"] as const) {
  test(`AC-5: a file whose ${edit} changes at 60 % stops, restarts from the new file when selected again, and no mixed revision is published`, async () => {
    // Arrange: 64 MiB, 19 of 32 blocks (59 %) go out, the rest is held while the file on disk changes.
    const name = edit === "content" ? "edited-content.bin" : "edited-time.bin";
    const file = await writeRandomFile(workDir, name, 64);
    const page = watchProblems(await context.newPage());
    await page.goto(folderUrl);
    const traffic = await uploadAndHold(page, file.path, 19);
    const sha256 = edit === "content" ? (await writeRandomFile(workDir, name, 64)).sha256 : (await touch(file.path), file.sha256);

    // Act: the held blocks go on; the next read of the file is refused by the browser.
    traffic.release();
    await expect(panelRow(page, name)).toContainText("Stopped:", WAIT);
    await expect(banner(page)).toBeVisible();
    expect(await revisionsOfOrNone(page, name), "nothing is committed from a changed file").toBe(0);
    await selectOnRow(page, name, file.path);

    // Assert
    await expect(panelRow(page, name)).toContainText(CHANGED, WAIT);
    await expectDone(page, name);
    expect(await downloadHash(page, name)).toBe(sha256);
    expect(await revisionsOf(page, name), "one revision, the new file").toBe(1);
    expect(await dumpRecords(page)).toEqual([]);
    await closeTab(page);
  });
}

/** Like `revisionsOf`, but a folder without that name has none (0). */
async function revisionsOfOrNone(page: Page, name: string): Promise<number> {
  const children = (await (await page.request.get(`/api/v1/nodes/${folderId()}/children?limit=200`)).json()) as { items: { name: string }[] };
  return children.items.some((item) => item.name === name) ? revisionsOf(page, name) : 0;
}

test("a page that is hidden shows its running upload as stopped, keeps the record, and continues when the file is selected", async () => {
  // Arrange
  const file = await writeRandomFile(workDir, "hidden.bin", 32);
  const page = watchProblems(await context.newPage());
  await page.goto(folderUrl);
  const traffic = await uploadAndHold(page, file.path, 4);

  // Act: what a browser sends a page it puts away.
  await page.evaluate(`window.dispatchEvent(new Event("pagehide"))`);

  // Assert
  await expect(banner(page)).toHaveText("1 upload stopped when the tab closed.");
  expect(await dumpRecords(page)).toHaveLength(1);
  traffic.release();
  await selectWithBanner(page, [file.path]);
  await expectDone(page, "hidden.bin");
  expect(await downloadHash(page, "hidden.bin")).toBe(file.sha256);
  await closeTab(page);
});

test("AC-4: an upload whose session expired says so, loses its record, and a retry uploads the file again", async () => {
  // Arrange: sessions expire one minute after their last checkpoint on this stack.
  const file = await writeRandomFile(workDir, "expired.bin", 32);
  try {
    await setUploadSessionTtl("1m");
    const first = watchProblems(await context.newPage());
    await first.goto(folderUrl);
    await uploadAndHold(first, file.path, 8);
    await closeTab(first);
    await waitWithHeartbeat(75, "waiting for the upload session to expire (TTL 1 m)");
    const second = await reopen();
    const traffic = watchUploads(second);
    await expect(banner(second)).toBeVisible();

    // Act
    await selectWithBanner(second, [file.path]);

    // Assert
    await expect(panelRow(second, "expired.bin")).toContainText("This upload expired on the server", WAIT);
    expect(traffic.blockPuts, "no block is sent for a session that is gone").toEqual([]);
    expect(await dumpRecords(second)).toEqual([]);
    await expect(banner(second)).toHaveCount(0);
    await second.getByRole("button", { name: "Retry expired.bin" }).click();
    await expectDone(second, "expired.bin");
    expect(await downloadHash(second, "expired.bin")).toBe(file.sha256);
    await closeTab(second);
  } finally {
    await setUploadSessionTtl(null);
  }
});

test("AC-7: with IndexedDB blocked a 300 MiB upload still completes, one notice says no resume is offered, and nothing is listed after a reload", async ({ browser }) => {
  // Arrange: storage that throws when the app touches it, as a browser with blocked site data does.
  const file = await writeRandomFile(workDir, "blocked-storage.bin", 300);
  const blocked = await browser.newContext({ acceptDownloads: true });
  await blocked.addInitScript(`Object.defineProperty(window, "indexedDB", {
    configurable: true,
    get() {
      throw new DOMException("The operation is insecure.", "SecurityError");
    },
  })`);
  try {
    const page = watchProblems(await signInAlice(blocked));
    await page.goto(folderUrl);
    await page.getByRole("button", { name: "Photo grid" }).click();

    // Act
    await page.locator('input[type="file"]').setInputFiles(file.path);
    await expectDone(page, "blocked-storage.bin");

    // Assert
    await expect(page.getByText("Interrupted uploads cannot be resumed in this browser.")).toHaveCount(1);
    expect(await downloadHash(page, "blocked-storage.bin")).toBe(file.sha256);
    await page.reload();
    await expect(rowOf(page, "blocked-storage.bin")).toBeVisible();
    await expect(banner(page)).toHaveCount(0);
  } finally {
    await blocked.close();
  }
});

test("the run produced no page error or console error", async () => {
  // Assert: held, aborted and expired requests are expected network noise, never a script error.
  const unexpected = problems.filter((line) => !/Failed to load resource|net::ERR|ERR_ABORTED/.test(line));
  expect(unexpected).toEqual([]);
});
