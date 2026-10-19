import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

const ModalContext = createContext<{
  active: string | null;
  setActive: Dispatch<SetStateAction<string | null>>;
} | null>(null);

/** One modal owns interaction at a time; opening another leaves it in control. */
export function ModalProvider({ children }: { children: ReactNode }) {
  const [active, setActive] = useState<string | null>(null);
  const value = useMemo(() => ({ active, setActive }), [active]);
  return (
    <ModalContext.Provider value={value}>{children}</ModalContext.Provider>
  );
}

/** Attach ref and tabIndex={-1} to the dialog rendered while open. */
export function useModal() {
  const scope = useContext(ModalContext);
  if (!scope) throw new Error("useModal requires ModalProvider");
  const { active, setActive } = scope;
  const id = useId();
  const ref = useRef<HTMLElement>(null);
  const open = active === id;
  const show = useCallback(
    () => setActive((owner) => owner ?? id),
    [id, setActive],
  );
  const close = useCallback(
    () => setActive((owner) => (owner === id ? null : owner)),
    [id, setActive],
  );
  // A feature can unmount on navigation while it owns the modal.
  useEffect(() => close, [close]);
  useEffect(() => {
    const dialog = ref.current;
    if (!open || !dialog) return;
    const previous =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const visible = (el: HTMLElement) =>
      el.getClientRects().length > 0 &&
      getComputedStyle(el).visibility !== "hidden";
    const targets = () =>
      [
        ...dialog.querySelectorAll<HTMLElement>(
          'button,a[href],input,select,textarea,summary,[tabindex],[contenteditable="true"]',
        ),
      ].filter(
        (el) =>
          el.tabIndex >= 0 &&
          !el.matches(":disabled") &&
          !el.closest("[inert]") &&
          visible(el),
      );
    // Opening must preserve the workspace position; subsequent focus moves
    // should reveal controls inside a scrollable dialog.
    (targets()[0] ?? dialog).focus({ preventScroll: true });
    const focus = (target = targets()[0] ?? dialog) => target.focus();
    const focusin = (event: FocusEvent) => {
      if (event.target instanceof Node && !dialog.contains(event.target))
        focus();
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close();
        return;
      }
      if (event.key !== "Tab") return;
      event.stopPropagation();
      const controls = targets();
      const first = controls[0],
        last = controls.at(-1);
      if (!first) {
        event.preventDefault();
        focus(dialog);
        return;
      }
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !controls.includes(document.activeElement as HTMLElement))
      ) {
        event.preventDefault();
        focus(last);
      } else if (
        !event.shiftKey &&
        (document.activeElement === last ||
          !controls.includes(document.activeElement as HTMLElement))
      ) {
        event.preventDefault();
        focus(first);
      }
    };
    document.addEventListener("keydown", keydown);
    document.addEventListener("focusin", focusin);
    return () => {
      document.removeEventListener("keydown", keydown);
      document.removeEventListener("focusin", focusin);
      document.body.style.overflow = overflow;
      const target =
        previous?.isConnected &&
        previous !== document.body &&
        !previous.matches(":disabled") &&
        visible(previous)
          ? previous
          : document.querySelector<HTMLElement>("main");
      target?.focus({ preventScroll: true });
    };
  }, [open, close]);
  return { open, show, close, ref };
}
