import type { AnalyticsPort } from "../features/models/contracts";
import { getApiJson, readApiResponse } from "./apiResponse";
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
    return readApiResponse(
      response,
      "The model could not complete. Try again.",
    );
  },
  async bars(symbol) {
    const fallback = "Historical prices are unavailable. Try again.";
    const data = await getApiJson<{
      bars?: Awaited<ReturnType<AnalyticsPort["bars"]>>;
      error?: string;
    } | null>(
      `/api/history?symbol=${encodeURIComponent(symbol)}&range=1y`,
      fallback,
    );
    if (!Array.isArray(data?.bars))
      throw new Error(
        typeof data?.error === "string" && data.error.trim()
          ? data.error
          : fallback,
      );
    return data.bars;
  },
};
