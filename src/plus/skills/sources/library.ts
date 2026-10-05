import type {
  LibraryChecks,
  LibraryPullData,
  LibraryPushData,
  LibraryStatusData,
} from "../../types/library";
import type { PlanV1 } from "../../ui";
import { planOf } from "../../ui";
import type { Gate, WriteSpec } from "../hooks";
import { plural } from "../model";
import { plain } from "./model";

const AUTH_NAME: Record<string, string> = {
  gh: "the GitHub CLI",
  "git-credential": "your git credentials",
  ssh: "your SSH key",
};

export const authName = (method: string): string => AUTH_NAME[method] ?? method;

/** What the sign-in row says: the method, and whether anything has tried it yet. */
export function authText(auth: LibraryStatusData["auth"]): string {
  const how = authName(auth.method);
  if (!auth.checked) return `${how}; not tried yet, Check the remote tries them`;
  return auth.ok ? `${how}; the remote accepted them` : `${how}; the remote refused them`;
}

/** The two halves of "where the library stands": what is not committed, what is not pushed. */
export function changesText(status: LibraryStatusData): string {
  const parts = [
    status.uncommitted > 0 ? `${plural(status.uncommitted, "file")} changed` : "",
    status.ahead > 0 ? `${plural(status.ahead, "commit")} not pushed` : "",
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(", ") : "Nothing changed here";
}

export function behindText(status: LibraryStatusData): string {
  const ref = status.upstream ?? "the remote";
  if (status.behind === 0 && status.ahead === 0) return `Level with ${ref}`;
  return [
    `${plural(status.behind, "commit")} behind`,
    `${plural(status.ahead, "commit")} ahead`,
  ].join(", ");
}

export const remoteText = (status: LibraryStatusData): string =>
  status.remote === null ? "No remote" : plain(status.remote);

/** Why Pull and Push cannot start, or null when they can. */
export function reasonOff(
  status: LibraryStatusData | null,
  loading: boolean,
): string | null {
  if (status === null)
    return loading
      ? "The library status is still being read."
      : "The library status could not be read, so Pull and Push stay off.";
  if (status.remote === null)
    return "This library has no remote. Add one in its folder with git, and Pull and Push turn on.";
  return null;
}

const findingLine = (rule: string, file: string, line: number, commit?: string): string =>
  `${rule} in ${file}:${line}${commit ? ` (commit ${commit})` : ""}`;

/** Rule, file and line of every finding, never the text that matched. */
export function findingLines(checks: LibraryChecks): string[] {
  const all = [...checks.gitleaks.findings, ...checks.builtinScan.findings];
  return [...new Set(all.map((f) => findingLine(f.rule, f.file, f.line, f.commit)))];
}

const NOTHING = /^(already up to date|nothing to push)/i;

function checksOf(raw: unknown): LibraryChecks | null {
  const checks = (raw as { checks?: LibraryChecks } | null)?.checks;
  return checks && typeof checks === "object" ? checks : null;
}

function summaryOf(raw: unknown): string {
  return planOf(raw)?.summary ?? "";
}

/** A dry run that has nothing to do is a note, not a confirmation to press. */
function pullGate(raw: unknown): Gate | null {
  return NOTHING.test(summaryOf(raw))
    ? {
        reason:
          "Already up to date: there is nothing on the remote that this library lacks.",
        info: true,
      }
    : null;
}

/** A secret finding blocks the push for good; the dialog lists it and offers no apply. */
function pushGate(raw: unknown): Gate | null {
  const checks = checksOf(raw);
  if (checks?.blocked) {
    return {
      reason:
        "The secret scan found something that looks like a secret in what would be pushed. Nothing was pushed and nothing is sent. Take it out of the files and out of the commits that added it, then try again.",
      lines: findingLines(checks),
    };
  }
  return NOTHING.test(summaryOf(raw))
    ? { reason: "Nothing to push: this library already matches the remote.", info: true }
    : null;
}

function auditWarnings(checks: LibraryChecks | null): string[] {
  if (!checks) return [];
  const { audit } = checks;
  if (audit.error) return [`The skills audit could not run: ${audit.error}`];
  return audit.ran && (audit.high ?? 0) > 0
    ? [
        `${plural(audit.high ?? 0, "high-severity audit finding")} in the library. This does not stop the push; read them with skills audit first.`,
      ]
    : [];
}

function resultPlan(
  summary: string,
  data: { result?: { changed: string[]; undo: string } },
  steps: PlanV1["steps"],
): PlanV1 {
  return {
    summary,
    steps: [
      ...steps,
      ...(data.result?.changed ?? []).map((path): PlanV1["steps"][number] => ({
        op: "update",
        path,
        detail: "Updated",
      })),
    ],
    effects: {},
    warnings: [],
    undo: data.result?.undo ?? "",
  };
}

function pullView(raw: unknown, done: boolean): unknown {
  const data = raw as LibraryPullData;
  if (!done) return null;
  const plan = resultPlan(
    data.pulled && data.commits
      ? `Pulled ${plural(data.commits, "commit")}`
      : "Already up to date",
    data,
    [],
  );
  return { plan };
}

function pushView(raw: unknown, done: boolean): unknown {
  const data = raw as LibraryPushData;
  if (done) {
    return {
      plan: resultPlan(
        data.pushed ? "Pushed to the remote" : (data.message ?? "Nothing was pushed"),
        data,
        [
          ...(data.commitSha
            ? [{ op: "note" as const, detail: `commit ${data.commitSha}` }]
            : []),
          ...(data.pushed
            ? [{ op: "exec" as const, detail: "git push (never forced)" }]
            : []),
        ],
      ),
    };
  }
  const plan = planOf(raw);
  const extra = auditWarnings(checksOf(raw));
  return plan && extra.length > 0
    ? { plan: { ...plan, warnings: [...plan.warnings, ...extra] } }
    : null;
}

export const PULL: WriteSpec = {
  command: "library pull",
  title: "Pull from the remote",
  argv: ["library", "pull"],
  confirmLabel: "Pull",
  phrase: "pull",
  gate: pullGate,
  view: pullView,
};

export const PUSH: WriteSpec = {
  command: "library push",
  title: "Push the library",
  argv: ["library", "push"],
  confirmLabel: "Push",
  phrase: "push",
  gate: pushGate,
  view: pushView,
};
