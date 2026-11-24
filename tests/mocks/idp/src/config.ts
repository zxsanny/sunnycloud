import { readFileSync } from "node:fs";

export interface MockUser {
  sub: string;
  email: string;
  emailVerified: boolean;
  name: string;
}

export interface MockConfig {
  /** 0 picks a free port. */
  port: number;
  /** The listener address: 127.0.0.1 unless `MOCK_IDP_BIND_HOST` says otherwise. The mock approves anyone, so only an isolated container network may set it. */
  bindHost?: string;
  /** Defaults to http://127.0.0.1:<bound port>. The server's `oidc_issuer` must equal it exactly. */
  issuer?: string;
  clientId: string;
  clientSecret: string;
  /** Exact-match list; authorization requests to any other redirect URI are refused. */
  redirectUris: string[];
  users: Record<string, MockUser>;
  /** Opt-in: a `login_hint` that names no user (letters, digits, dashes, at most 40) signs in as a new verified user of that name, so a test can mint one identity per round. */
  acceptAnyHint: boolean;
  /** The user the authorization endpoint approves unless `login_hint` or the control API says otherwise. */
  defaultUser: string;
  /** How long the `slow` fault delays the token answer; the default is 12 s, past the server's 10 s provider timeout. */
  slowMs?: number;
  codeTtlMs: number;
  tokenTtlSeconds: number;
}

export const DEFAULT_USERS: Record<string, MockUser> = {
  alice: { sub: "mock-alice", email: "alice@acceptance.test", emailVerified: true, name: "Alice Admin" },
  bob: { sub: "mock-bob", email: "bob@acceptance.test", emailVerified: true, name: "Bob Builder" },
};

export function defaultConfig(overrides: Partial<MockConfig> = {}): MockConfig {
  return {
    port: 0,
    clientId: "sunnycloud-acceptance",
    clientSecret: "mock-client-secret",
    redirectUris: [],
    users: DEFAULT_USERS,
    acceptAnyHint: false,
    defaultUser: "alice",
    codeTtlMs: 60_000,
    tokenTtlSeconds: 300,
    ...overrides,
  };
}

/** `NAME` from the environment, or the content of the file `NAME_FILE` names (a container secret), trimmed of line ends. */
function required(env: Record<string, string | undefined>, name: string): string {
  const file = env[`${name}_FILE`];
  const value = file ? readFileSync(file, "utf8").replace(/[\r\n]+$/, "") : env[name];
  if (value === undefined || value === "") throw new Error(`${name} is required`);
  return value;
}

/** Settings from the environment; MOCK_IDP_USERS is optional JSON of the same shape as DEFAULT_USERS. */
export function configFromEnv(env: Record<string, string | undefined>): MockConfig {
  const users = env.MOCK_IDP_USERS ? (JSON.parse(env.MOCK_IDP_USERS) as Record<string, MockUser>) : DEFAULT_USERS;
  const port = Number(env.MOCK_IDP_PORT ?? "9000");
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error("MOCK_IDP_PORT is not a port");
  return defaultConfig({
    port,
    bindHost: env.MOCK_IDP_BIND_HOST || undefined,
    issuer: env.MOCK_IDP_ISSUER,
    clientId: env.MOCK_IDP_CLIENT_ID ?? "sunnycloud-acceptance",
    clientSecret: required(env, "MOCK_IDP_CLIENT_SECRET"),
    redirectUris: required(env, "MOCK_IDP_REDIRECT_URIS").split(","),
    users,
    acceptAnyHint: env.MOCK_IDP_ACCEPT_ANY_HINT === "1",
    defaultUser: env.MOCK_IDP_DEFAULT_USER ?? "alice",
  });
}
