import { configFromEnv } from "./config.ts";
import { startMockIdp } from "./server.ts";

const config = configFromEnv(process.env);
const idp = await startMockIdp(config);
console.log(`mock-idp listening on 127.0.0.1, issuer ${idp.issuer}, users ${Object.keys(config.users).join(",")}`);
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    idp.stop();
    process.exit(0);
  });
}
