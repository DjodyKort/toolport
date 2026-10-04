import { useEffect } from "react";

const DIALOG = '[role="dialog"], [role="alertdialog"]';

/** Gives focus back to the control that started a flow of dialogs (name, preview, result) once
 * the last one is gone. The dialogs have no trigger element, and the controls are disabled
 * while a write runs, so Radix would leave focus on the page body or on the tab strip. */
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
      if (
        open &&
        !now &&
        opener?.isConnected &&
        !(opener as HTMLButtonElement).disabled
      ) {
        opener.focus();
      }
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
