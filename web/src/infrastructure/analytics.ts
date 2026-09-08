import type { AnalyticsPort } from "../features/models/contracts";
export const cloudAnalytics: AnalyticsPort = {
  async execute(request) {
    const response = await fetch("/api/engine", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body: request,
      credentials: "same-origin",
      redirect: "error",
      signal: AbortSignal.timeout(35000),
    }).catch(() => {
      throw new Error(
        "The engine could not be reached. Reload to refresh your session, then retry.",
      );
    });
    if (!response.ok) {
      if (response.status === 401 || response.status === 403)
        throw new Error("Your session has expired. Reload to sign in again.");
      const error = (await response.json().catch(() => ({}))) as {
        error?: string;
      };
      throw new Error(
        error.error ?? "The model could not complete. Try again.",
      );
    }
    if (!response.headers.get("Content-Type")?.includes("application/json"))
      throw new Error("Your session has expired. Reload to sign in again.");
    return response.text();
  },
  async bars(symbol) {
    const response = await fetch(
      `/api/history?symbol=${encodeURIComponent(symbol)}&range=1y`,
    );
    const data = (await response.json()) as {
      bars?: Awaited<ReturnType<AnalyticsPort["bars"]>>;
      error?: string;
    };
    if (!response.ok || !data.bars)
      throw new Error(data.error ?? "Historical prices are unavailable.");
    return data.bars;
  },
};
