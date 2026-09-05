export class ProviderError extends Error {}

export async function readBounded(
  response: Response,
  limit: number,
): Promise<string> {
  if (Number(response.headers.get("content-length")) > limit)
    throw new ProviderError("Provider response exceeds its size limit");
  const reader = response.body?.getReader();
  if (!reader) throw new ProviderError("Provider returned no data");
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) {
        await reader.cancel();
        throw new ProviderError("Provider response exceeds its size limit");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder().decode(bytes);
}

export async function upstream(
  url: string,
  ttl = 60,
  limit = 8 * 1024 * 1024,
): Promise<unknown> {
  const response = await fetch(url, {
    headers: {
      "User-Agent": "MarketTerminal/1.0 (https://market.frodojo.com)",
      Accept: "application/json",
    },
    signal: AbortSignal.timeout(12000),
    redirect: "manual",
    cf: { cacheTtl: ttl, cacheEverything: true },
  });
  if (!response.ok)
    throw new ProviderError(
      `Source temporarily unavailable (${response.status}). Try again shortly.`,
    );
  return JSON.parse(await readBounded(response, limit));
}
export function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
export function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}
export function string(value: unknown): string {
  return typeof value === "string" ? value : "";
}
export function number(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
