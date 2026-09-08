import type { Session } from "./infrastructure/auth";
import { executeResearch } from "./infrastructure/engine";

export function privateJson(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
    },
  });
}

const MAX_BODY = 4 * 1024 * 1024;
export async function analytics(
  request: Request,
  env: Env,
  session: Session,
): Promise<Response> {
  if (request.method !== "POST")
    return privateJson({ error: "Method not allowed" }, 405);
  const origin = request.headers.get("Origin");
  if (
    (origin && origin !== new URL(request.url).origin) ||
    request.headers.get("Sec-Fetch-Site") === "cross-site"
  )
    return privateJson({ error: "Cross-origin requests are not allowed" }, 403);
  if (
    request.headers.get("Content-Type")?.split(";")[0].trim() !==
    "application/json"
  )
    return privateJson({ error: "Use application/json" }, 415);
  if (Number(request.headers.get("Content-Length")) > MAX_BODY)
    return privateJson({ error: "Research input exceeds 4 MiB" }, 413);
  const { success } = await env.ANALYTICS_LIMITER.limit({
    key: `market-terminal:${session.principal}`,
  });
  if (!success) {
    const response = privateJson(
      { error: "Too many model requests. Try again in a minute." },
      429,
    );
    response.headers.set("Retry-After", "60");
    return response;
  }
  const reader = request.body?.getReader();
  if (!reader)
    return privateJson({ error: "A research request is required" }, 400);
  const timer = setTimeout(() => {
    void reader.cancel();
  }, 10000);
  const deadline = Date.now() + 10000;
  try {
    const chunks: Uint8Array[] = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (Date.now() >= deadline)
        return privateJson({ error: "Request body timed out" }, 408);
      if (done) break;
      size += value.byteLength;
      if (size > MAX_BODY) {
        await reader.cancel();
        return privateJson({ error: "Research input exceeds 4 MiB" }, 413);
      }
      chunks.push(value);
    }
    const body = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      body.set(chunk, offset);
      offset += chunk.length;
    }
    // Keep JSON as text across the transport: engine integers exceed JS precision.
    const result = executeResearch(
      new TextDecoder("utf-8", { fatal: true }).decode(body),
      session.principal,
      env.ENGINE_CAPABILITIES,
    );
    return new Response(result, {
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
      },
    });
  } catch {
    return privateJson(
      {
        error: "Research input is invalid or exceeds your permitted workload.",
      },
      422,
    );
  } finally {
    clearTimeout(timer);
    reader.releaseLock();
  }
}
