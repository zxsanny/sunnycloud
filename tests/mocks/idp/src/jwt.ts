const ENCODER = new TextEncoder();

export function base64url(bytes: ArrayBuffer | Uint8Array): string {
  return Buffer.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)).toString("base64url");
}

/** An RS256 signing key generated at start; the public half is served as a JWKS. */
export interface SigningKey {
  kid: string;
  privateKey: CryptoKey;
  publicJwk: JsonWebKey & { kid: string; use: string; alg: string };
}

export async function generateSigningKey(): Promise<SigningKey> {
  const pair = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );
  const kid = base64url(crypto.getRandomValues(new Uint8Array(12)));
  const jwk = await crypto.subtle.exportKey("jwk", pair.publicKey);
  return { kid, privateKey: pair.privateKey, publicJwk: { ...jwk, kid, use: "sig", alg: "RS256" } };
}

export async function signJwt(key: SigningKey, claims: Record<string, unknown>): Promise<string> {
  const header = { alg: "RS256", typ: "JWT", kid: key.kid };
  const signingInput = `${base64url(ENCODER.encode(JSON.stringify(header)))}.${base64url(ENCODER.encode(JSON.stringify(claims)))}`;
  const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", key.privateKey, ENCODER.encode(signingInput));
  return `${signingInput}.${base64url(signature)}`;
}

export async function sha256Base64url(text: string): Promise<string> {
  return base64url(await crypto.subtle.digest("SHA-256", ENCODER.encode(text)));
}
