import type { PlanStep, PlanV1 } from "../ui";
import { clientName, plural, type Collision } from "./model";

type Data = Record<string, unknown>;

const list = (value: unknown): Data[] => (Array.isArray(value) ? (value as Data[]) : []);
const str = (value: unknown): string => String(value ?? "");
const texts = (value: unknown): string[] =>
  (Array.isArray(value) ? value : []).map((item) =>
    typeof item === "string" ? item : JSON.stringify(item),
  );
const num = (value: unknown): number => (typeof value === "number" ? value : 0);

function collisionStep(row: Collision): PlanStep {
  const replaced = row.action === "replaced";
  return {
    op: replaced ? "update" : "note",
    path: row.collisionPath,
    detail: replaced
      ? `${row.skill} (${clientName(row.client)}): the command file is moved to ${row.backupPath}`
      : `${row.skill} (${clientName(row.client)}): the command file shadows the skill and stays`,
  };
}

/** The dry run (and the apply) of the skills commands answer in their own shape, not as a
 * `PlanV1`; this words that shape as the plan the confirmation shows. `done` is the past tense. */
export function planOf(command: string, data: Data, done: boolean): PlanV1 | null {
  switch (command) {
    case "skills sync": {
      const entries = list(data.entries);
      const clients = texts(data.targetedClients);
      const root = str(data.outputRoot);
      const skills = num(data.skillCount);
      const rules = num(data.ruleCount);
      return {
        summary: `${done ? "Wrote" : "Write"} ${plural(skills, "skill")} and ${plural(rules, "rule")} to ${plural(clients.length, "client")}`,
        steps: [
          ...clients.map((client): PlanStep => ({
            op: "update",
            path: root,
            detail: `${clientName(client)}: ${plural(entries.filter((e) => texts(e.clientsSynced).includes(client)).length, "entry", "entries")}`,
          })),
          ...texts(data.cleaned).map((path): PlanStep => ({
            op: "delete",
            path,
            detail: "Stale file from an earlier sync",
          })),
          ...(list(data.collisions) as unknown as Collision[]).map(collisionStep),
        ],
        effects: {},
        warnings: entries.flatMap((entry) =>
          texts(entry.warnings).map((text) => `${str(entry.name)}: ${text}`),
        ),
        undo: "toolportctl skills clean",
      };
    }
    case "skills clean": {
      const removed = texts(data.removed);
      return {
        summary: `${done ? "Removed" : "Remove"} ${plural(removed.length, "synced skill file")}`,
        steps: [
          ...removed.map((path): PlanStep => ({
            op: "delete",
            path,
            detail: "Synced skill output",
          })),
          ...(data.lockfileRemoved
            ? [
                {
                  op: "delete",
                  path: str(data.lockDir),
                  detail: "The lockfile",
                } as PlanStep,
              ]
            : []),
          { op: "note", detail: "The skills in your library are kept" },
        ],
        effects: {},
        warnings: list(data.skipped).map((row) => `Skipped: ${JSON.stringify(row)}`),
        undo: "toolportctl skills sync",
      };
    }
    case "skills uninstall": {
      const outputs = texts(data.outputs);
      return {
        summary: `${done ? "Removed" : "Remove"} the skill '${str(data.name)}' and its ${plural(outputs.length, "output")}`,
        steps: [
          { op: "delete", path: str(data.sourcePath), detail: "Skill source folder" },
          ...outputs.map((path): PlanStep => ({
            op: "delete",
            path,
            detail: "Synced skill output",
          })),
          ...(data.lockUpdated
            ? [
                {
                  op: "update",
                  path: str(data.lockDir),
                  detail: "Drop its lockfile entry",
                } as PlanStep,
              ]
            : []),
        ],
        effects: {},
        warnings: [],
        undo: "",
      };
    }
    case "skills resolve": {
      const collisions = list(data.collisions) as unknown as Collision[];
      return {
        summary:
          collisions.length === 0
            ? "Nothing shadows a synced skill"
            : `${done ? "Resolved" : "Resolve"} ${plural(collisions.length, "collision")}`,
        steps: collisions.map(collisionStep),
        effects: {},
        warnings: [],
        undo: data.backupRoot ? `Restore the files from ${str(data.backupRoot)}` : "",
      };
    }
    case "skills add": {
      const kind = str(data.type);
      return {
        summary: `${done ? "Created" : "Create"} the ${kind} '${str(data.name)}' from the template`,
        steps: texts(data.files).map((path): PlanStep => ({
          op: "create",
          path,
          detail: `New ${kind} file`,
        })),
        effects: {},
        warnings: [],
        undo: `toolportctl skills uninstall ${str(data.name)}`,
      };
    }
    default:
      return null;
  }
}
