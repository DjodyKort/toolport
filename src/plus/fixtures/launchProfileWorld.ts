import type { CtlReply } from "./ctlWorld";
import type { WorldSet } from "./serverWorld";
import type {
  FolderWarning,
  LaunchProfileRow,
  LaunchProfileState,
  LaunchProfileWriteData,
  Plan,
} from "../context/data";

export interface LaunchProfileSeed {
  name: string;
  /** An access set id. */
  set: string;
  denyDirect?: boolean;
  state?: LaunchProfileState;
}

export interface LaunchProfileWorldOptions {
  profiles?: LaunchProfileSeed[];
  /** Folder mappings that outrank a profile's set: the folder and the set it maps to. */
  folders?: Array<{ path: string; set: string }>;
  /** Servers the access set the profile uses leaves out, by entry name. */
  directLeftOut?: string[];
}

const HOME = "/Users/example";
const RULE =
  "A folder mapping outranks a client's own access set: inside a mapped folder, and below it, the gateway uses the folder's set for every client. Order: folder mapping, the client's own set, TOOLPORT_PROFILE, Default access.";

const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

/** `context launch-profile` of the in-memory toolportctl. */
export function createLaunchProfileWorld(
  options: LaunchProfileWorldOptions,
  sets: () => WorldSet[],
) {
  const profiles = new Map<string, LaunchProfileSeed>(
    (options.profiles ?? []).map((seed) => [seed.name, seed]),
  );

  const file = (name: string) => `${HOME}/.config/toolport/profiles/${name}/mcp.json`;
  const setOf = (reference: string) =>
    sets().find((set) => set.id === reference || set.name.toLowerCase() === reference);

  function warnings(set: WorldSet | undefined, cwd: string | null): FolderWarning[] {
    return (options.folders ?? [])
      .filter((folder) => folder.set !== set?.id)
      .filter(
        (folder) =>
          cwd === null || cwd === folder.path || cwd.startsWith(`${folder.path}/`),
      )
      .map((folder) => ({
        folder: folder.path,
        text: `In ${folder.path} the gateway uses ${setOf(folder.set)?.name ?? folder.set}, not launch profile's own set. ${RULE}`,
        servers: [],
        rule: RULE,
      }));
  }

  function row(seed: LaunchProfileSeed, cwd: string | null = null): LaunchProfileRow {
    const set = setOf(seed.set);
    const state = seed.state ?? (set ? "ok" : "set-missing");
    return {
      name: seed.name,
      clientId: `claude-code-${seed.name}`,
      accessSet: set ? { id: set.id, name: set.name } : null,
      accessSetRef: seed.set,
      denyDirect: seed.denyDirect ?? false,
      discovery: null,
      state,
      problems: state === "ok" ? [] : [`${seed.name} needs repair (${state})`],
      mcpFile: file(seed.name),
      command: `claude --strict-mcp-config --mcp-config ${file(seed.name)}`,
      policyFolders: [],
      folderWarnings: warnings(set, cwd),
    };
  }

  const plan = (
    summary: string,
    detail: string,
    undo: string,
    extra: string[] = [],
  ): Plan => ({
    summary,
    steps: [{ op: "create", path: file("x"), detail }],
    effects: {},
    warnings: extra,
    undo,
  });

  function answer(
    name: string,
    dryRun: boolean,
    summary: string,
    detail: string,
    undo: string,
    extra: Partial<LaunchProfileWriteData> = {},
  ): CtlReply {
    const data: LaunchProfileWriteData = {
      dryRun,
      name,
      clientId: `claude-code-${name}`,
      plan: plan(summary, detail, undo),
      result: dryRun ? null : { applied: true, changed: [file(name)], undo, backups: [] },
      ...extra,
    };
    return { ok: true, data };
  }

  function flag(argv: string[], name: string) {
    const at = argv.indexOf(name);
    return at >= 0 ? (argv[at + 1] ?? null) : null;
  }

  return {
    profiles,
    run(argv: string[]): CtlReply {
      const verb = argv[2];
      const operand = argv[3] && !argv[3].startsWith("--") ? argv[3] : null;
      const dryRun = argv.includes("--dry-run");
      if (verb === "ls") {
        const cwd = flag(argv, "--cwd");
        const rows = [...profiles.values()]
          .filter((seed) => operand === null || seed.name === operand)
          .map((seed) => row(seed, cwd));
        if (operand && rows.length === 0) {
          return fail("not_found", `no launch profile called ${operand}`);
        }
        return { ok: true, data: { profiles: rows, base: "claude-code" } };
      }
      if (!operand) return fail("usage", `usage: context launch-profile ${verb} <name>`);
      if (verb === "add") {
        const existing = profiles.get(operand);
        const reference = flag(argv, "--set") ?? existing?.set ?? null;
        if (!reference) return fail("usage", "--set <access set> is required");
        const set = setOf(reference.toLowerCase());
        if (!set) {
          return fail(
            "not_found",
            `no access set called ${reference}; the access sets are: ${sets()
              .map((s) => s.name)
              .join(", ")}`,
          );
        }
        const deny = flag(argv, "--deny-direct");
        const seed: LaunchProfileSeed = {
          name: operand,
          set: set.id,
          denyDirect: deny === null ? (existing?.denyDirect ?? false) : deny === "on",
        };
        if (!dryRun) profiles.set(operand, seed);
        const undo = existing
          ? `toolportctl context launch-profile add ${operand} --set ${existing.set}`
          : `toolportctl context launch-profile rm ${operand}`;
        return answer(
          operand,
          dryRun,
          `${existing ? "Update" : "Add"} launch profile ${operand}: Claude Code started with claude-code-${operand} sees the access set ${set.name}`,
          `bind claude-code-${operand} to the access set ${set.name}`,
          undo,
          {
            created: !existing,
            profile: dryRun ? null : row(seed),
            folderWarnings: warnings(set, null),
          },
        );
      }
      if (verb === "rm") {
        const existing = profiles.get(operand);
        if (!existing) return fail("not_found", `no launch profile called ${operand}`);
        if (!dryRun) profiles.delete(operand);
        return answer(
          operand,
          dryRun,
          `Remove launch profile ${operand}`,
          "remove the binding, the generated mcp.json and the record",
          `toolportctl context launch-profile add ${operand} --set ${existing.set}`,
        );
      }
      if (verb === "policy") {
        const existing = profiles.get(operand);
        if (!existing) return fail("not_found", `no launch profile called ${operand}`);
        const cwd = flag(argv, "--cwd") ?? "";
        const keys = (options.directLeftOut ?? []).map((serverName) => ({ serverName }));
        return answer(
          operand,
          dryRun,
          `Keep ${keys.length} direct entries out of ${cwd}`,
          "deniedMcpServers in .claude/settings.local.json",
          `toolportctl context launch-profile add ${operand} --deny-direct off`,
          { keys, cwd, denyDirect: existing.denyDirect },
        );
      }
      return fail("usage", `unknown launch-profile command ${verb}`);
    },
  };
}
