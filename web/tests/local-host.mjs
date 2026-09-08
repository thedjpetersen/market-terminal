// Local integration host: real signature verification, fresh throwaway signing keys.
// This file is never imported by the Worker or deployed.
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { generateKeyPair, exportJWK, SignJWT } from "jose";

const issuer = "http://127.0.0.1:8900";
const { privateKey, publicKey } = await generateKeyPair("RS256", {
  extractable: true,
});
const jwk = {
  ...(await exportJWK(publicKey)),
  kid: "local-test",
  alg: "RS256",
  use: "sig",
};
const server = createServer(async (request, response) => {
  const url = new URL(request.url, issuer);
  response.setHeader("Content-Type", "application/json");
  response.setHeader("Cache-Control", "no-store");
  if (url.pathname === "/cdn-cgi/access/certs")
    response.end(JSON.stringify({ keys: [jwk] }));
  else if (url.pathname === "/token") {
    const kind = url.searchParams.get("kind");
    const token = await new SignJWT({
      email:
        kind === "denied" ? "stranger@example.com" : "thedjpetersen@gmail.com",
    })
      .setProtectedHeader({ alg: "RS256", kid: "local-test" })
      .setIssuer(issuer)
      .setAudience(kind === "audience" ? "another-app" : "local-test")
      .setSubject(url.searchParams.get("sub") || "local-browser-test")
      .setIssuedAt()
      .setExpirationTime(kind === "expired" ? "0s" : "1h")
      .sign(privateKey);
    response.end(JSON.stringify({ token }));
  } else if (url.pathname === "/ready") {
    try {
      const result = await fetch("http://127.0.0.1:8899/api/session");
      response.statusCode = result.status === 401 ? 200 : 503;
    } catch {
      response.statusCode = 503;
    }
    response.end("{}");
  } else {
    response.statusCode = 404;
    response.end("{}");
  }
});
server.listen(8900, "127.0.0.1");
const child = spawn(
  process.execPath,
  [
    "node_modules/wrangler/bin/wrangler.js",
    "dev",
    "--ip",
    "127.0.0.1",
    "--port",
    "8899",
    "--var",
    `ACCESS_ISSUER:${issuer}`,
    "--var",
    "ACCESS_AUD:local-test",
  ],
  { stdio: "inherit" },
);
function stop() {
  child.kill("SIGTERM");
  server.close();
}
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
child.on("exit", (code) => {
  server.close();
  process.exitCode = code || 0;
});
