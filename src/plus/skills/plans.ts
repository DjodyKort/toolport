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
    case "skills install": {
      const audit = (data.audit ?? {}) as Data;
      const found = list(audit.findings);
      const rows = list(data.skills);
      const fresh = rows.filter((row) => row.status === "installed");
      const high = num(audit.high);
      const blocked = data.blocked === true;
      return {
        summary: blocked
          ? `Blocked: ${plural(high, "high-severity audit finding")} in ${str(data.spec)}`
          : `${done ? "Installed" : "Install"} ${plural(fresh.length, "skill")} from ${str(data.tap)}`,
        steps: blocked
          ? []
          : [
              ...(data.tapMissing || data.tapAdded
                ? [
                    {
                      op: "create",
                      path: str(data.cloneUrl),
                      detail: `${done ? "Registered" : "Clone and register"} the tap ${str(data.tap)} (needs the network)`,
                    } as PlanStep,
                  ]
                : []),
              ...rows.map((row): PlanStep =>
                row.status === "installed"
                  ? {
                      op: "create",
                      path: str(data.target),
                      detail: `${str(row.name)} (${str(row.type)})`,
                    }
                  : {
                      op: "note",
                      detail: `${str(row.name)} already exists and is kept`,
                    },
              ),
            ],
        effects: {},
        warnings: [
          ...(audit.ran === false
            ? [
                "The audit was skipped: nothing checked these skills for risky instructions",
              ]
            : []),
          ...found.map(
            (f) =>
              `${str(f.severity)}: ${str(f.skill)}: ${str(f.message)}${f.line != null ? ` (line ${str(f.line)})` : ""}`,
          ),
          ...texts(data.discoveryWarnings),
          ...texts(data.symlinksSkipped).map((link) => `Skipped the symlink ${link}`),
        ],
        undo:
          fresh.length === 1 ? `toolportctl skills uninstall ${str(fresh[0].name)}` : "",
      };
    }
    case "skills tap add":
      return {
        summary: `${done ? "Added" : "Add"} the tap '${str(data.name)}' from ${str(data.url)}`,
        steps: [
          {
            op: "create",
            path: str(data.path),
            detail: `${done ? "Cloned" : "Clone"} the repository (needs the network)`,
          },
        ],
        effects: {},
        warnings: [],
        undo: `toolportctl skills tap remove ${str(data.name)}`,
      };
    case "skills tap remove":
      return {
        summary: `${done ? "Removed" : "Remove"} the tap '${str(data.name)}'`,
        steps: [
          data.hadClone
            ? { op: "delete", path: str(data.path), detail: "Delete the local clone" }
            : { op: "note", detail: "The tap has no local clone" },
        ],
        effects: {},
        warnings: [],
        undo: "",
      };
    case "skills tap update": {
      const results = list(data.results);
      return {
        summary:
          results.length === 0
            ? "No taps to update"
            : `${done ? "Updated" : "Update"} ${plural(results.length, "tap")}`,
        steps: results.map((row): PlanStep => ({
          op: "update",
          detail: `${str(row.name)}: pull from its remote (needs the network)`,
        })),
        effects: {},
        warnings: results
          .filter((row) => row.ok === false)
          .map((row) => `${str(row.name)}: ${str(row.error)}`),
        undo: "",
      };
    }
    case "skills bundle": {
      const skills = list(data.skills);
      return {
        summary: `${done ? "Packed" : "Pack"} ${plural(skills.length, "skill")} (${plural(num(data.fileCount), "file")}) into a zip`,
        steps: [
          {
            op: "create",
            path: str(data.output),
            detail: `${num(data.sourceBytes)} bytes of sources${data.bundleBytes != null ? `, ${num(data.bundleBytes)} bytes zipped` : ""}`,
          },
          ...skills.map((row): PlanStep => ({
            op: "note",
            detail: `${str(row.name)} (${str(row.type)}): ${plural(num(row.files), "file")}`,
          })),
        ],
        effects: {},
        warnings: [],
        undo: "",
      };
    }
    case "skills unbundle": {
      const overwritten = texts(data.overwritten);
      const names = texts(data.names);
      return {
        summary: `${done ? "Extracted" : "Extract"} ${plural(names.length, "skill")} from the bundle into ${str(data.target)}`,
        steps: [
          ...texts(data.files).map((file): PlanStep => ({
            op: overwritten.includes(file) ? "update" : "create",
            path: `${str(data.target)}/${file}`,
            detail: overwritten.includes(file)
              ? "Overwrites a file that is already there"
              : "New file",
          })),
          ...overwritten
            .filter((file) => !texts(data.files).includes(file))
            .map((file): PlanStep => ({
              op: "update",
              path: file,
              detail: "Overwrites a file that is already there",
            })),
        ],
        effects: {},
        warnings: [
          ...(overwritten.length > 0
            ? [`${plural(overwritten.length, "file")} will be overwritten`]
            : []),
          ...texts(data.skipped).map((entry) => `Skipped: ${entry}`),
        ],
        undo: "",
      };
    }
    case "skills init": {
      const exists = data.alreadyExists === true;
      return {
        summary: exists
          ? `A skills repository already exists at ${str(data.repo)}`
          : `${done ? "Created" : "Create"} the skills repository '${str(data.name)}' at ${str(data.repo)}`,
        steps: exists
          ? [{ op: "note", path: str(data.repo), detail: "Nothing is overwritten" }]
          : texts(data.created).map((entry): PlanStep => ({
              op: "create",
              path: `${str(data.repo)}/${entry}`,
              detail: entry.endsWith("/") ? "Folder" : "Repository file",
            })),
        effects: {},
        warnings: [],
        undo: "",
      };
    }
    default:
      return null;
  }
}
