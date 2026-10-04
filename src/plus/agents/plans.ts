import type { PlanStep, PlanV1 } from "../ui";
import { clientName, plural } from "./model";

type Data = Record<string, unknown>;

const list = (value: unknown): Data[] => (Array.isArray(value) ? (value as Data[]) : []);
const str = (value: unknown): string => String(value ?? "");
const texts = (value: unknown): string[] => list(value).map(String);
const join = (root: string, file: string) => `${root.replace(/\/$/, "")}/${file}`;

function deletes(paths: string[], what: string): PlanStep[] {
  return paths.map((path) => ({ op: "delete", path, detail: what }));
}

function skippedWarnings(data: Data): string[] {
  return list(data.skipped).map(
    (row) => `${clientName(String(row.client))}: ${String(row.error)}`,
  );
}

/** The dry run (and the apply) of these commands answer in their own shape, not as a `PlanV1`;
 * this words that shape as the plan the confirmation shows. `done` is the past tense. */
export function planOf(command: string, data: Data, done: boolean): PlanV1 | null {
  const warnings = texts(data.discoveryWarnings);
  switch (command) {
    case "agents add":
    case "styles add": {
      const kind = command === "agents add" ? "agent" : "style";
      const name = str(data.name);
      return {
        summary: `${done ? "Created" : "Create"} the ${kind} '${name}' from the template`,
        steps: [{ op: "create", path: str(data.path), detail: `New ${kind} file` }],
        effects: {},
        warnings,
        undo:
          kind === "agent"
            ? `toolportctl agents uninstall ${name}`
            : "toolportctl styles clean",
      };
    }
    case "agents sync": {
      const agents = list(data.agents);
      const steps: PlanStep[] = agents.flatMap((agent) =>
        list(agent.outputFiles).flatMap((out) =>
          texts(out.files).map((file): PlanStep => ({
            op: "create",
            path: join(str(data.outputRoot), file),
            detail: `${str(agent.name)} for ${clientName(String(out.client))}`,
          })),
        ),
      );
      return {
        summary: `${done ? "Wrote" : "Write"} ${plural(agents.length, "agent")} to ${plural(Number(data.clientCount ?? 0), "client")}`,
        steps,
        effects: {},
        warnings: [
          ...warnings,
          ...agents.flatMap((agent) =>
            texts(agent.warnings).map((text) => `${str(agent.name)}: ${text}`),
          ),
        ],
        undo: "toolportctl agents clean",
      };
    }
    case "agents clean": {
      const removed = texts(data.removed);
      return {
        summary: `${done ? "Removed" : "Remove"} ${plural(removed.length, "synced agent file")}`,
        steps: [
          ...deletes(removed, "Synced agent output"),
          { op: "note", detail: "The lockfile and the agent sources are kept" },
        ],
        effects: {},
        warnings: [...warnings, ...skippedWarnings(data)],
        undo: "toolportctl agents sync",
      };
    }
    case "agents uninstall": {
      const outputs = texts(data.outputs);
      return {
        summary: `${done ? "Removed" : "Remove"} the agent '${data.name}' and its ${plural(outputs.length, "output")}`,
        steps: [
          { op: "delete", path: str(data.sourcePath), detail: "Agent source folder" },
          ...deletes(outputs, "Synced agent output"),
          { op: "update", path: str(data.lockDir), detail: "Drop its lockfile entry" },
        ],
        effects: {},
        warnings,
        undo: "",
      };
    }
    case "styles sync": {
      const styles = list(data.styles);
      return {
        summary: `${done ? "Wrote" : "Write"} ${plural(styles.length, "style")} to ${plural(Number(data.clientCount ?? 0), "native client")}`,
        steps: texts(data.outputs).map((path): PlanStep => ({
          op: "create",
          path,
          detail: "Style file",
        })),
        effects: {},
        warnings: [
          ...warnings,
          ...styles.flatMap((style) =>
            texts(style.warnings).map((text) => `${str(style.name)}: ${text}`),
          ),
        ],
        undo: "toolportctl styles clean",
      };
    }
    case "styles apply": {
      const applied = list(data.applied);
      const replaced = list(data.replaced);
      return {
        summary: `${done ? "Applied" : "Apply"} '${data.name}' as an always-on rule in ${plural(applied.length, "client")}`,
        steps: [
          ...applied.map((row): PlanStep => ({
            op: "create",
            path: str(row.path),
            detail: `Rule for ${clientName(String(row.client))}`,
          })),
          ...replaced.map((row): PlanStep => ({
            op: "note",
            detail: `Replaces ${typeof row === "string" ? row : JSON.stringify(row)}`,
          })),
        ],
        effects: {},
        warnings,
        undo: "toolportctl styles remove",
      };
    }
    case "styles remove": {
      const removed = list(data.removed);
      return {
        summary: data.hadActive
          ? `${done ? "Removed" : "Remove"} the active style from ${plural(removed.length, "client")}`
          : "No style is active, nothing to remove",
        steps: removed.map((row): PlanStep => ({
          op: "delete",
          path: str(row.path),
          detail: `'${row.style}' for ${clientName(String(row.client))}`,
        })),
        effects: {},
        warnings,
        undo: removed[0] ? `toolportctl styles apply ${str(removed[0].style)}` : "",
      };
    }
    case "styles clean": {
      const removed = texts(data.removed);
      return {
        summary: `${done ? "Removed" : "Remove"} ${plural(removed.length, "style file")}`,
        steps: deletes(removed, "Synced or applied style file"),
        effects: {},
        warnings: [...warnings, ...skippedWarnings(data)],
        undo: "toolportctl styles sync",
      };
    }
    default:
      return null;
  }
}
