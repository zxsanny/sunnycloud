import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { configFromEnv, defaultConfig } from "../src/config.ts";
import { sha256Base64url } from "../src/jwt.ts";
import { startMockIdp, type RunningIdp } from "../src/server.ts";

const REDIRECT = "http://localhost:4000/api/v1/auth/providers/oidc/callback";
const VERIFIER = "v".repeat(64);
let idp: RunningIdp;

beforeAll(async () => {
  idp = await startMockIdp(defaultConfig({ redirectUris: [REDIRECT] }));
});
afterAll(() => idp.stop());

async function authorizeUrl(extra: Record<string, string> = {}, drop: string[] = []): Promise<string> {
  const query = new URLSearchParams({
    response_type: "code",
    client_id: "sunnycloud-acceptance",
    redirect_uri: REDIRECT,
    scope: "openid email profile",
    state: "state-1",
    nonce: "nonce-1",
    code_challenge: await sha256Base64url(VERIFIER),
    code_challenge_method: "S256",
    ...extra,
  });
  for (const name of drop) query.delete(name);
  return `${idp.issuer}/authorize?${query}`;
}

async function authorizeCode(extra: Record<string, string> = {}): Promise<string> {
  const response = await fetch(await authorizeUrl(extra), { redirect: "manual" });
  const location = new URL(response.headers.get("location") ?? "");
  return location.searchParams.get("code") ?? "";
}

function tokenRequest(code: string, overrides: Record<string, string> = {}, basic = true): Promise<Response> {
  const form = new URLSearchParams({ grant_type: "authorization_code", code, redirect_uri: REDIRECT, code_verifier: VERIFIER, ...overrides });
  const headers: Record<string, string> = { "content-type": "application/x-www-form-urlencoded" };
  if (basic) headers.authorization = `Basic ${Buffer.from("sunnycloud-acceptance:mock-client-secret").toString("base64")}`;
  return fetch(`${idp.issuer}/token`, { method: "POST", headers, body: form });
}

function decodePart(part: string): Record<string, unknown> {
  return JSON.parse(Buffer.from(part, "base64url").toString("utf8"));
}

