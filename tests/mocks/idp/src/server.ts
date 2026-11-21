import type { MockConfig } from "./config.ts";
import { createProvider } from "./provider.ts";

export interface RunningIdp {
  issuer: string;
  stop(): void;
}

/** Binds 127.0.0.1 only; there is deliberately no way to choose another address. */
export async function startMockIdp(config: MockConfig): Promise<RunningIdp> {
  let handle: ((request: Request) => Promise<Response>) | null = null;
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: config.port,
    fetch: (request) => (handle ? handle(request) : new Response("starting\n", { status: 503 })),
  });
  const issuer = config.issuer ?? `http://127.0.0.1:${server.port}`;
  handle = await createProvider(config, issuer);
  return { issuer, stop: () => void server.stop(true) };
}
