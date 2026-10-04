import { useEffect } from "react";

const DIALOG = '[role="dialog"], [role="alertdialog"]';

/** Gives focus back to the control that started a dialog once the last dialog is gone. The
 * dialogs here have no trigger element, so Radix would leave focus on the page body, and a
 * flow of several dialogs (ask, preview, result) would lose it at every step. */
export function useRestoreFocus() {
  useEffect(() => {
    let opener: HTMLElement | null = null;
    let open = document.querySelector(DIALOG) !== null;
    const onFocusIn = (event: FocusEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        target !== document.body &&
        !target.closest(DIALOG)
      ) {
        opener = target;
      }
    };
    const observer = new MutationObserver(() => {
      const now = document.querySelector(DIALOG) !== null;
      const lost =
        document.activeElement === null || document.activeElement === document.body;
      if (open && !now && lost && opener?.isConnected) opener.focus();
      open = now;
    });
    document.addEventListener("focusin", onFocusIn);
    observer.observe(document.body, { childList: true });
    return () => {
      document.removeEventListener("focusin", onFocusIn);
      observer.disconnect();
    };
  }, []);
}
