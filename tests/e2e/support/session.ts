import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "./env.ts";

const run = promisify(execFile);

/**
 * The Files page is up: its h1 while the root folder loads, then the breadcrumb that replaces it (the two never show together).
 * A wait on the h1 alone loses a race with a fast root load and fails a sign-in that worked.
 */
export function filesPage(page: Page): Locator {
  return page.getByRole("heading", { name: "Files", level: 1 }).or(page.getByRole("navigation", { name: "Breadcrumb" }));
}

/** Signs alice in through the real sign-in page and the mock IdP (the account already exists). */
export async function signInAlice(context: BrowserContext): Promise<Page> {
  const page = await context.newPage();
  await page.goto("/login");
  await page.getByRole("button", { name: /OpenID Connect/ }).click();
  await expect(filesPage(page)).toBeVisible();
  return page;
}

/** Stops or starts the acceptance API unit only (scripts/dev/acceptance-api-ctl.sh). */
export async function apiUnit(action: "stop" | "start"): Promise<void> {
  await run(readAcceptanceEnv().ACC_API_CTL, [action], { timeout: 120_000 });
}

/** The mock IdP signs in this user from now on (`alice` or `bob`). */
export async function mockIdpUser(user: "alice" | "bob"): Promise<void> {
  const response = await fetch(`${readAcceptanceEnv().ACC_ISSUER}/__control/user`, { method: "POST", body: JSON.stringify({ user }) });
  if (!response.ok) throw new Error(`the mock IdP refused to switch to ${user}: ${response.status}`);
}

/** A one-time invite token from the server's own CLI, for the admin account `adminId`. It is returned, never logged. */
export async function inviteToken(adminId: string): Promise<string> {
  const { stdout } = await run(readAcceptanceEnv().ACC_CLI, ["--json", "admin", "invite", "--admin", adminId, "create"], { timeout: 60_000 });
  const token = (JSON.parse(stdout) as { data?: { token?: string } }).data?.token;
  if (!token) throw new Error("`admin invite create --json` printed no token");
  return token;
}
