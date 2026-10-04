import type { PlanV1 } from "../ui";

export interface ServerRef {
  id: string;
  name: string;
}

/** `secret set` and `secret rm` have no dry run of their own, so the plan the dialogs show is
 * written from what the command does: it only touches the vault entry of one key. */
export function setPlan(
  server: ServerRef,
  key: string,
  existing: boolean | null,
): PlanV1 {
  const replace = existing === true;
  return {
    summary: replace
      ? `Replace ${key} for ${server.name} in the vault`
      : `Store ${key} for ${server.name} in the vault`,
    steps: [
      {
        op: existing === false ? "create" : "update",
        path: "vault",
        detail: replace
          ? `Replace the value of ${key} for ${server.name}`
          : `Store the value of ${key} for ${server.name}`,
        keys: [key],
      },
      {
        op: "note",
        detail:
          "The value is sent on stdin, never shown again and not written to a config file",
      },
    ],
    effects: {},
    warnings:
      existing === false
        ? []
        : ["An existing value is overwritten and cannot be restored"],
    undo: existing === false ? `toolportctl secret rm ${server.id} ${key}` : "",
  };
}

export function removePlan(server: ServerRef, key: string): PlanV1 {
  return {
    summary: `Remove ${key} for ${server.name} from the vault`,
    steps: [
      {
        op: "delete",
        path: "vault",
        detail: `Delete the stored value of ${key} for ${server.name}`,
        keys: [key],
      },
      {
        op: "note",
        detail: `${server.name} keeps its entry and shows a login needed until a new value is set`,
      },
    ],
    effects: {},
    warnings: ["The value cannot be restored once it is removed"],
    undo: `toolportctl secret set ${server.id} ${key}`,
  };
}

export const setLine = (server: ServerRef, key: string) =>
  `toolportctl secret set ${server.id} ${key}`;
export const removeLine = (server: ServerRef, key: string) =>
  `toolportctl secret rm ${server.id} ${key}`;
