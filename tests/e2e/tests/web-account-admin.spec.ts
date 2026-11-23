// Account page (AZ-3937) and admin page (AZ-3946) in a real Chromium against the real stack. Runs after smoke.spec.ts
// (alice is the first account, so she is the admin). Not here: account deletion (it ends alice's account for every spec
// after this one) and provider linking (the mock IdP offers one provider).
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { filesPage, inviteToken, mockIdpUser, signInAlice } from "../support/session.ts";

test.describe.configure({ mode: "serial" });

let first: BrowserContext;
let main: Page;

test.beforeAll(async ({ browser }) => {
  first = await browser.newContext();
  main = await signInAlice(first);
});

test.afterAll(async () => {
  await first?.close();
});

test("the account page shows the profile, the storage and this browser's session", async () => {
  // Act
  await main.goto("/account");

  // Assert
  await expect(main.getByRole("heading", { name: "Account", level: 1 })).toBeVisible();
  await expect(main.getByTestId("profile-name")).toContainText("alice", { ignoreCase: true });
  await expect(main.getByText("Admin", { exact: true })).toBeVisible();
  await expect(main.getByRole("heading", { name: "Storage" })).toBeVisible();
  await expect(main.getByText(/of .* used/)).toBeVisible();
  await expect(main.getByRole("list", { name: "Browser sessions" }).getByTestId("current-chip")).toHaveCount(1);
});

test("a second browser signed in as alice is listed and Revoke signs it out", async ({ browser }) => {
  // Arrange
  const other = await browser.newContext();
  const otherPage = await signInAlice(other);
  await main.goto("/account");
  const sessions = main.getByRole("list", { name: "Browser sessions" });
  const revokeButtons = sessions.getByRole("button", { name: /^Revoke / });
  await expect(revokeButtons.first()).toBeVisible();
  const before = await sessions.getByRole("listitem").count();

  // Act: revoke the first row that is not this browser (the page offers no Revoke on its own row)
  await revokeButtons.first().click();
  await main.getByRole("button", { name: "Revoke browser", exact: true }).click();

  // Assert
  await expect(sessions.getByRole("listitem")).toHaveCount(before - 1);
  await expect.poll(async () => (await otherPage.request.get("/api/v1/accounts/me")).status(), { timeout: 30_000 }).toBe(401);
  await other.close();
  expect((await main.request.get("/api/v1/accounts/me")).status(), "this browser is still signed in").toBe(200);
});

test("the admin page offers users, invites, quotas and health", async () => {
  // Act
  await main.goto("/admin");

  // Assert
  await expect(main.getByRole("heading", { name: "Admin", level: 1 })).toBeVisible();
  const tabs = main.getByRole("tablist", { name: "Admin sections" }).getByRole("tab");
  await expect(tabs).toHaveCount(4);
  await expect(main.getByRole("tab", { name: "Users" })).toHaveAttribute("aria-selected", "true");
  await expect(main.getByText("alice", { exact: false }).first()).toBeVisible();
});

test("an invite is created with a note, shown once with its link, listed and revoked", async () => {
  // Arrange
  const note = `Browser test ${Date.now()}`;
  await main.goto("/admin?tab=invites");

  // Act
  await main.getByLabel("Note (optional)").fill(note);
  await main.getByRole("button", { name: "Create invite" }).click();

  // Assert
  const created = main.getByRole("region").filter({ hasText: "Invite" }).first();
  await expect(created).toBeVisible();
  await expect(main.getByRole("button", { name: "Copy invite link" })).toBeVisible();
  await main.getByRole("button", { name: "Close" }).first().click();
  await expect(main.getByText(note)).toBeVisible();

  // Act
  await main.getByRole("button", { name: `Revoke invite ${note}` }).click();

  // Assert
  await expect(main.getByText(note)).toHaveCount(0);
});

test("the health tab lists the checks of the stack", async () => {
  // Act
  await main.goto("/admin?tab=health");

  // Assert
  const checks = main.getByRole("table", { name: "Health checks" });
  await expect(checks).toBeVisible();
  await expect(checks.getByRole("row")).not.toHaveCount(1);
});

test("a member sees no admin page", async ({ browser }) => {
  // Arrange: bob is a member (the mock IdP signs bob in on request); on a new stack he needs alice's invite first
  const me = (await (await main.request.get("/api/v1/accounts/me")).json()) as { id: string };
  const token = await inviteToken(me.id);
  await mockIdpUser("bob");
  const bobContext = await browser.newContext();
  try {
    const page = await bobContext.newPage();
    await page.goto("/login");
    await page.getByRole("button", { name: /OpenID Connect/ }).click();
    const notInvited = page.getByText("This account is not invited.");
    await expect(notInvited.or(filesPage(page))).toBeVisible();
    if (await notInvited.isVisible()) {
      await page.goto(`/invite#${token}`);
      await expect(page.getByText("You have an invite.")).toBeVisible();
      await page.getByRole("button", { name: /OpenID Connect/ }).click();
    }
    await expect(filesPage(page)).toBeVisible();

    // Act
    await page.goto("/admin");

    // Assert: the not-found page, and the server refuses the admin API as well
    await expect(page.getByRole("heading", { name: "Page not found" })).toBeVisible();
    expect((await page.request.get("/api/v1/admin/users")).status()).toBe(403);
  } finally {
    await mockIdpUser("alice");
    await bobContext.close();
  }
});
