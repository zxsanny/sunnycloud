// Device approval page (AZ-3944) in a real Chromium against the real stack. Runs after smoke.spec.ts (alice).
// The desktop app is played by calls to the API's public port with a forwarded address, which the stack trusts from
// 127.0.0.1: the same address as the browser's gives a match, another one a mismatch. Not here: AC-5 (the 10-minute
// expiry needs a test clock the stack has not got) and AC-7 (six wrong codes would block the page for the other specs).
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "../support/env.ts";
import { signInAlice } from "../support/session.ts";

const LAN = readAcceptanceEnv().ACC_HOST === "localhost" ? "127.0.0.1" : (readAcceptanceEnv().ACC_HOST as string);
const ELSEWHERE = "203.0.113.7";
const api = `http://127.0.0.1:${readAcceptanceEnv().ACC_API_PORT}/api/v1`;

interface Grant {
  deviceCode: string;
  userCode: string;
}

/** The desktop app starts a sign-in from `from`: a person-facing code and the code only the app holds. */
async function startGrant(deviceName: string, from: string): Promise<Grant> {
  const response = await fetch(`${api}/device-grants`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-forwarded-for": from },
    body: JSON.stringify({ device_name: deviceName, platform: "linux" }),
  });
  expect(response.status).toBe(200);
  const body = (await response.json()) as { device_code: string; user_code: string };
  return { deviceCode: body.device_code, userCode: body.user_code };
}

/** What the app's poll gets: a token once approved, an error while pending or after a denial. */
async function poll(grant: Grant): Promise<{ ok: boolean; status: number }> {
  const response = await fetch(`${api}/device-tokens`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ device_code: grant.deviceCode }),
  });
  return { ok: response.ok, status: response.status };
}

async function enterCode(page: Page, code: string): Promise<void> {
  await page.goto("/device");
  await page.getByLabel("Code").fill(code);
  await page.getByRole("button", { name: "Continue" }).click();
}

let context: BrowserContext;
let page: Page;

test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browser }) => {
  context = await browser.newContext();
  page = await signInAlice(context);
});

test.afterAll(async () => {
  await context?.close();
});

test("a code from the same place shows both locations, no warning, and Approve connects the device", async () => {
  // Arrange
  const name = `Match laptop ${Date.now()}`;
  const grant = await startGrant(name, LAN);

  // Act
  await enterCode(page, grant.userCode);

  // Assert: the review shows the device and both addresses, and no mismatch notice
  await expect(page.getByRole("heading", { name: "Approve this sign-in?" })).toBeVisible();
  await expect(page.getByTestId("device-name")).toHaveText(name);
  await expect(page.getByText("Same location")).toBeVisible();
  await expect(page.getByText("This request comes from a different place than you.")).toHaveCount(0);
  await expect(page.getByText(LAN, { exact: true }).first()).toBeVisible();
  expect((await poll(grant)).ok, "nothing is granted before the decision").toBe(false);

  // Act
  await page.getByRole("button", { name: "Approve", exact: true }).click();

  // Assert
  await expect(page.getByRole("heading", { name: `${name} is connected` })).toBeVisible();
  expect((await poll(grant)).ok, "the app's poll now gets its token").toBe(true);
  await page.goto("/account");
  await expect(page.getByText(name)).toBeVisible();
});

test("a code from another place shows the warning, Deny is the main button and Approve needs the tick", async () => {
  // Arrange
  const name = `Elsewhere laptop ${Date.now()}`;
  const grant = await startGrant(name, ELSEWHERE);

  // Act
  await enterCode(page, grant.userCode);

  // Assert
  await expect(page.getByText("This request comes from a different place than you.")).toBeVisible();
  await expect(page.getByText("Different location")).toBeVisible();
  await expect(page.getByText(ELSEWHERE, { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Deny request" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toBeDisabled();

  // Act: ticking the box enables Approve, unticking disables it again
  await page.getByLabel("I started this sign-in on that device").check();
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toBeEnabled();
  await page.getByLabel("I started this sign-in on that device").uncheck();
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Deny request" }).click();

  // Assert
  await expect(page.getByRole("heading", { name: "Request denied" })).toBeVisible();
  expect((await poll(grant)).ok, "a denied request never yields a token").toBe(false);
  await page.goto("/account");
  await expect(page.getByText(name)).toHaveCount(0);
});

test("a mismatch can still be approved after the tick", async () => {
  // Arrange
  const name = `Ticked laptop ${Date.now()}`;
  const grant = await startGrant(name, ELSEWHERE);
  await enterCode(page, grant.userCode);
  await expect(page.getByText("Different location")).toBeVisible();

  // Act
  await page.getByLabel("I started this sign-in on that device").check();
  await page.getByRole("button", { name: "Approve", exact: true }).click();

  // Assert
  await expect(page.getByRole("heading", { name: `${name} is connected` })).toBeVisible();
  expect((await poll(grant)).ok).toBe(true);
});

test("a wrong code and a used code both end on the same expired-or-invalid page", async () => {
  // Arrange
  const used = await startGrant(`Used laptop ${Date.now()}`, LAN);
  await enterCode(page, used.userCode);
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(page.getByRole("heading", { name: /is connected/ })).toBeVisible();

  // Act and assert: the used code
  await enterCode(page, used.userCode);
  await expect(page.getByRole("heading", { name: "This code has expired or isn't valid" })).toBeVisible();

  // Act and assert: a code nobody has
  await enterCode(page, "ZZZZZZZZ");
  await expect(page.getByRole("heading", { name: "This code has expired or isn't valid" })).toBeVisible();
});

test("without a session the page sends the visitor to sign-in and nothing is decided", async ({ browser }) => {
  // Arrange
  const anonymous = await browser.newContext();
  const grant = await startGrant(`Signed-out laptop ${Date.now()}`, LAN);

  // Act
  const visitor = await anonymous.newPage();
  await visitor.goto("/device");

  // Assert
  await expect(visitor).toHaveURL(/\/login/);
  expect((await poll(grant)).ok).toBe(false);
  await anonymous.close();
});
