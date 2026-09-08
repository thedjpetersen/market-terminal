import {
  initSync,
  execute_authenticated_research,
} from "../../public/engine/research_engine.js";
import wasm from "../../public/engine/research_engine_bg.wasm";

// Synchronous execution completes before another request can enter this instance.
initSync({ module: wasm });

export function executeResearch(
  request: string,
  principal: string,
  capabilities: string,
): string {
  return execute_authenticated_research(
    request,
    "market-terminal",
    principal,
    capabilities,
  );
}
