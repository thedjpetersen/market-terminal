import { beforeAll, expect, test, vi } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, relative, dirname, sep } from "node:path";
import init, { execute_research } from "../public/engine/research_engine.js";
import {
  parseResearchJson,
  stringifyResearchJson,
} from "../../contracts/web/client/index";
import { fixed, fixedInput } from "../src/features/models/numbers";
import { annualFacts } from "../worker/infrastructure/sec";
import { validSymbol } from "../worker/infrastructure/yahoo";
import { readBounded, upstream } from "../worker/infrastructure/upstream";
test("web features keep consumer contracts independent and never import peer features or infrastructure", () => {
  const root = resolve("src/features");
  for (const name of readdirSync(root, { recursive: true })) {
    const path = resolve(root, String(name));
    if (!/\.tsx?$/.test(path)) continue;
    const source = readFileSync(path, "utf8"),
      owner = relative(root, path).split(sep)[0];
    for (const [, imported] of source.matchAll(
      /(?:from\s+|import\s*\()\s*["']([^"']+)["']/g,
    )) {
      if (!imported.startsWith(".")) continue;
      const target = resolve(dirname(path), imported),
        location = relative(root, target).split(sep);
      expect(
        target.includes(`${sep}infrastructure${sep}`),
        `${name} imports infrastructure`,
      ).toBe(false);
      if (location[0] !== "..")
        expect(location[0], `${name} imports a peer feature`).toBe(owner);
      if (path.endsWith(`${sep}contracts.ts`))
        expect(
          target.includes(`${sep}ui${sep}`) ||
            target.includes(`${sep}app${sep}`),
          `${name} imports presentation`,
        ).toBe(false);
    }
  }
});
beforeAll(async () => {
  await init({
    module_or_path: readFileSync(
      new URL("../public/engine/research_engine_bg.wasm", import.meta.url),
    ),
  });
});
test("the shipped WebAssembly module exactly replays every native engine fixture", () => {
  const fixture = parseResearchJson(
    readFileSync(
      new URL("../../contracts/web/v3/engine-fixtures.json", import.meta.url),
      "utf8",
    ),
  ) as { cases: { request: unknown; response: unknown }[] };
  for (const entry of fixture.cases)
    expect(
      parseResearchJson(execute_research(stringifyResearchJson(entry.request))),
    ).toEqual(entry.response);
});
test("decimal inputs and displayed evidence never round through floating point", () => {
  expect(fixedInput("9998990001.009999")).toBe(9998990001009999n);
  expect(fixed(9998990001009999n)).toBe("9,998,990,001.01");
  expect(fixedInput("-0.25", 2)).toBe(-25n);
  expect(() => fixedInput("1.001", 2)).toThrow();
  expect(() => fixedInput("1e4")).toThrow();
});
test("latest annual SEC revenue survives changing taxonomy tags; quarter and nine-month facts are excluded", () => {
  const entry = (filed: string, val: number, start = "2024-01-01") => ({
    start,
    end: "2024-12-31",
    filed,
    val,
  });
  const data = {
    facts: {
      "us-gaap": {
        Revenues: { units: { USD: [entry("2025-01-01", 100)] } },
        NewRevenue: {
          units: {
            USD: [
              entry("2025-02-01", 110),
              entry("2025-03-01", 30, "2024-10-01"),
              entry("2025-04-01", 90, "2024-04-01"),
            ],
          },
        },
      },
    },
  };
  expect(
    annualFacts(data, ["Revenues", "NewRevenue"], "USD").get("2024-12-31")
      ?.value,
  ).toBe(110);
});
test("ticker validation prevents arbitrary URL path traversal", () => {
  expect(validSymbol("BTC-USD")).toBe(true);
  expect(validSymbol("^GSPC")).toBe(true);
  for (const symbol of ["../secret", "AAPL?x=1", "https://host", "", "A..B"])
    expect(validSymbol(symbol)).toBe(false);
});
test("upstream bodies are bounded even without content-length", async () => {
  await expect(readBounded(new Response("too large"), 3)).rejects.toThrow();
  expect(await readBounded(new Response("ok"), 3)).toBe("ok");
});
test("provider requests use Workers-compatible redirect handling and reject redirects", async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response('{"ok":true}'));
  vi.stubGlobal("fetch", fetcher);
  try {
    expect(await upstream("https://query1.finance.yahoo.com/test")).toEqual({
      ok: true,
    });
    expect(fetcher.mock.calls[0][1].redirect).toBe("manual");
    fetcher.mockResolvedValue(
      new Response(null, {
        status: 302,
        headers: { Location: "https://untrusted.example/" },
      }),
    );
    await expect(
      upstream("https://query1.finance.yahoo.com/test"),
    ).rejects.toThrow("302");
  } finally {
    vi.unstubAllGlobals();
  }
});
