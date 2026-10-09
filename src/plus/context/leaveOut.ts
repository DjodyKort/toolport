import type { LoadItem } from "../bridge/data";
import type { WriteSpec } from "./hooks";

/** What a folder switch of a row does, in the words the dialogs use. */
export interface Switchable {
  item: LoadItem;
  /** True when the row is left out by the `leave-out` control and can be loaded again. */
  left: boolean;
  /** Why the switch cannot be used, when it cannot. */
  locked: string | null;
}

export function switchable(item: LoadItem): Switchable | null {
  if (!item.leaveOut) return null;
  const left = Boolean(item.leftOutBy);
  if (left && item.leftOutBy === "settings") {
    return {
      item,
      left,
      locked: "Left out by settings Toolport did not write. Change them where they live.",
    };
  }
  if (!left && item.kind !== "memory-index" && !item.path) {
    return { item, left, locked: "This row has no file path." };
  }
  if (!left && !item.loaded && !item.lazy) {
    return {
      item,
      left,
      locked: "This file does not load here, so there is nothing to leave out.",
    };
  }
  return { item, left, locked: null };
}

export function leaveOutSpec(
  item: LoadItem,
  cwd: string,
  action: "add" | "rm",
): WriteSpec {
  const memory = item.leaveOut === "memory";
  const target = memory ? ["--memory"] : [item.path ?? ""];
  const leaving = action === "add";
  return {
    command: "context leave-out",
    title: memory
      ? leaving
        ? "Turn auto memory off in this folder"
        : "Turn auto memory back on in this folder"
      : leaving
        ? `Leave out ${item.name} in this folder`
        : `Load ${item.name} again in this folder`,
    argv: ["context", "leave-out", action, ...target, "--cwd", cwd],
    confirmLabel: leaving ? "Leave out" : "Load again",
    phrase: leaving ? "leave out" : "load again",
  };
}
