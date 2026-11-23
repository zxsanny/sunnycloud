// Browser session renewal (roadmap section 7, D6), in a real Chromium against the real stack. Runs after smoke.spec.ts (alice).
// The 15-minute session cookie cannot be waited out, so the test deletes it: that is the state of the browser when it
// has expired. The refresh cookie is kept or deleted by the test; nothing else about the session is touched.
import { expect, test } from "@playwright/test";
import { filesPage, signInAlice } from "../support/session.ts";

const SESSION = "__Host-session";
const REFRESH = "__Secure-refresh";
const RENEWAL_PATH = "/api/v1/auth/session-renewals";

test.describe.configure({ mode: "serial" });

test("a sign-in sets a refresh cookie that only the renewal route receives", async ({ browser }) => {
  // Arrange
  const context = await browser.newContext();

  // Act
  await signInAlice(context);
  const cookies = await context.cookies();

  // Assert
  const refresh = cookies.find((cookie) => cookie.name === REFRESH);
  expect(refresh, "the sign-in set a refresh cookie").toBeDefined();
  expect({ path: refresh?.path, httpOnly: refresh?.httpOnly, secure: refresh?.secure, sameSite: refresh?.sameSite }).toEqual({
    path: RENEWAL_PATH,
    httpOnly: true,
    secure: true,
    sameSite: "Strict",
  });
  await context.close();
});

test("with the session cookie gone, a reload keeps alice signed in on a rotated refresh cookie", async ({ browser }) => {
  // Arrange
  const context = await browser.newContext();
  const page = await signInAlice(context);
  const before = (await context.cookies()).find((cookie) => cookie.name === REFRESH);
  expect(before, "the sign-in set a refresh cookie").toBeDefined();
  await context.clearCookies({ name: SESSION });
  expect((await context.cookies()).some((cookie) => cookie.name === SESSION)).toBe(false);

  // Act
  await page.reload();

  // Assert
  await expect(filesPage(page)).toBeVisible();
  expect(new URL(page.url()).pathname, "the app did not go back to the sign-in page").not.toBe("/login");
  const after = await context.cookies();
  expect(after.some((cookie) => cookie.name === SESSION), "a new session cookie was set").toBe(true);
  const rotated = after.find((cookie) => cookie.name === REFRESH);
  expect(rotated?.value, "the refresh cookie was rotated").not.toBe(before?.value);
  expect(rotated?.path).toBe(RENEWAL_PATH);
  expect((await page.request.get("/api/v1/accounts/me")).status()).toBe(200);
  await context.close();
});

test("with both cookies gone, a reload shows the sign-in page", async ({ browser }) => {
  // Arrange
  const context = await browser.newContext();
  const page = await signInAlice(context);
  await context.clearCookies({ name: SESSION });
  await context.clearCookies({ name: REFRESH });

  // Act
  await page.reload();

  // Assert
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByRole("button", { name: /OpenID Connect/ })).toBeVisible();
  await context.close();
});
