import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import {
  parseResearchJson,
  stringifyResearchJson,
} from "../../contracts/web/client/index";

// Exercise the actual Workers runtime and compiled Rust, with a local JWT issuer.
test.skip(
  !!process.env.MARKET_WEB_URL,
  "Local boundary tests use an ephemeral test issuer",
);
async function headers(kind = "", sub = "engine-test") {
  const { token } = (await (
    await fetch(`http://127.0.0.1:8900/token?kind=${kind}&sub=${sub}`)
  ).json()) as { token: string };
  return {
    "Cf-Access-Jwt-Assertion": token,
    "Content-Type": "application/json",
  };
}
test("every native fixture replays through authenticated HTTP without integer loss", async ({
  request,
}) => {
  const fixture = parseResearchJson(
    readFileSync(
      new URL("../../contracts/web/v3/engine-fixtures.json", import.meta.url),
      "utf8",
    ),
  ) as {
    cases: { request: unknown; response: unknown }[];
  };
  const auth = await headers("", `fixtures-${crypto.randomUUID()}`);
  for (const item of fixture.cases) {
    const result = await request.post("/api/engine", {
      headers: auth,
      data: stringifyResearchJson(item.request),
    });
    expect(result.status()).toBe(200);
    expect(result.headers()["cache-control"]).toBe("no-store");
    expect(parseResearchJson(await result.text())).toEqual(item.response);
  }
});
test("assets, session and engine reject absent or invalid authentication", async ({
  request,
}) => {
  for (const path of [
    "/",
    "/api/session",
    "/api/engine",
    "/api/health",
    "/engine/research_engine_bg.wasm",
  ]) {
    expect((await request.get(path)).status()).toBe(401);
    for (const kind of ["expired", "audience", "denied"])
      expect(
        (await request.get(path, { headers: await headers(kind) })).status(),
      ).toBe(401);
  }
  const session = await request.get("/api/session", {
    headers: await headers(),
  });
  expect(await session.json()).toMatchObject({
    email: "thedjpetersen@gmail.com",
    analytics: "cloudflare-wasm",
  });
});
test("engine bounds bodies, methods, origins, workload and request rates", async ({
  request,
}) => {
  const auth = await headers("", `limits-${crypto.randomUUID()}`);
  expect((await request.get("/api/engine", { headers: auth })).status()).toBe(
    405,
  );
  expect(
    (
      await request.post("/api/engine", {
        headers: { ...auth, Origin: "https://untrusted.example" },
        data: "{}",
      })
    ).status(),
  ).toBe(403);
  expect(
    (
      await request.post("/api/engine", {
        headers: { ...auth, "Content-Type": "text/plain" },
        data: "{}",
      })
    ).status(),
  ).toBe(415);
  expect(
    (
      await request.post("/api/engine", {
        headers: auth,
        data: "x".repeat(4 * 1024 * 1024 + 1),
      })
    ).status(),
  ).toBe(413);
  expect(
    (
      await request.post("/api/engine", { headers: auth, data: "invalid" })
    ).status(),
  ).toBe(422);
  let limited = false;
  for (let i = 0; i < 31; i++) {
    const result = await request.post("/api/engine", {
      headers: auth,
      data: "{}",
    });
    if (result.status() === 429) {
      limited = true;
      expect(result.headers()["retry-after"]).toBe("60");
      break;
    }
  }
  expect(limited).toBe(true);
});
