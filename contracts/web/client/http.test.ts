import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout } from "node:timers/promises";
import test from "node:test";
import { executeEngine, parseResearchJson, stringifyResearchJson } from "./index.ts";
import type { ResearchRequest, ResearchResponse } from "./index.ts";

test("real HTTP host preserves every Rust fixture through the JavaScript client", async () => {
  const reservation = createServer();
  await new Promise<void>(resolve => reservation.listen(0, "127.0.0.1", resolve));
  const address = reservation.address();
  assert.ok(address && typeof address !== "string");
  const endpoint = `http://127.0.0.1:${address.port}`;
  await new Promise<void>((resolve, reject) => reservation.close(error => error ? reject(error) : resolve()));
  const cwd = await mkdtemp(join(tmpdir(), "market-terminal-client-"));
  const token = randomBytes(32).toString("hex");
  const binary = process.env.MARKET_TERMINAL_API_BINARY
    ?? new URL("../../../target/debug/market-terminal-api", import.meta.url).pathname;
  const child = spawn(binary, [], {
    cwd, stdio: "ignore",
    env: { PATH: process.env.PATH, MARKET_TERMINAL_API_TOKEN: token,
      MARKET_TERMINAL_API_BIND: `127.0.0.1:${address.port}` },
  });
  let spawnError: Error | undefined;
  child.on("error", error => { spawnError = error; });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      if (spawnError) throw spawnError;
      try { ready = (await fetch(`${endpoint}/healthz`)).ok; } catch { /* booting */ }
      if (ready) break;
      await setTimeout(50);
    }
    assert.ok(ready, "API must become healthy");
    const headers = { authorization: `Bearer ${token}` };
    const fixtures = parseResearchJson(await readFile(new URL("../v3/engine-fixtures.json", import.meta.url), "utf8")) as {
      cases: { request: ResearchRequest; response: ResearchResponse }[];
    };
    for (const fixture of fixtures.cases) {
      assert.deepEqual(await executeEngine(`${endpoint}/v1/engine`, fixture.request, { headers }), fixture.response);
    }
    const response = await fetch(`${endpoint}/v1/engine`, {
      method: "POST", headers: { ...headers, "content-type": "application/json" },
      body: stringifyResearchJson({ ...fixtures.cases[0].request, request_id: "x".repeat(100_000) }),
    });
    assert.equal(response.headers.get("x-request-id"), null);
    assert.equal(response.status, 400);
  } finally {
    if (child.exitCode === null && !spawnError) {
      const exited = new Promise<void>(resolve => child.once("exit", () => resolve()));
      child.kill();
      await exited;
    }
    await rm(cwd, { recursive: true, force: true });
  }
});
