import type { AnalyticsPort } from "../features/models/contracts";
export const browserAnalytics: AnalyticsPort = {
  execute(request) {
    return new Promise((resolve, reject) => {
      const worker = new Worker(
        new URL("./analytics.worker.ts", import.meta.url),
        { type: "module" },
      );
      const timer = setTimeout(() => {
        worker.terminate();
        reject(
          new Error(
            "The model exceeded the 30-second device budget. Try a smaller input.",
          ),
        );
      }, 30_000);
      worker.onmessage = (event) => {
        clearTimeout(timer);
        worker.terminate();
        if (event.data.error) reject(new Error(event.data.error));
        else resolve(event.data.result);
      };
      worker.onerror = () => {
        clearTimeout(timer);
        worker.terminate();
        reject(
          new Error(
            "The research engine could not start. Reload the page and try again.",
          ),
        );
      };
      worker.postMessage({ request });
    });
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
