import type { MockConfig, MockUser } from "./config.ts";
import { generateSigningKey, sha256Base64url, signJwt, type SigningKey } from "./jwt.ts";

interface PendingCode {
  user: MockUser;
  clientId: string;
  redirectUri: string;
  nonce: string | null;
  challenge: string;
  expiresAt: number;
}

const JSON_HEADERS = { "content-type": "application/json", "cache-control": "no-store" };

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: JSON_HEADERS });
}

function tokenError(error: string, description: string, status = 400): Response {
  return json({ error, error_description: description }, status);
}

function badRequest(message: string): Response {
  return new Response(`mock-idp: ${message}\n`, { status: 400, headers: { "content-type": "text/plain" } });
}

/** `Authorization: Basic` carries the client id and secret form-urlencoded (RFC 6749 section 2.3.1). */
function basicCredentials(header: string | null): { id: string; secret: string } | null {
  if (!header?.startsWith("Basic ")) return null;
  const decoded = Buffer.from(header.slice(6), "base64").toString("utf8");
  const colon = decoded.indexOf(":");
  if (colon < 0) return null;
  const form = (text: string) => decodeURIComponent(text.replace(/\+/g, " "));
  return { id: form(decoded.slice(0, colon)), secret: form(decoded.slice(colon + 1)) };
}

/** The request handler of the provider for one issuer. State is in memory: a restart forgets every code. */
export async function createProvider(config: MockConfig, issuer: string): Promise<(request: Request) => Promise<Response>> {
  const key: SigningKey = await generateSigningKey();
  const codes = new Map<string, PendingCode>();
  let currentUser = config.defaultUser;

  function userNamed(name: string): MockUser | undefined {
    return Object.hasOwn(config.users, name) ? config.users[name] : undefined;
  }

  /** A configured user, or with `acceptAnyHint` a new verified one named by a plain `login_hint`. */
  function userForHint(hint: string): MockUser | undefined {
    const known = userNamed(hint);
    if (known || !config.acceptAnyHint || !/^[a-z0-9-]{1,40}$/.test(hint)) return known;
    return { sub: `any-${hint}`, email: `${hint}@e2e.test`, emailVerified: true, name: hint };
  }

  function discovery(): Response {
    return json({
      issuer,
      authorization_endpoint: `${issuer}/authorize`,
      token_endpoint: `${issuer}/token`,
      jwks_uri: `${issuer}/jwks`,
      response_types_supported: ["code"],
      subject_types_supported: ["public"],
      id_token_signing_alg_values_supported: ["RS256"],
      token_endpoint_auth_methods_supported: ["client_secret_basic", "client_secret_post"],
      code_challenge_methods_supported: ["S256"],
      scopes_supported: ["openid", "email", "profile"],
      claims_supported: ["sub", "iss", "aud", "exp", "iat", "nonce", "email", "email_verified", "name"],
    });
  }

  function authorize(url: URL): Response {
    const query = url.searchParams;
    const redirectUri = query.get("redirect_uri") ?? "";
    // A bad client or redirect URI is never redirected to: the browser stays here with the error.
    if (query.get("client_id") !== config.clientId) return badRequest("unknown client_id");
    if (!config.redirectUris.includes(redirectUri)) return badRequest("redirect_uri is not registered");
    if (query.get("response_type") !== "code") return badRequest("only response_type=code is supported");
    const challenge = query.get("code_challenge");
    if (!challenge || query.get("code_challenge_method") !== "S256") return badRequest("PKCE with S256 is required");
    const state = query.get("state");
    if (!state) return badRequest("state is required");
    const hint = query.get("login_hint");
    const userName = hint ?? currentUser;
    const user = hint === null ? userNamed(userName) : userForHint(hint);
    if (!user) return badRequest(`unknown user ${userName}`);

    const code = Buffer.from(crypto.getRandomValues(new Uint8Array(24))).toString("base64url");
    codes.set(code, {
      user,
      clientId: config.clientId,
      redirectUri,
      nonce: query.get("nonce"),
      challenge,
      expiresAt: Date.now() + config.codeTtlMs,
    });
    const target = new URL(redirectUri);
    target.searchParams.set("code", code);
    target.searchParams.set("state", state);
    return new Response(null, { status: 302, headers: { location: target.toString(), "cache-control": "no-store" } });
  }

  async function token(request: Request): Promise<Response> {
    const form = new URLSearchParams(await request.text());
    const basic = basicCredentials(request.headers.get("authorization"));
    const clientId = basic?.id ?? form.get("client_id");
    const clientSecret = basic?.secret ?? form.get("client_secret");
    if (clientId !== config.clientId || clientSecret !== config.clientSecret) {
      return tokenError("invalid_client", "client authentication failed", 401);
    }
    if (form.get("grant_type") !== "authorization_code") {
      return tokenError("unsupported_grant_type", "only authorization_code is supported");
    }
    const code = form.get("code") ?? "";
    const pending = codes.get(code);
    codes.delete(code); // one use, also when the checks below fail
    if (!pending || pending.expiresAt < Date.now()) return tokenError("invalid_grant", "unknown or expired code");
    if (pending.clientId !== clientId || pending.redirectUri !== form.get("redirect_uri")) {
      return tokenError("invalid_grant", "client or redirect_uri differs from the authorization request");
    }
    const verifier = form.get("code_verifier") ?? "";
    if (verifier === "" || (await sha256Base64url(verifier)) !== pending.challenge) {
      return tokenError("invalid_grant", "PKCE verification failed");
    }
    const now = Math.floor(Date.now() / 1000);
    const claims: Record<string, unknown> = {
      iss: issuer,
      aud: config.clientId,
      sub: pending.user.sub,
      email: pending.user.email,
      email_verified: pending.user.emailVerified,
      name: pending.user.name,
      iat: now,
      exp: now + config.tokenTtlSeconds,
    };
    if (pending.nonce !== null) claims.nonce = pending.nonce;
    return json({
      access_token: Buffer.from(crypto.getRandomValues(new Uint8Array(24))).toString("base64url"),
      token_type: "Bearer",
      expires_in: config.tokenTtlSeconds,
      id_token: await signJwt(key, claims),
    });
  }

  async function control(request: Request): Promise<Response> {
    if (request.method === "GET") return json({ user: currentUser });
    if (request.method !== "POST") return json({ error: "method not allowed" }, 405);
    const body = (await request.json()) as { user?: unknown };
    if (typeof body.user !== "string" || !userNamed(body.user)) return json({ error: "unknown user" }, 400);
    currentUser = body.user;
    return json({ user: currentUser });
  }

  return async (request: Request): Promise<Response> => {
    const url = new URL(request.url);
    const get = request.method === "GET";
    if (get && url.pathname === "/.well-known/openid-configuration") return discovery();
    if (get && url.pathname === "/jwks") return json({ keys: [key.publicJwk] });
    if (get && url.pathname === "/authorize") return authorize(url);
    if (request.method === "POST" && url.pathname === "/token") return token(request);
    if (url.pathname === "/__control/user") return control(request);
    return new Response("not found\n", { status: 404 });
  };
}
