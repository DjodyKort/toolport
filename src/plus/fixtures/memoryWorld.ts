import { FIXTURE_FOLDER } from "./contextWorld";
import type {
  MemoryFile,
  OverviewRow,
  PlanData,
  SessionRow,
  SwitchPosition,
} from "../memory/data";

interface Reply {
  ok: boolean;
  data?: unknown;
  error?: { code: string; message: string };
}

export interface MemoryWorldOptions {
  /** Starts the main repository with auto memory switched off by hand. */
  off?: boolean;
}

const OTHER = "/Users/example/work/other";
const RETENTION = {
  days: 30,
  default: 30,
  file: "/Users/example/.claude/settings.json",
  min: 1,
  perRepository: false,
  source: "default",
};

const file = (name: string, bytes: number, index = false): MemoryFile => ({
  bytes,
  description: index ? null : `about ${name}`,
  index,
  modified: "2026-11-10T09:00:00Z",
  name,
  path: `/Users/example/.claude/projects/web-app/memory/${name}`,
  type: index ? null : "feedback",
});

const session = (id: string, ageDays: number): SessionRow => ({
  ageDays,
  bytes: 1000 * ageDays,
  daysLeft: 30 - ageDays,
  expiring: 30 - ageDays <= 7,
  folder: FIXTURE_FOLDER,
  id,
  modified: "2026-11-01T09:00:00Z",
});

const plan = (summary: string, detail: string, undo: string | null): PlanData => ({
  effects: {},
  steps: [{ detail, op: "move", path: "/Users/example/.claude/projects/web-app" }],
  summary,
  undo,
  warnings: ["Undo from the trash works for 30 days"],
});

/** The `memory` and `sessions` commands over one repository with auto memory, instruction files
 * and sessions, plus a second repository whose memory is off. */
