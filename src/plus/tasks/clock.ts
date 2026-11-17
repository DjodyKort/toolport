let offset = 0;

/** The screen reads time here, so the fixture can show a fixed morning and tests can move it. */
export function now(): number {
  return Date.now() + offset;
}

/** Makes `now()` read `at` at this moment and move on with real time. */
export function setClock(at: number | null): void {
  offset = at === null ? 0 : at - Date.now();
}
