import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, type BrowserContext, type Page } from "@playwright/test";
import { readAcceptanceEnv } from "./env.ts";

const run = promisify(execFile);

/** Signs alice in through the real sign-in page and the mock IdP (the account already exists). */
export async function signInAlice(context: BrowserContext): Promise<Page> {
  const page = await context.newPage();
  await page.goto("/login");
  await page.getByRole("button", { name: /OpenID Connect/ }).click();
  await expect(page.getByRole("heading", { name: "Files", level: 1 })).toBeVisible();
  return page;
}

/** Stops or starts the acceptance API unit only (scripts/dev/acceptance-api-ctl.sh). */
export async function apiUnit(action: "stop" | "start"): Promise<void> {
  await run(readAcceptanceEnv().ACC_API_CTL, [action], { timeout: 120_000 });
}
