// @vitest-environment jsdom
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, test, vi } from "vitest";
import { Search } from "../src/features/research/Search";
import type {
  Instrument,
  ResearchPort,
} from "../src/features/research/contracts";

interface PendingSearch {
  query: string;
  signal?: AbortSignal;
  resolve: (results: Instrument[]) => void;
  reject: (error: Error) => void;
}

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let port: ResearchPort;
let requests: PendingSearch[];
const onSelect = vi.fn();
const onClose = () => root.render(null);
const input = () => document.querySelector("input")!;
const rows = () => [
  ...document.querySelectorAll<HTMLButtonElement>(".search-results button"),
];
const status = () => document.querySelector('[role="status"]')?.textContent;
const unexpected = () => Promise.reject(new Error("Unexpected research call"));

function render() {
  act(() =>
    root.render(
      <StrictMode>
        <Search port={port} onSelect={onSelect} onClose={onClose} />
      </StrictMode>,
    ),
  );
}
function edit(query: string) {
  act(() => {
    // Use the native setter so React observes the same input event as typing.
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input(), query);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function press(key: string) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
  });
  act(() => input().dispatchEvent(event));
  return event;
}
async function advance(ms = 200) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
}
async function succeed(request: number, symbols: string[]) {
  await act(async () => {
    requests[request].resolve(
      symbols.map((symbol) => ({
        symbol,
        name: `${symbol} test instrument`,
        exchange: "Test exchange",
        kind: "Equity",
      })),
    );
  });
}
async function fail(request: number) {
  await act(async () => {
    requests[request].reject(new Error("Search provider unavailable"));
  });
}
async function initialResults() {
  await succeed(0, ["AAPL", "AMZN"]);
  expect(rows()).toHaveLength(2);
}
async function search(query: string) {
  edit(query);
  await advance();
  expect(requests.at(-1)?.query).toBe(query.trim() || "AAPL");
}

beforeEach(async () => {
  vi.useFakeTimers();
  onSelect.mockClear();
  requests = [];
  port = {
    search: (query, signal) =>
      // Deliberately ignore cancellation: obsolete work may still complete.
      new Promise((resolve, reject) =>
        requests.push({ query, signal, resolve, reject }),
      ),
    quotes: unexpected,
    history: unexpected,
    news: unexpected,
    company: unexpected,
  };
  const container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  render();
  await advance();
  expect(requests.map((r) => r.query)).toEqual(["AAPL"]);
});
afterEach(() => {
  act(() => root.unmount());
  document.body.replaceChildren();
  vi.useRealTimers();
});

test("editing invalidates keyboard and pointer results throughout the debounce", async () => {
  await initialResults();
  press("ArrowDown");
  edit("MSFT");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  expect(rows()).toHaveLength(0);
  expect(status()).toContain("Searching instruments");
  await advance(199);
  expect(requests).toHaveLength(1);
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  await advance(1);
  expect(requests.map((r) => r.query)).toEqual(["AAPL", "MSFT"]);
  await succeed(1, ["MSFT", "META"]);
  expect(rows()[0].className).toBe("selected");
  press("Enter");
  expect(onSelect.mock.calls).toEqual([["MSFT"]]);
  act(() => rows()[1].click());
  expect(onSelect.mock.calls).toEqual([["MSFT"], ["META"]]);
});

test("pending requests block activation even when previous results were selected", async () => {
  await initialResults();
  await search("MSFT");
  expect(status()).toContain("Searching instruments");
  press("ArrowDown");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  await succeed(1, ["MSFT"]);
  press("ArrowDown");
  press("ArrowUp");
  press("Enter");
  expect(onSelect.mock.calls).toEqual([["MSFT"]]);
});

test("Enter is consumed before selection can restore focus to the search trigger", async () => {
  await initialResults();
  expect(press("Enter").defaultPrevented).toBe(true);
  expect(onSelect.mock.calls).toEqual([["AAPL"]]);
});

test("failure blocks old results and a subsequent successful query recovers", async () => {
  await initialResults();
  await search("MSFT");
  await fail(1);
  expect(status()).toBe("Search provider unavailable");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  expect(rows()).toHaveLength(0);
  await search("META");
  await succeed(2, ["META"]);
  act(() => rows()[0].click());
  expect(onSelect.mock.calls).toEqual([["META"]]);
});

test.each(["success", "failure"])(
  "an obsolete %s cannot replace a newer successful query",
  async (completion) => {
    await search("MSFT");
    expect(requests[0].signal?.aborted).toBe(true);
    await succeed(1, ["MSFT"]);
    if (completion === "success") await succeed(0, ["AAPL"]);
    else await fail(0);
    expect(rows()).toHaveLength(1);
    expect(rows()[0].textContent).toContain("MSFT");
    press("Enter");
    expect(onSelect.mock.calls).toEqual([["MSFT"]]);
  },
);

test("returning to a previous query does not revive its successful results", async () => {
  await initialResults();
  edit("MSFT");
  edit("AAPL");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  expect(rows()).toHaveLength(0);
  await advance();
  expect(requests.map((r) => r.query)).toEqual(["AAPL", "AAPL"]);
  await succeed(1, ["AAPL"]);
  press("Enter");
  expect(onSelect.mock.calls).toEqual([["AAPL"]]);
});

test("a late completion for the same text cannot satisfy a new request", async () => {
  await search("MSFT");
  await search("AAPL");
  expect(requests.map((r) => r.query)).toEqual(["AAPL", "MSFT", "AAPL"]);
  await succeed(0, ["AMZN"]);
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  expect(status()).toContain("Searching instruments");
  await succeed(2, ["AAPL"]);
  await fail(1);
  press("Enter");
  expect(onSelect.mock.calls).toEqual([["AAPL"]]);
});

test("empty results and whitespace fallback cannot activate a previous match", async () => {
  await initialResults();
  await search("ZZZZ");
  await succeed(1, []);
  expect(status()).toBe("No matching instruments.");
  press("ArrowDown");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  await search("   ");
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  await succeed(2, ["AAPL"]);
  expect(rows()[0].className).toBe("selected");
  press("Enter");
  expect(onSelect.mock.calls).toEqual([["AAPL"]]);
});

test("changing the provider invalidates its results before the next request", async () => {
  await initialResults();
  port = { ...port };
  render();
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  expect(rows()).toHaveLength(0);
  await advance();
  expect(requests.map((r) => r.query)).toEqual(["AAPL", "AAPL"]);
  await succeed(1, ["AAPL"]);
  act(() => rows()[0].click());
  expect(onSelect.mock.calls).toEqual([["AAPL"]]);
});

test("closing aborts work and reopening starts a fresh search", async () => {
  press("Escape");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(requests[0].signal?.aborted).toBe(true);
  await succeed(0, ["AMZN"]);
  render();
  expect(document.activeElement).toBe(input());
  press("Enter");
  expect(onSelect).not.toHaveBeenCalled();
  await advance();
  expect(requests.map((r) => r.query)).toEqual(["AAPL", "AAPL"]);
  await succeed(1, ["AAPL"]);
  expect(rows()[0].textContent).toContain("AAPL");
});
