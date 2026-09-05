import { useEffect } from "react";
/** Keep keyboard focus and scroll inside the active modal, then restore them. */
export function useModal(open: boolean, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const previous =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    document
      .querySelector<HTMLElement>(
        '[role="dialog"] button, [role="dialog"] input',
      )
      ?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
      if (event.key !== "Tab") return;
      const dialog = document.querySelector('[role="dialog"]');
      const targets = [
        ...(dialog?.querySelectorAll<HTMLElement>(
          'button:not(:disabled),a[href],input,select,[tabindex="0"]',
        ) ?? []),
      ].filter((el) => el.offsetParent !== null);
      const first = targets[0],
        last = targets.at(-1);
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !dialog?.contains(document.activeElement))
      ) {
        event.preventDefault();
        last?.focus();
      } else if (
        !event.shiftKey &&
        (document.activeElement === last ||
          !dialog?.contains(document.activeElement))
      ) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", keydown);
    return () => {
      document.body.style.overflow = overflow;
      document.removeEventListener("keydown", keydown);
      previous?.focus();
    };
  }, [open, close]);
}
