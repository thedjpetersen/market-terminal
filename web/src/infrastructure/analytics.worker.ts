interface BrowserEngine {
  default: (input?: unknown) => Promise<unknown>;
  execute_research: (request: string) => string;
}
let engine: Promise<BrowserEngine> | undefined;
self.onmessage = async (event: MessageEvent<{ request: string }>) => {
  try {
    engine ??= (async () => {
      const path = "/engine/research_engine.js";
      const module: BrowserEngine = await import(/* @vite-ignore */ path);
      await module.default();
      return module;
    })();
    const module = await engine;
    self.postMessage({ result: module.execute_research(event.data.request) });
  } catch (error) {
    self.postMessage({ error: String(error) });
  }
};