async function verifySignature(idToken: string): Promise<boolean> {
  const [header, payload, signature] = idToken.split(".");
  const { keys } = (await (await fetch(`${idp.issuer}/jwks`)).json()) as { keys: JsonWebKey[] };
  const key = await crypto.subtle.importKey("jwk", keys[0], { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
  return crypto.subtle.verify("RSASSA-PKCS1-v1_5", key, Buffer.from(signature, "base64url"), new TextEncoder().encode(`${header}.${payload}`));
}

describe("discovery and keys", () => {
  test("the discovery document names the issuer and its endpoints", async () => {
    // Act
    const document = (await (await fetch(`${idp.issuer}/.well-known/openid-configuration`)).json()) as Record<string, unknown>;

    // Assert
    expect(document.issuer).toBe(idp.issuer);
    expect(document.authorization_endpoint).toBe(`${idp.issuer}/authorize`);
    expect(document.token_endpoint).toBe(`${idp.issuer}/token`);
    expect(document.jwks_uri).toBe(`${idp.issuer}/jwks`);
    expect(document.id_token_signing_alg_values_supported).toEqual(["RS256"]);
  });

  test("the listener is loopback and the issuer says so", () => {
    // Assert
    expect(new URL(idp.issuer).hostname).toBe("127.0.0.1");
  });
});

describe("authorization", () => {
  test("approves the default user and redirects with code and state", async () => {
    // Act
    const response = await fetch(await authorizeUrl(), { redirect: "manual" });

    // Assert
    const location = new URL(response.headers.get("location") ?? "");
    expect(response.status).toBe(302);
    expect(`${location.origin}${location.pathname}`).toBe(REDIRECT);
    expect(location.searchParams.get("state")).toBe("state-1");
    expect(location.searchParams.get("code")?.length).toBeGreaterThan(20);
  });

  test.each([
    ["an unregistered redirect_uri", { redirect_uri: "http://evil.test/cb" }],
    ["an unknown client", { client_id: "other" }],
    ["plain PKCE", { code_challenge_method: "plain" }],
    ["an unknown user", { login_hint: "mallory" }],
  ])("refuses %s without redirecting", async (_name, extra) => {
    // Act
    const response = await fetch(await authorizeUrl(extra), { redirect: "manual" });

    // Assert
    expect(response.status).toBe(400);
    expect(response.headers.get("location")).toBeNull();
  });

  test("refuses a request without a code challenge", async () => {
    // Act
    const response = await fetch(await authorizeUrl({}, ["code_challenge"]), { redirect: "manual" });

    // Assert
    expect(response.status).toBe(400);
  });
});

describe("token", () => {
  test("returns an RS256 id token with the claims the server reads, signed with the published key", async () => {
    // Arrange
    const code = await authorizeCode();

    // Act
    const body = (await (await tokenRequest(code)).json()) as { id_token: string; token_type: string };

    // Assert
    const [header, payload] = body.id_token.split(".");
    const claims = decodePart(payload);
    expect(body.token_type).toBe("Bearer");
    expect(decodePart(header).alg).toBe("RS256");
    expect(claims).toMatchObject({
      iss: idp.issuer,
      aud: "sunnycloud-acceptance",
      sub: "mock-alice",
      email: "alice@acceptance.test",
      email_verified: true,
      nonce: "nonce-1",
    });
    expect((claims.exp as number) - (claims.iat as number)).toBe(300);
    expect(await verifySignature(body.id_token)).toBe(true);
  });

  test("accepts client credentials in the body as well as in Basic", async () => {
    // Arrange
    const code = await authorizeCode();

    // Act
    const response = await tokenRequest(code, { client_id: "sunnycloud-acceptance", client_secret: "mock-client-secret" }, false);

    // Assert
    expect(response.status).toBe(200);
  });

  test("a code works once", async () => {
    // Arrange
    const code = await authorizeCode();
    await tokenRequest(code);

    // Act
    const second = await tokenRequest(code);

    // Assert
    expect(second.status).toBe(400);
    expect(((await second.json()) as { error: string }).error).toBe("invalid_grant");
  });

  test("a wrong PKCE verifier is refused", async () => {
    // Arrange
    const code = await authorizeCode();

    // Act
    const response = await tokenRequest(code, { code_verifier: "x".repeat(64) });

    // Assert
    expect(response.status).toBe(400);
    expect(((await response.json()) as { error_description: string }).error_description).toContain("PKCE");
  });

  test("a wrong client secret is refused", async () => {
    // Arrange
    const code = await authorizeCode();

    // Act
    const response = await tokenRequest(code, { client_secret: "nope", client_id: "sunnycloud-acceptance" }, false);

    // Assert
    expect(response.status).toBe(401);
  });

  test("a different redirect_uri than the authorization request is refused", async () => {
    // Arrange
    const code = await authorizeCode();

    // Act
    const response = await tokenRequest(code, { redirect_uri: "http://localhost:4000/other" });

    // Assert
    expect(response.status).toBe(400);
  });
});

describe("user selection", () => {
  test("login_hint picks bob for one request and the control API changes the default", async () => {
    // Arrange
    const hinted = await authorizeCode({ login_hint: "bob" });

    // Act
    const hintedClaims = decodePart(((await (await tokenRequest(hinted)).json()) as { id_token: string }).id_token.split(".")[1]);
    await fetch(`${idp.issuer}/__control/user`, { method: "POST", body: JSON.stringify({ user: "bob" }) });
    const controlled = decodePart(((await (await tokenRequest(await authorizeCode())).json()) as { id_token: string }).id_token.split(".")[1]);
    await fetch(`${idp.issuer}/__control/user`, { method: "POST", body: JSON.stringify({ user: "alice" }) });

    // Assert
    expect(hintedClaims.sub).toBe("mock-bob");
    expect(controlled.email).toBe("bob@acceptance.test");
  });

  test("the control API refuses an unknown user", async () => {
    // Act
    const response = await fetch(`${idp.issuer}/__control/user`, { method: "POST", body: JSON.stringify({ user: "mallory" }) });

    // Assert
    expect(response.status).toBe(400);
  });
});

describe("listener address", () => {
  const env = { MOCK_IDP_CLIENT_SECRET: "s", MOCK_IDP_REDIRECT_URIS: REDIRECT };

  test("is loopback unless the environment names another address", () => {
    // Act
    const plain = configFromEnv(env);
    const opened = configFromEnv({ ...env, MOCK_IDP_BIND_HOST: "0.0.0.0" });

    // Assert
    expect([plain.bindHost, opened.bindHost]).toEqual([undefined, "0.0.0.0"]);
  });

  test("reads the client secret from a secret file when MOCK_IDP_CLIENT_SECRET_FILE is set", () => {
    // Arrange
    const dir = mkdtempSync(join(tmpdir(), "idp-secret-"));
    const file = join(dir, "secret");
    writeFileSync(file, "from-file\n");

    // Act
    const config = configFromEnv({ MOCK_IDP_CLIENT_SECRET_FILE: file, MOCK_IDP_REDIRECT_URIS: REDIRECT });

    // Assert
    expect(config.clientSecret).toBe("from-file");
    rmSync(dir, { recursive: true });
  });
});
