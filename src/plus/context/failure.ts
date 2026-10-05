/** A reply that is a failed envelope; `data` is what a command that exits 1 still prints. */
export class Failure {
  constructor(
    readonly code: string,
    readonly message: string,
    readonly data?: unknown,
  ) {}
}
