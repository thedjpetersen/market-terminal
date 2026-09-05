import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  executeEngine, parseResearchJson, stringifyResearchJson, ResearchHttpError,
} from "./index.ts";
import type { ResearchRequest, ResearchResponse } from "./index.ts";

const fixtureText = readFileSync(new URL("../v3/engine-fixtures.json", import.meta.url), "utf8");
const fixtures = parseResearchJson(fixtureText) as {
  cases: { request: ResearchRequest; response: ResearchResponse }[];
};

for (const fixture of fixtures.cases) {
  test(`exact Rust fixture round trip: ${fixture.request.request_id}`, async () => {
    const fetchFixture: typeof fetch = async (_url, init) => {
      assert.deepEqual(parseResearchJson(String(init?.body)), fixture.request);
      assert.equal(new Headers(init?.headers).get("content-type"), "application/json");
      return new Response(stringifyResearchJson(fixture.response));
    };
    const response = await executeEngine("http://localhost/v1/engine", fixture.request, { fetch: fetchFixture });
    assert.deepEqual(response, fixture.response);
    assert.equal(response.schema_version, 1n);
  });
}

test("large engine outputs retain every digit", () => {
  const fixture = fixtures.cases.find(item => item.request.request_id === "contract:option-large-integer");
  assert.ok(fixture, "Rust must generate the large-integer regression fixture");
  const wire = stringifyResearchJson(fixture.response);
  assert.ok(wire.includes("9998990001009999"));
  assert.notEqual(JSON.stringify(JSON.parse(wire)), wire);
  assert.deepEqual(parseResearchJson(wire), fixture.response);
});

test("signed and unsigned 64-bit limits, strings, and unsafe inputs", () => {
  const values = { min: -9223372036854775808n, max: 18446744073709551615n, text: "18446744073709551615" };
  assert.deepEqual(parseResearchJson(stringifyResearchJson(values)), values);
  for (const value of [9007199254740992, Infinity, NaN, 0.5]) {
    assert.throws(() => stringifyResearchJson({ value }), /Unsafe or noninteger/);
  }
  for (const json of ['{"n":1.1}', '{"n":1e999}', '{"n":1,"n":2}']) {
    assert.throws(() => parseResearchJson(json));
  }
});

test("typed engine rejections and HTTP problems remain distinct", async () => {
  const request = fixtures.cases[0].request;
  const error = { schema_version: 1n, request_id: request.request_id, status: "error", error: { code: "backtest_rejected", message: "rejected" } };
  assert.deepEqual(await executeEngine("http://localhost/v1/engine", request, {
    fetch: async () => new Response(stringifyResearchJson(error), { status: 422 }),
  }), error);
  await assert.rejects(executeEngine("http://localhost/v1/engine", request, {
    fetch: async () => new Response('{"code":"unauthorized","message":"denied"}', { status: 401 }),
  }), ResearchHttpError);
  await assert.rejects(executeEngine("http://localhost/v1/engine", request, {
    fetch: async () => new Response(stringifyResearchJson({ ...error, request_id: "other" })),
  }), /mismatched/);
});
