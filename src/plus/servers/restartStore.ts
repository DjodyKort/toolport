import { useSyncExternalStore } from "react";
import { toast } from "sonner";
import { restartServer } from "./commands";
import type { RestartGateway, RestartStatus } from "./data";
import { errorText } from "../settings/useCtl";

/** How long "Wait, then restart" waits between two questions to the gateways. */
const WAIT_POLL_MS = 2_000;

export interface RestartResult {
  status: RestartStatus;
  gateways: RestartGateway[];
  at: number;
}

export type RestartPhase =
  | { kind: "idle" }
  | { kind: "asking"; since: number }
  | { kind: "busy"; gateways: RestartGateway[] }
  | { kind: "waiting"; gateways: RestartGateway[] };

export interface RestartState {
  phase: RestartPhase;
  /** The last answer, kept so a later glance still says what happened. */
  last: RestartResult | null;
  error: string | null;
}

const IDLE: RestartState = { phase: { kind: "idle" }, last: null, error: null };

const states = new Map<string, RestartState>();
const running = new Map<string, AbortController>();
const listeners = new Set<() => void>();
let version = 0;

function notify() {
  version += 1;
  for (const listener of listeners) listener();
}

function set(id: string, next: Partial<RestartState>) {
  states.set(id, { ...(states.get(id) ?? IDLE), ...next });
  notify();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useRestartState(id: string): RestartState {
  return useSyncExternalStore(subscribe, () => states.get(id) ?? IDLE);
}

/** Servers whose last restart failed in at least one gateway, for the list banner. */
export function useFailedRestarts(): Array<{ id: string; result: RestartResult }> {
  useSyncExternalStore(subscribe, () => version);
  return [...states.entries()]
    .filter(([, s]) => s.last?.status === "failed" && s.phase.kind === "idle")
    .map(([id, s]) => ({ id, result: s.last! }));
}

export function resetRestartStore() {
  for (const controller of running.values()) controller.abort();
  running.clear();
  states.clear();
  notify();
}

export function summary(name: string, result: RestartResult): string {
  const count = result.gateways.length;
  const processes = `${count} gateway${count === 1 ? "" : "s"}`;
  switch (result.status) {
    case "restarted":
      return `${name} restarted in ${processes}`;
    case "failed": {
      const failed = result.gateways.filter((g) => !g.restarted).length;
      return `${name}: ${failed} of ${processes} could not restart it`;
    }
    case "not_running":
      return `No gateway holds ${name} right now; it starts fresh on its next call`;
    default:
      return `Asked the gateways to restart ${name}`;
  }
}

function finish(
  id: string,
  name: string,
  status: RestartStatus,
  gateways: RestartGateway[],
) {
  const result: RestartResult = { status, gateways, at: Date.now() };
  set(id, { phase: { kind: "idle" }, last: result, error: null });
  if (status === "restarted") toast.success(summary(name, result));
  else if (status === "failed") toast.error(summary(name, result));
  else toast.message(summary(name, result));
}

async function ask(id: string, name: string, ifIdle: boolean, signal: AbortSignal) {
  const data = await restartServer(id, { ifIdle, signal });
  if (signal.aborted) return null;
  if (data.status === "busy") {
    set(id, { phase: { kind: "busy", gateways: data.gateways } });
    return data;
  }
  finish(id, name, data.status, data.gateways);
  return data;
}

function begin(id: string): AbortController | null {
  if (running.has(id)) return null;
  const controller = new AbortController();
  running.set(id, controller);
  return controller;
}

async function run(id: string, work: (signal: AbortSignal) => Promise<void>) {
  const controller = begin(id);
  if (!controller) return;
  try {
    await work(controller.signal);
  } catch (error) {
    if (!controller.signal.aborted) {
      set(id, { phase: { kind: "idle" }, error: errorText(error) });
    }
  } finally {
    running.delete(id);
  }
}

/** One click: ask the gateways whether a call is running, and restart when none is. */
export function restart(id: string, name: string) {
  set(id, { phase: { kind: "asking", since: Date.now() }, error: null });
  return run(id, async (signal) => {
    await ask(id, name, true, signal);
  });
}

/** Ends the calls that are running and restarts anyway. */
export function restartNow(id: string, name: string) {
  set(id, { phase: { kind: "asking", since: Date.now() }, error: null });
  return run(id, async (signal) => {
    await ask(id, name, false, signal);
  });
}

/** Keeps asking until no call is running, then restarts. */
export function waitThenRestart(id: string, name: string) {
  const current = states.get(id)?.phase;
  const gateways = current && "gateways" in current ? current.gateways : [];
  set(id, { phase: { kind: "waiting", gateways }, error: null });
  return run(id, async (signal) => {
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, WAIT_POLL_MS));
      if (signal.aborted) return;
      const answer = await ask(id, name, true, signal);
      if (!answer || answer.status !== "busy") return;
      set(id, { phase: { kind: "waiting", gateways: answer.gateways } });
    }
  });
}

/** Leaves the confirmation, or stops waiting; a restart already sent to the gateways cannot be
 * taken back. */
export function cancel(id: string) {
  running.get(id)?.abort();
  running.delete(id);
  set(id, { phase: { kind: "idle" } });
}
