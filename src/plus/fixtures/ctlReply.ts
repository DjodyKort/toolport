/** A reply of the fixture bridge that is not plain `data`: a failed envelope, or a run that
 * stays open until it is cancelled (a sign-in waiting for the browser). */
export class CtlReplyFailure {
  constructor(
    readonly code: string,
    readonly message: string,
    readonly data?: unknown,
  ) {}
}

export class CtlReplyHeld {
  constructor(
    /** stderr lines the run prints while it waits. */
    readonly lines: string[],
    /** The data it returns when it is released instead of cancelled. */
    readonly data?: unknown,
  ) {}
}

export const ctlReplyFailure = (code: string, message: string, data?: unknown) =>
  new CtlReplyFailure(code, message, data);
