import { describe, expect, it } from "vitest";
import { updateWorld } from "./fixtures";
import {
  canUpdate,
  checkLabel,
  clientSnippet,
  filterTools,
  itemText,
  lines,
  nameMapOf,
  planOfRenameRefs,
  planOfSyncPull,
  planOfSyncPush,
  planOfUpdate,
  policyOf,
  postUpdateOf,
  profileStates,
  refRows,
  toolEntries,
  updateReport,
} from "./model";
import { commandsData } from "./fixtures";
import { golden } from "./testkit";
import type { CommandRow } from "../bridge/data";

const rows = (commandsData as { commands: CommandRow[] }).commands;

describe("policy of the System commands, from the real registry", () => {
  it.each([
    ["sync push", "destructive", "--dry-run"],
    ["sync pull", "write", "--dry-run"],
    ["sync reset", "destructive", null],
    ["sync rotate-passphrase", "destructive", null],
    ["sync init", "write", null],
    ["update", "write", "--dry-run"],
    ["council uninstall", "destructive", null],
    ["council install", "write", null],
    ["mcp uninstall", "destructive", null],
    ["mcp install", "write", null],
    ["import mcpm", "write", "--dry-run"],
    ["import rename-refs", "write", "--dry-run"],
  ])("%s is %s with preview flag %s", (id, tier, flag) => {
    expect(policyOf(rows, id)).toEqual({ tier, previewFlag: flag, terminal: false });
  });

  it("has no policy for a command the registry does not know or has not loaded", () => {
    expect(policyOf(rows, "sync bogus")).toBeNull();
    expect(policyOf(null, "sync push")).toBeNull();
  });
});

describe("plans from the golden envelopes", () => {
  it("words a sync push dry run and its apply", () => {
    const preview = planOfSyncPush(golden("sync-push.preview"), false);
    expect(preview.summary).toBe("Push 1 file from m-test");
    expect(preview.steps[0]).toMatchObject({
      op: "update",
      path: "projects/proj1/a.txt",
    });
    expect(preview.steps.at(-1)?.detail).toMatch(/never in the bundle/);
    expect(planOfSyncPush(golden("sync-push.apply"), true).summary).toBe(
      "Pushed 1 file from m-test",
    );
  });

  it("warns when a push committed but did not reach the remote", () => {
    const plan = planOfSyncPush({ ...golden("sync-push.apply"), pushed: false }, true);
    expect(plan.warnings).toEqual([
      "Committed in the sync repository but not pushed to the remote",
    ]);
  });

  it("words a pull as new, changed, removed files and conflicts", () => {
    const plan = planOfSyncPull(
      {
        ...golden("sync-pull.preview"),
        changes: {
          new: ["a"],
          modified: ["b"],
          removed: ["c"],
          conflicts: ["d"],
          unchanged: [],
        },
      },
      false,
    );
    expect(plan.steps.map((step) => step.op)).toEqual(["create", "update", "delete"]);
    expect(plan.summary).toBe("Pull 3 files from m-test");
    expect(plan.warnings).toEqual(["Conflict: d"]);
  });

  it("says a pull of an identical machine has nothing to do", () => {
    expect(planOfSyncPull(golden("sync-pull.preview"), false).summary).toBe(
      "Nothing to pull: this machine matches the bundle",
    );
  });

  it("reads the update servers with their plan lines and finds the update command", () => {
    const report = updateReport(updateWorld);
    expect(report.servers.map((server) => server.status)).toEqual([
      "update-available",
      "up-to-date",
      "auto",
      "skipped",
    ]);
    expect(postUpdateOf(report.servers[0])).toEqual({
      command: "./build.sh",
      held: true,
    });
    expect(postUpdateOf(report.servers[1])).toBeNull();
    expect(report.servers.filter(canUpdate)).toHaveLength(1);
    expect(
      postUpdateOf({ ...report.servers[0], plan: ["post_update: ./build.sh"] }),
    ).toEqual({ command: "./build.sh", held: false });
  });

  it("words an update report as a plan, and the golden skipped servers as notes", () => {
    const plan = planOfUpdate(updateWorld, false);
    expect(plan.summary).toBe("Update 1 server");
    expect(plan.steps.find((step) => step.op === "exec")?.detail).toBe(
      "Update command: ./build.sh (not run without --allow-commands)",
    );
    const golden2 = planOfUpdate(golden("update.preview"), false);
    expect(golden2.summary).toBe("Nothing to change");
    expect(golden2.steps.every((step) => step.op === "note")).toBe(true);
  });

  it("turns an error status of an update into a warning", () => {
    const plan = planOfUpdate(
      { servers: [{ id: "s", kind: "git", status: "error", message: "diverged" }] },
      true,
    );
    expect(plan.warnings).toEqual(["s: diverged"]);
  });

  it("makes ask and deny orphans blocking warnings and keeps the others as notes", () => {
    const plan = planOfRenameRefs(
      {
        ...golden("import-rename-refs.preview"),
        orphans: [
          { path: "/a", reference: "x*", reason: "wildcard", rule: "ask" },
          { path: "/b", reference: "y", reason: "unknown tool" },
        ],
      },
      false,
    );
    expect(plan.warnings).toEqual(["x* is in a ask rule and was not renamed (/a)"]);
    expect(
      plan.steps.filter((step) => step.detail.startsWith("Left as it is")),
    ).toHaveLength(2);
    expect(refRows(null)).toEqual([]);
  });

  it("reads the name map of the importer", () => {
    const map = nameMapOf(golden("import-mcpm.name-map"));
    expect(map.count).toBe(3);
    expect(map.map[0]).toEqual([
      "mcp__mcpm_alpha-mock__add",
      "mcp__toolport__alpha_mock__add",
    ]);
  });
});

describe("small readers", () => {
  it("names an entry of an untyped list by its path or name", () => {
    expect(itemText("a/b")).toBe("a/b");
    expect(itemText({ path: "p", x: 1 })).toBe("p");
    expect(itemText({ name: "n" })).toBe("n");
    expect(itemText({ x: 1 })).toBe('{"x":1}');
  });

  it("splits lines and drops blanks", () => {
    expect(lines(" a \n\n b\r\nc ")).toEqual(["a", "b", "c"]);
  });

  it("filters tools by text and tier and reads the gate", () => {
    const tools = toolEntries(golden("mcp-tools").tools);
    expect(tools).toHaveLength(100);
    expect(filterTools(tools, "SKILLS_LIST", null).map((t) => t.name)).toContain(
      "skills_list",
    );
    expect(filterTools(tools, "", 4).every((t) => t.tier === 4)).toBe(true);
    expect(tools.find((t) => t.name === "skills_list")?.gate).toBe("none");
  });

  it("reads profile states from a string, an object or a list", () => {
    expect(profileStates(null)).toEqual([]);
    expect(profileStates("default")).toEqual([
      { id: "default", enabled: true, optedOut: false },
    ]);
    expect(profileStates({ id: "w", enabled: false, optedOut: true })).toEqual([
      { id: "w", enabled: false, optedOut: true },
    ]);
  });

  it("labels the doctor checks and builds the client snippet", () => {
    expect(checkLabel("key_in_vault")).toBe("API key in the vault");
    expect(checkLabel("some_new_check")).toBe("Some new check");
    expect(JSON.parse(clientSnippet("/bin/self"))).toEqual({
      mcpServers: { "toolport-plus-self": { command: "/bin/self" } },
    });
  });
});