export function createMemoryWorld(options: MemoryWorldOptions = {}) {
  let position: SwitchPosition = options.off ? "off" : "default";
  let globalOn: boolean | null = null;
  let files = [
    file("MEMORY.md", 90, true),
    file("feedback_testing.md", 80),
    file("user_role.md", 70),
  ];
  let sessions = [
    session("sess-new", 2),
    session("sess-mid", 12),
    session("sess-old", 27),
  ];
  const retentionDays = 30;
  const trash: Array<{
    id: string;
    kind: "memory" | "sessions";
    files?: MemoryFile[];
    sessions?: SessionRow[];
    position?: SwitchPosition;
  }> = [];
  const archive: Array<{ id: string; sessions: SessionRow[] }> = [];
  let next = 1;

  const enabled = () =>
    position === "on" ? true : position === "off" ? false : (globalOn ?? true);
  const totalBytes = (rows: Array<{ bytes: number }>) =>
    rows.reduce((n, r) => n + r.bytes, 0);
  const totals = () => ({
    bytes: totalBytes(sessions),
    count: sessions.length,
    expiringThisWeek: sessions.filter((s) => s.expiring).length,
    oldest: sessions.length ? "2026-10-20T09:00:00Z" : null,
  });
  const retention = () => ({ ...RETENTION, days: retentionDays });
  const state = () => ({
    enabled: enabled(),
    environment: false,
    file: "/Users/example/web-app/.claude/settings.local.json",
    global: globalOn,
    locked: false,
    managed: false,
    project: position === "default" ? null : position === "on",
    setBy: position === "default" ? "default" : "repository",
    setByHand: false,
    switch: position,
    toolportOwns: position !== "default",
  });
  const fail = (code: string, message: string): Reply => ({
    ok: false,
    error: { code, message },
  });

  function status(cwd: string): Reply {
    return {
      ok: true,
      data: {
        cwd,
        directory: {
          custom: false,
          exists: true,
          path: "/Users/example/.claude/projects/web-app/memory",
        },
        instructions: {
          basis: "estimate",
          excluded: 0,
          imports: 0,
          leftOutTokens: 0,
          localFiles: 0,
          memoryIndexTokens: 30,
          rows: [
            {
              bytes: 400,
              canExclude: true,
              kind: "claude-md",
              lazy: false,
              loaded: true,
              name: "CLAUDE.md",
              note: null,
              path: `${FIXTURE_FOLDER}/CLAUDE.md`,
              scope: "project",
              source: "project",
              state: "loaded",
              tokens: 100,
            },
            {
              bytes: 300,
              canExclude: false,
              kind: "claude-md",
              lazy: false,
              loaded: true,
              name: "CLAUDE.md",
              note: "Managed by your organization; Claude Code has no setting to leave it out",
              path: "/Library/Application Support/ClaudeCode/CLAUDE.md",
              scope: "managed",
              source: "managed",
              state: "loaded",
              tokens: 75,
            },
          ],
          totalTokens: 205,
        },
        memory: {
          bytes: totalBytes(files),
          count: files.length,
          lastWritten: files.length ? "2026-11-10T09:00:00Z" : null,
          topicFiles: Math.max(files.length - 1, 0),
        },
        notes: [],
        repository: {
          git: true,
          key: "web-app",
          linked: false,
          name: "web-app",
          root: FIXTURE_FOLDER,
          worktrees: [{ current: true, exists: true, path: FIXTURE_FOLDER }],
        },
        sessions: { retention: retention(), totals: totals() },
        state: state(),
        trash: trash.map((t) => ({ id: t.id, kind: t.kind })),
      },
    };
  }

  function overview(args: string[]): Reply {
    const at = args.indexOf("--filter");
    const filter = at >= 0 ? args[at + 1] : null;
    const row = (
      name: string,
      root: string,
      on: boolean,
      count: number,
    ): OverviewRow => ({
      instructions: { excluded: 0, files: 2, flags: [], localFiles: 0, tokens: 205 },
      key: name,
      lastActive: "2026-11-10T09:00:00Z",
      memory: {
        bytes: 240,
        enabled: on,
        files: 3,
        lastWritten: "2026-11-10T09:00:00Z",
        locked: false,
        setBy: on ? "default" : "repository",
        switch: on ? "default" : "off",
      },
      name,
      root,
      sessions: {
        bytes: 1000 * count,
        count,
        expiringThisWeek: count > 2 ? 1 : 0,
        oldest: "2026-10-20T09:00:00Z",
      },
      worktrees: 0,
    });
    let rows = [
      row("web-app", FIXTURE_FOLDER, enabled(), sessions.length),
      row("other", OTHER, false, 0),
    ];
    if (filter === "memory-off") rows = rows.filter((r) => !r.memory.enabled);
    if (filter === "expiring") rows = rows.filter((r) => r.sessions.expiringThisWeek > 0);
    return {
      ok: true,
      data: {
        filter,
        filters: ["memory-off", "has-local-file", "expiring", "large"],
        global: { enabled: globalOn, file: RETENTION.file },
        other: { folders: 1, memoryBytes: 0, sessionBytes: 3, sessions: 1 },
        retention: retention(),
        rows,
        totals: {
          asOf: "2026-11-17T09:00:00Z",
          bytes: 0,
          memoryBytes: rows.reduce((n, r) => n + r.memory.bytes, 0),
          memoryFiles: rows.reduce((n, r) => n + r.memory.files, 0),
          repositories: rows.length,
          sessionBytes: rows.reduce((n, r) => n + r.sessions.bytes, 0),
          sessions: rows.reduce((n, r) => n + r.sessions.count, 0),
        },
      },
    };
  }

  return {
    get position() {
      return position;
    },
    get fileCount() {
      return files.length;
    },
    get sessionCount() {
      return sessions.length;
    },
    get archived() {
      return archive.length;
    },
    run(key: string[]): Reply {
      const [group, verb, ...rest] = key;
      const flag = (name: string) => {
        const at = rest.indexOf(name);
        return at >= 0 ? rest[at + 1] : undefined;
      };
      const dryRun = rest.includes("--dry-run");
      const cwd = flag("--cwd") ?? FIXTURE_FOLDER;

      if (group === "memory") {
        if (verb === "status") return status(cwd);
        if (verb === "overview") return overview(rest);
        if (verb === "ls") {
          return {
            ok: true,
            data: {
              cwd,
              directory: "/Users/example/.claude/projects/web-app/memory",
              files,
              sharedBy: [],
            },
          };
        }
        if (verb === "set") {
          const to = rest[0] as SwitchPosition;
          const previous = position;
          const summary = `Set auto memory to ${to} for web-app`;
          if (!dryRun) {
            if (rest.includes("--global"))
              globalOn = to === "default" ? null : to === "on";
            else position = to;
          }
          return {
            ok: true,
            data: {
              dryRun,
              name: "web-app",
              plan: plan(
                summary,
                "write autoMemoryEnabled",
                `toolportctl memory set ${previous}`,
              ),
              position: to,
              previous,
              state: { enabled: enabled(), setBy: "repository", switch: position },
            },
          };
        }
        if (verb === "clear") {
          const forever = rest.includes("--forever");
          if (forever && flag("--confirm") !== "web-app") {
            return fail("confirm", "type the repository name to delete for good");
          }
          const cleared = files;
          const id = `m${next++}`;
          if (!dryRun) {
            trash.push({ id, kind: "memory", files: cleared, position });
            files = [];
            if (rest.includes("--also-off")) position = "off";
          }
          return {
            ok: true,
            data: {
              alsoOff: rest.includes("--also-off"),
              dryRun,
              files: cleared,
              forever,
              name: "web-app",
              plan: plan(
                "Clear the auto memory of web-app",
                forever ? "delete for good" : "move to Toolport's trash; kept 30 days",
                forever ? null : "toolportctl memory undo",
              ),
              result: dryRun ? null : { applied: true, trashId: id, undo: "memory undo" },
              sharedBy: [],
              totals: { bytes: totalBytes(cleared), files: cleared.length },
            },
          };
        }
        if (verb === "undo") {
          const at = trash.map((t) => t.kind).lastIndexOf("memory");
          if (at < 0) return fail("nothing", "nothing to undo");
          if (!dryRun) {
            const [entry] = trash.splice(at, 1);
            files = entry.files ?? [];
            position = entry.position ?? position;
          }
          return {
            ok: true,
            data: {
              dryRun,
              plan: plan("Restore the auto memory", "move back", null),
              result: dryRun ? null : { applied: true },
            },
          };
        }
      }

      if (group === "sessions") {
        if (verb === "ls") {
          return {
            ok: true,
            data: {
              archive: archive.map((a) => ({ id: a.id })),
              name: "web-app",
              note: null,
              retention: retention(),
              sessions,
              totals: totals(),
              trash: trash
                .filter((t) => t.kind === "sessions")
                .map((t) => ({ id: t.id })),
            },
          };
        }
        if (verb === "retention") {
          return { ok: true, data: { note: null, retention: retention() } };
        }
        if (verb === "clear") {
          const days = Number(flag("--older-than") ?? 0);
          const to = flag("--to") === "archive" ? "archive" : "trash";
          const chosen = sessions.filter((s) => s.ageDays >= days);
          const id = `s${next++}`;
          if (!dryRun && chosen.length) {
            sessions = sessions.filter((s) => !chosen.includes(s));
            if (to === "archive") archive.push({ id, sessions: chosen });
            else trash.push({ id, kind: "sessions", sessions: chosen });
          }
          return {
            ok: true,
            data: {
              dryRun,
              name: "web-app",
              plan: plan(
                `Clear ${chosen.length} session(s) of web-app`,
                `${chosen.length} session(s), to ${to}`,
                "toolportctl sessions undo",
              ),
              result: dryRun ? null : { applied: true, id, undo: "sessions undo" },
              sessions: chosen,
              to,
              totalBytes: totalBytes(chosen),
            },
          };
        }
        if (verb === "undo") {
          const at = trash.map((t) => t.kind).lastIndexOf("sessions");
          if (at < 0) return fail("nothing", "nothing to undo");
          if (!dryRun)
            sessions = [...sessions, ...(trash.splice(at, 1)[0].sessions ?? [])];
          return {
            ok: true,
            data: {
              dryRun,
              plan: plan("Put the sessions back", "move back", null),
              result: dryRun ? null : { applied: true },
            },
          };
        }
        if (verb === "restore") {
          const at = archive.findIndex((a) => a.id === rest[0]);
          if (at < 0) return fail("unknown", `no archive ${rest[0]}`);
          if (!dryRun) sessions = [...sessions, ...archive.splice(at, 1)[0].sessions];
          return {
            ok: true,
            data: {
              dryRun,
              plan: plan("Restore the archive", "move back", null),
              result: dryRun ? null : { applied: true },
            },
          };
        }
      }
      throw new Error(`Unimplemented fixture command: plus_ctl ${key.join(" ")}`);
    },
  };
}

export type MemoryWorld = ReturnType<typeof createMemoryWorld>;
