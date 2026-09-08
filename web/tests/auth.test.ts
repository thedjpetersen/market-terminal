import { beforeAll, expect, test } from "vitest";
import { createLocalJWKSet, exportJWK, generateKeyPair, SignJWT } from "jose";
import { authenticate } from "../worker/infrastructure/auth";

const config = {
  ACCESS_ISSUER: "https://frodojo.cloudflareaccess.com",
  ACCESS_AUD: "pending-access-application",
  ACCESS_EMAILS: "thedjpetersen@gmail.com",
} as const;
let privateKey: CryptoKey;
let keys: ReturnType<typeof createLocalJWKSet>;
beforeAll(async () => {
  const pair = await generateKeyPair("RS256");
  privateKey = pair.privateKey;
  keys = createLocalJWKSet({
    keys: [{ ...(await exportJWK(pair.publicKey)), kid: "test", alg: "RS256" }],
  });
});
async function token(
  overrides: Record<string, unknown> = {},
  key = privateKey,
) {
  return new SignJWT({
    iss: config.ACCESS_ISSUER,
    aud: config.ACCESS_AUD,
    sub: "user-1",
    email: config.ACCESS_EMAILS,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 300,
    ...overrides,
  })
    .setProtectedHeader({ alg: "RS256", kid: "test" })
    .sign(key);
}
function request(token?: string) {
  return new Request("https://market.frodojo.com/api/engine", {
    headers: token ? { "Cf-Access-Jwt-Assertion": token } : {},
  });
}
test("verified sessions resolve the server-owned identity", async () => {
  expect(await authenticate(request(await token()), config, keys)).toEqual({
    principal: "user-1",
    email: config.ACCESS_EMAILS,
  });
});
test("missing, expired, future, wrong issuer, wrong audience and unapproved users fail closed", async () => {
  expect(await authenticate(request(), config, keys)).toBeNull();
  expect(await authenticate(request("forged"), config, keys)).toBeNull();
  for (const override of [
    { exp: 1 },
    { exp: undefined },
    { nbf: Math.floor(Date.now() / 1000) + 300 },
    { iss: "https://other.example" },
    { aud: "other-app" },
    { email: "stranger@example.com" },
    { sub: "bad/user" },
    { sub: undefined },
  ]) {
    expect(
      await authenticate(request(await token(override)), config, keys),
    ).toBeNull();
  }
  const other = await generateKeyPair("RS256");
  expect(
    await authenticate(
      request(await token({}, other.privateKey)),
      config,
      keys,
    ),
  ).toBeNull();
});
