import { parse, stringify } from "lossless-json";
import type { EngineRequest, EngineResponse, ProblemResponse } from "../v3/market-terminal-api.ts";

/** JSON integers become bigint, including schema versions and small counts. */
export type Lossless<T> = T extends number ? bigint
  : T extends readonly unknown[] ? { readonly [K in keyof T]: Lossless<T[K]> }
  : T extends object ? { readonly [K in keyof T]: Lossless<T[K]> }
  : T;

export type ResearchRequest = Lossless<EngineRequest>;
export type ResearchResponse = Lossless<EngineResponse>;

/** Read original response text: Response.json() has already lost precision. */
export function parseResearchJson(text: string): unknown {
  return parse(text, undefined, {
    parseNumber(source: string): bigint {
      if (!/^-?(0|[1-9][0-9]*)$/.test(source)) {
        throw new TypeError("Research contracts require integer JSON number tokens");
      }
      return BigInt(source);
    },
  });
}

/** Emit bigint as exact JSON number tokens without changing the v1 wire schema. */
export function stringifyResearchJson(value: unknown): string {
  const text = stringify(value, (_key: string, item: unknown) => {
    if (typeof item === "number" && !Number.isSafeInteger(item)) {
      throw new TypeError("Unsafe or noninteger number: supply an exact bigint instead");
    }
    return item;
  });
  if (text === undefined) throw new TypeError("A JSON value is required");
  return text;
}

export class ResearchHttpError extends Error {
  readonly status: number;
  readonly problem: unknown;
  constructor(status: number, problem: unknown) {
    super(`Research HTTP request failed (${status})`);
    this.status = status;
    this.problem = problem;
  }
}

/**
 * Credentials and cancellation stay with the caller. No persistence or retries.
 * Validates the response envelope; analytical invariants remain server-owned.
 */
export async function executeEngine(
  endpoint: string | URL,
  request: ResearchRequest,
  options: { headers?: HeadersInit; signal?: AbortSignal; fetch?: typeof fetch } = {},
): Promise<ResearchResponse> {
  const headers = new Headers(options.headers);
  headers.set("content-type", "application/json");
  const response = await (options.fetch ?? fetch)(endpoint, {
    method: "POST", headers, signal: options.signal,
    body: stringifyResearchJson(request),
  });
  const body = parseResearchJson(await response.text());
  if (!isRecord(body) || body.schema_version !== 1n || body.request_id !== request.request_id) {
    if (!response.ok) throw new ResearchHttpError(response.status, body as ProblemResponse);
    throw new TypeError("Unsupported or mismatched engine response envelope");
  }
  if (body.status === "error" && isRecord(body.error)
    && typeof body.error.code === "string" && typeof body.error.message === "string") {
    return body as ResearchResponse;
  }
  const result = {
    run_backtest: "backtest", compare_backtests: "backtest_comparison",
    price_option: "option_analytics", analyze_bond: "bond_analytics",
  }[request.operation];
  if (!response.ok) throw new ResearchHttpError(response.status, body);
  if (body.status !== "ok" || body.result_type !== result || !isRecord(body.data)) {
    throw new TypeError("Unexpected engine result envelope");
  }
  return body as ResearchResponse;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
