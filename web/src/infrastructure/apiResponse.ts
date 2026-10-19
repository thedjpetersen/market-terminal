// Keep HTTP/session handling here so both research and analytical adapters use
// the same recovery messages without coupling their feature-owned contracts.
export async function readApiResponse(
  response: Response,
  fallback: string,
): Promise<string> {
  const contentType = response.headers
    .get("Content-Type")
    ?.split(";")[0]
    .trim()
    .toLowerCase();
  if (
    response.status === 401 ||
    response.status === 403 ||
    (response.ok && contentType === "text/html")
  )
    throw new Error("Your session has expired. Reload to sign in again.");

  if (!response.ok) {
    const data: unknown = await response.json().catch(() => null);
    throw new Error(
      data &&
        typeof data === "object" &&
        "error" in data &&
        typeof data.error === "string" &&
        data.error.trim()
        ? data.error
        : fallback,
    );
  }
  if (contentType !== "application/json") throw new Error(fallback);
  // Engine evidence must remain exact JSON text: parsing it here would round
  // integers above Number.MAX_SAFE_INTEGER before the lossless client sees them.
  return response.text();
}

export async function getApiJson<T>(
  path: string,
  fallback: string,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(path, {
    signal,
    headers: { Accept: "application/json" },
    credentials: "same-origin",
  }).catch((error: unknown) => {
    if (signal?.aborted) throw error;
    throw new Error(
      `${fallback} Check your connection or reload to refresh your session.`,
    );
  });
  const text = await readApiResponse(response, fallback);
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new Error(fallback);
  }
}
