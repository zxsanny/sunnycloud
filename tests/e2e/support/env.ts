import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { promisify } from "node:util";

const run = promisify(execFile);
export const ACC_DATA = process.env.ACC_DATA ?? "/develop/zxsanny/sunnycloud-acceptance";

/** The non-secret facts acceptance-up.sh wrote: origin, ports, the CLI wrapper. */
export function readAcceptanceEnv(): Record<string, string> {
  const path = `${ACC_DATA}/acceptance.env`;
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    throw new Error(`${path} is missing: start the stack with scripts/dev/acceptance-up.sh (${String(error)})`);
  }
  const values: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const eq = line.indexOf("=");
    if (eq > 0) values[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return values;
}

/** A one-time setup token from the server's own CLI. It is returned, never logged. */
export async function setupToken(): Promise<string> {
  const { stdout } = await run(readAcceptanceEnv().ACC_CLI, ["--json", "admin", "setup-token"]);
  const token = (JSON.parse(stdout) as { data?: { token?: string } }).data?.token;
  if (!token) throw new Error("`admin setup-token --json` printed no token (does an account already exist? run acceptance-down.sh and acceptance-up.sh)");
  return token;
}
