import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const PLUS_CTL_EVENT = "plus-ctl";

const FORBIDDEN_FLAGS = ["--home", "--data-dir"];
const PENDING_JOBS = 32;
const PENDING_EVENTS = 200;

export interface CtlEnvelope<T = unknown> {
  ok: boolean;
  command: string;
  schemaVersion: number;
  data?: T;
  error?: { code: string; message: string };
}

export interface CtlJobEvent {
  job: string;
  seq: number;
  kind: "stderr" | "exit";
  line?: string;
  exitCode?: number | null;
  cancelled?: boolean;
}

export interface CtlResult<T = unknown> {
  job: string;
  exitCode: number | null;
  signal: number | null;
  cancelled: boolean;
  envelope: CtlEnvelope<T> | null;
  parseError: string | null;
  stderr: string[];
  truncated: boolean;
}

export interface CtlRunOptions {
  /** Written to the child's stdin and nowhere else. */
  stdinSecret?: string;
  /** Live stderr lines; the full list is also in the result. */
  onStderr?: (line: string) => void;
  signal?: AbortSignal;
}

export interface CtlJob<T = unknown> {
  id: Promise<string>;
  result: Promise<CtlResult<T>>;
  cancel: () => Promise<void>;
}

export class CtlError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly result: CtlResult,
  ) {
    super(message);
    this.name = "CtlError";
  }

  /** The `data` of a failed envelope, e.g. the checks of a failed `doctor`. */
  get data(): unknown {
    return this.result.envelope?.data;
  }
}

type Handler = (event: CtlJobEvent) => void;

const handlers = new Map<string, Handler>();
const pending = new Map<string, CtlJobEvent[]>();
let hub: Promise<unknown> | null = null;

function route(event: CtlJobEvent) {
  const handler = handlers.get(event.job);
  if (handler) return handler(event);
  const queue = pending.get(event.job) ?? [];
  if (queue.length < PENDING_EVENTS) queue.push(event);
  pending.set(event.job, queue);
  if (pending.size > PENDING_JOBS) pending.delete(pending.keys().next().value!);
}

function ensureHub() {
  hub ??= listen<CtlJobEvent>(PLUS_CTL_EVENT, ({ payload }) => route(payload)).catch(
    (error) => {
      hub = null;
      throw error;
    },
  );
  return hub;
}

function subscribe(job: string, handler: Handler): () => void {
  handlers.set(job, handler);
  for (const event of pending.get(job) ?? []) handler(event);
  pending.delete(job);
  return () => {
    handlers.delete(job);
    pending.delete(job);
  };
}

export function forbiddenArg(argv: string[]): string | null {
  for (const arg of argv) {
    const key = arg.split("=")[0];
    if (FORBIDDEN_FLAGS.includes(key)) return key;
  }
  return null;
}

/** The GUI always previews a write first: the same argv plus `--dry-run`. */
export function dryRunArgv(argv: string[]): string[] {
  return argv.includes("--dry-run") ? argv : [...argv, "--dry-run"];
}

export function runCtl<T = unknown>(
  argv: string[],
  options: CtlRunOptions = {},
): CtlJob<T> {
  const bad = forbiddenArg(argv);
  const id: Promise<string> = bad
    ? Promise.reject(new Error(`${bad} is not allowed through the bridge`))
    : ensureHub().then(() =>
        invoke<string>("plus_ctl", { argv, stdinSecret: options.stdinSecret ?? null }),
      );
  const result = id.then(async (job) => {
    const stop = subscribe(job, (event) => {
      if (event.kind === "stderr" && event.line !== undefined) {
        options.onStderr?.(event.line);
      }
    });
    try {
      return await invoke<CtlResult<T>>("plus_ctl_result", { job });
    } finally {
      stop();
    }
  });
  const cancel = async () => {
    const job = await id.catch(() => null);
    if (job) await invoke("plus_ctl_cancel", { job });
  };
  if (options.signal) {
    if (options.signal.aborted) void cancel();
    else options.signal.addEventListener("abort", () => void cancel(), { once: true });
  }
  return { id, result, cancel };
}

function failure(result: CtlResult): CtlError {
  const error = result.envelope?.error;
  if (error) return new CtlError(error.message, error.code, result);
  if (result.cancelled) return new CtlError("cancelled", "cancelled", result);
  const why = result.parseError ?? `toolportctl exited with ${result.exitCode}`;
  return new CtlError(why, "bridge", result);
}

/** Runs a command and returns its `data`; a failed envelope throws a `CtlError`. */
export async function ctlData<T = unknown>(
  argv: string[],
  options?: CtlRunOptions,
): Promise<T> {
  const result = await runCtl<T>(argv, options).result;
  if (!result.envelope?.ok) throw failure(result);
  return result.envelope.data as T;
}
