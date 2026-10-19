// @vitest-environment jsdom
import { StrictMode } from "react";
import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Application } from "../src/bootstrap";

beforeEach(() => {
  history.replaceState(null, "", "/?view=saved");
  localStorage.clear();
  localStorage.setItem(
    "mt:library:v1",
    JSON.stringify([
      {
        id: "modal-test",
        title: "Saved AAPL evidence",
        createdAt: "2026-10-18T00:00:00Z",
        content: '{"symbol":"AAPL"}',
      },
    ]),
  );
  document.body.style.overflow = "auto";
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        new Response(
          JSON.stringify([
            {
              symbol: "AAPL",
              name: "Apple Inc.",
              exchange: "NASDAQ",
              kind: "Equity",
            },
          ]),
        ),
    ),
  );
  // jsdom has no layout; give visible controls geometry for the focus trap.
  vi.spyOn(HTMLElement.prototype, "getClientRects").mockImplementation(
    function (this: HTMLElement) {
      const rects =
        this.hidden || this.style.display === "none"
          ? []
          : [new DOMRect(0, 0, 100, 20)];
      return Object.assign(rects, {
        item: (index: number) => rects[index] ?? null,
      });
    },
  );
  vi.spyOn(HTMLElement.prototype, "offsetParent", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.hidden ? null : document.body;
    },
  );
});

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
  document.body.style.overflow = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const triggers = {
  about: "About this workspace",
  evidence: /^Saved AAPL evidence/,
  search: "Search a ticker or company",
} as const;

async function open(kind: keyof typeof triggers) {
  const user = userEvent.setup();
  render(
    <StrictMode>
      <Application />
    </StrictMode>,
  );
  const trigger = screen.getByRole("button", { name: triggers[kind] });
  await user.click(trigger);
  return { user, trigger, dialog: screen.getByRole("dialog") };
}

for (const kind of ["about", "evidence"] as const) {
  for (const modifier of ["Meta", "Control"]) {
    test(`${modifier}-K preserves ${kind} as the only active modal`, async () => {
      const { user, dialog, trigger } = await open(kind);
      await user.keyboard(`{${modifier}>}k{/${modifier}}`);
      expect(screen.getAllByRole("dialog")).toEqual([dialog]);
      expect(dialog.contains(document.activeElement)).toBe(true);
      await user.keyboard("{Escape}");
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(document.activeElement).toBe(trigger);
      expect(document.body.style.overflow).toBe("auto");
      // Ownership has been released, so the same shortcut now opens search.
      await user.keyboard(`{${modifier}>}k{/${modifier}}`);
      expect(screen.getByRole("dialog").getAttribute("aria-label")).toBe(
        "Search instruments",
      );
      expect(document.activeElement).toBe(
        screen.getByRole("textbox", { name: "Ticker or company" }),
      );
    });
  }
}

for (const kind of ["about", "evidence", "search"] as const) {
  test(`${kind} wraps Tab in both directions and owns Escape`, async () => {
    const { user, dialog, trigger } = await open(kind);
    if (kind === "search")
      await within(dialog).findByRole("button", { name: /Apple Inc/ });
    const first =
      kind === "search"
        ? within(dialog).getByRole("textbox")
        : within(dialog).getAllByRole("button")[0];
    const last = within(dialog).getAllByRole("button").at(-1)!;
    expect(document.activeElement).toBe(first);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(last);
    await user.tab();
    expect(document.activeElement).toBe(first);
    const escaped = vi.fn();
    window.addEventListener("keydown", escaped);
    try {
      await user.keyboard("{Escape}");
      expect(escaped).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", escaped);
    }
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(document.body.style.overflow).toBe("auto");
  });

  test(`${kind} restores focus without scrolling after button and backdrop dismissal`, async () => {
    const { user, dialog, trigger } = await open(kind);
    const focus = vi.spyOn(trigger, "focus");
    await user.click(
      within(dialog).getByRole("button", {
        name:
          kind === "about"
            ? "Back to workspace"
            : kind === "search"
              ? "Close search"
              : "Close evidence",
      }),
    );
    expect(document.activeElement).toBe(trigger);
    expect(focus).toHaveBeenLastCalledWith({ preventScroll: true });
    expect(document.body.style.overflow).toBe("auto");
    await user.click(trigger);
    expect(document.body.style.overflow).toBe("hidden");
    await user.click(screen.getByRole("dialog").parentElement!);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(document.body.style.overflow).toBe("auto");
  });
}

test("focus is scoped to the owning element even if an unrelated dialog precedes it", async () => {
  const other = document.createElement("section");
  other.setAttribute("role", "dialog");
  other.innerHTML = "<button>Unrelated dialog</button>";
  document.body.appendChild(other);
  const user = userEvent.setup();
  render(<Application />);
  await user.click(screen.getByRole("button", { name: triggers.about }));
  const button = screen.getByRole("button", { name: "Back to workspace" });
  expect(document.activeElement).toBe(button);
  await user.tab();
  expect(document.activeElement).toBe(button);
  // Programmatic focus cannot escape into the background either.
  act(() => screen.getByRole("button", { name: "Unrelated dialog" }).focus());
  expect(document.activeElement).toBe(button);
});

test("unmounting the active feature releases ownership, focus and scroll lock", async () => {
  await open("evidence");
  act(() => {
    history.pushState(null, "", "/?view=watchlist");
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(document.body.style.overflow).toBe("auto");
  expect(document.activeElement).toBe(screen.getByRole("main"));
  const user = userEvent.setup();
  await user.keyboard("{Control>}k{/Control}");
  expect(screen.getByRole("dialog").getAttribute("aria-label")).toBe(
    "Search instruments",
  );
});

test("a dialog with no enabled controls retains keyboard focus", async () => {
  const { user, dialog } = await open("about");
  within(dialog).getByRole<HTMLButtonElement>("button").disabled = true;
  await user.tab();
  expect(document.activeElement).toBe(dialog);
  await user.tab({ shift: true });
  expect(document.activeElement).toBe(dialog);
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("Tab wrapping allows a long search list to reveal its focused control", async () => {
  vi.mocked(fetch).mockResolvedValue(
    new Response(
      JSON.stringify(
        Array.from({ length: 12 }, (_, i) => ({
          symbol: `TEST${i}`,
          name: `Instrument ${i}`,
          exchange: "Test",
          kind: "Equity",
        })),
      ),
    ),
  );
  const { user, dialog } = await open("search");
  const last = await within(dialog).findByRole("button", {
    name: /Instrument 11/,
  });
  const focus = vi.spyOn(last, "focus");
  await user.tab({ shift: true });
  expect(document.activeElement).toBe(last);
  // jsdom cannot assert scroll geometry; the browser counterpart does that.
  expect(focus).toHaveBeenLastCalledWith();
});

test.each(["", "scroll", "hidden"])(
  "restores the original overflow value %j across repeated openings",
  async (overflow) => {
    const user = userEvent.setup();
    render(<Application />);
    document.body.style.overflow = overflow;
    const trigger = screen.getByRole("button", { name: triggers.about });
    for (let i = 0; i < 3; i++) {
      await user.click(trigger);
      expect(document.body.style.overflow).toBe("hidden");
      await user.keyboard("{Escape}");
      expect(document.body.style.overflow).toBe(overflow);
      expect(document.activeElement).toBe(trigger);
    }
  },
);
