import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createLibraryWorld, type LibraryWorldOptions } from "./libraryWorld";

const goldens = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

function golden(name: string): Record<string, unknown> {
  const file = JSON.parse(readFileSync(join(goldens, `${name}.json`), "utf8"));
  return file.envelope.data ?? {};
}

function reply(argv: string[], options: LibraryWorldOptions = {}) {
  const out = createLibraryWorld(options).run(argv);
  return (out.data ?? {}) as Record<string, unknown>;
}

function at(value: unknown, path: string): unknown {
  return path
    .split(".")
    .reduce<unknown>(
      (node, key) =>
        Array.isArray(node)
          ? (node[0] as Record<string, unknown>)?.[key]
          : (node as Record<string, unknown> | undefined)?.[key],
      value,
    );
}

/** The keys the Library screen reads of each answer, with the golden of the Rust side that has
 * them too. */
const READS: Array<{
  golden: string;
  argv: string[];
  options?: LibraryWorldOptions;
  keys: string[];
  /** Keys the golden has no row for (an empty list in the golden). */
  worldOnly?: string[];
}> = [
  {
    golden: "skills-sync.preview",
    argv: ["skills", "sync", "--dry-run"],
    keys: [
      "dryRun",
      "plan.counts.add",
      "plan.counts.hook",
      "plan.changes.skill",
      "plan.changes.client",
      "plan.changes.path",
      "plan.changes.op",
      "plan.changes.held",
      "held",
      "entries.name",
      "entries.type",
      "entries.clientsSynced",
      "targetedClients",
      "outputRoot",
      "repo",
      "skillCount",
      "ruleCount",
      "collisions",
      "snapshot",
      "verify",
    ],
  },
  {
    golden: "skills-sync.apply",
    argv: ["skills", "sync"],
    keys: [
      "snapshot.id",
      "snapshot.files",
      "snapshot.hooks",
      "verify.clean",
      "verify.matched",
      "verify.total",
      "verify.mismatched",
    ],
  },
  {
    golden: "skills-undo.preview",
    argv: ["skills", "undo", "--dry-run"],
    options: { scenario: "done" },
    keys: [
      "files.action",
      "files.path",
      "removed",
      "restored",
      "conflicts",
      "snapshot.id",
      "autoSync.willPause",
    ],
  },
  {
    golden: "skills-verify.clean",
    argv: ["skills", "verify"],
    keys: ["clean", "matched", "total", "mismatched"],
  },
  {
    golden: "skills-auto-status.off",
    argv: ["skills", "auto", "status"],
    keys: [
      "paused",
      "reason",
      "since",
      "lastRun",
      "schedule.task",
      "schedule.present",
      "schedule.enabled",
      "schedule.cron",
      "schedule.when",
      "schedule.autoRun",
      "schedule.nextRun",
      "schedule.lastRun",
    ],
  },
  {
    golden: "skills-auto-status.last_run",
    argv: ["skills", "auto", "status"],
    options: { auto: "on" },
    keys: [
      "lastRun.at",
      "lastRun.applied",
      "lastRun.held",
      "lastRun.skipped",
      "schedule.enabled",
      "schedule.nextRun",
    ],
  },
  {
    golden: "skills-auto-on.apply",
    argv: ["skills", "auto", "on"],
    keys: [
      "dryRun",
      "task",
      "enabled",
      "plan.summary",
      "plan.undo",
      "result.applied",
      "result.undo",
      "schedule.enabled",
      "schedule.nextRun",
    ],
  },
  {
    golden: "skills-auto-off.apply",
    argv: ["skills", "auto", "off"],
    options: { auto: "on" },
    keys: ["enabled", "schedule.enabled", "result.undo"],
  },
  {
    golden: "skills-status",
    argv: ["skills", "status"],
    keys: [
      "entries.name",
      "entries.type",
      "entries.clientsSynced",
      "entries.knownToLockfile",
      "lockfilePresent",
      "lockfileSyncedAt",
      "targetedClients",
      "outputs",
    ],
    worldOnly: ["outputs.name", "outputs.client", "outputs.present"],
  },
  {
    golden: "skills-lint.checks",
    argv: ["skills", "lint"],
    keys: [
      "errors",
      "warnings",
      "limits.description",
      "limits.specDescription",
      "consequences",
      "messages.code",
      "messages.consequence",
      "messages.level",
      "messages.message",
      "messages.name",
    ],
  },
  {
    golden: "skills-audit",
    argv: ["skills", "audit"],
    keys: ["clean", "findings", "high", "skillCount"],
  },
  {
    golden: "skills-files.checks",
    argv: ["skills", "files", "spec-writer"],
    keys: [
      "skill",
      "type",
      "source",
      "counts",
      "denyList",
      "description.length",
      "description.limit",
      "description.specLimit",
      "description.description",
      "description.whenToUse",
      "files.path",
      "files.status",
      "files.size",
      "files.op",
      "files.reason",
      "files.client",
      "installed.dir",
      "installed.present",
      "links.line",
      "links.state",
      "links.target",
    ],
  },
  {
    golden: "skills-ls.library",
    argv: ["skills", "ls", "--source", "library"],
    keys: [
      "repo",
      "skills.name",
      "skills.type",
      "skills.description",
      "skills.visible",
      "skills.origin.kind",
      "skills.path",
    ],
  },
  {
    golden: "skills-fix.preview",
    argv: ["skills", "fix", "chmod", "--dry-run"],
    options: { chmod: true },
    keys: ["count", "fixed.skill", "fixed.file", "fixed.from", "fixed.to"],
  },
  {
    golden: "library-status.behind",
    argv: ["library", "status"],
    options: { scenario: "behind" },
    keys: [
      "ahead",
      "behind",
      "branch",
      "remote",
      "repo",
      "uncommitted",
      "lastFetch",
      "fetch.requested",
      "fetch.ok",
      "fetch.error",
    ],
  },
  {
    golden: "library-pull.preview",
    argv: ["library", "pull", "--dry-run"],
    options: { scenario: "behind" },
    keys: [
      "plan.summary",
      "plan.steps.detail",
      "plan.steps.op",
      "plan.undo",
      "plan.warnings",
    ],
  },
  {
    golden: "library-push.dry-run",
    argv: ["library", "push", "--dry-run"],
    keys: [
      "commits.sha",
      "commits.subject",
      "checks.blocked",
      "checks.builtinScan.count",
      "checks.audit.findings",
      "checks.gitleaks.available",
      "plan.undo",
      "plan.warnings",
    ],
  },
  {
    golden: "skills-tap-ls",
    argv: ["skills", "tap", "ls"],
    keys: ["taps"],
  },
  {
    golden: "skills-tap-update.none",
    argv: ["skills", "tap", "update"],
    keys: ["failed", "results"],
  },
  {
    golden: "skills-search.empty",
    argv: ["skills", "search", "deploy"],
    keys: ["query", "tapCount", "results", "discoveryWarnings"],
  },
  {
    golden: "skills-install.preview",
    argv: ["skills", "install", "@acme-co/agent-skills/dep-check", "--dry-run"],
    keys: ["tap", "blocked", "audit.findings", "audit.high", "skills"],
    worldOnly: ["skills.name", "skills.path", "skills.files"],
  },
  {
    golden: "sources-ls.summary",
    argv: ["sources", "ls"],
    keys: [
      "generatedAt",
      "sources.id",
      "sources.origin.kind",
      "sources.origin.name",
      "sources.status.state",
      "sources.status.detail",
      "sources.writable",
    ],
  },
  {
    golden: "sources-root-ls",
    argv: ["sources", "root", "ls"],
    keys: ["roots.path", "roots.origin", "roots.exists"],
  },
  {
    golden: "skills-clean.preview",
    argv: ["skills", "clean", "--dry-run"],
    keys: ["managed", "removed", "skipped"],
  },
  {
    golden: "skills-bundle.preview",
    argv: ["skills", "bundle", "--dry-run"],
    keys: ["output", "fileCount", "sourceBytes", "bundleBytes"],
  },
  {
    golden: "skills-unbundle.preview",
    argv: ["skills", "unbundle", "/tmp/x.zip", "--dry-run"],
    keys: ["names", "files", "overwritten"],
  },
  {
    golden: "skills-uninstall.preview",
    argv: ["skills", "uninstall", "api-review", "--dry-run"],
    keys: ["name", "outputs", "sourcePath"],
  },
  {
    golden: "skills-add.preview",
    argv: ["skills", "add", "fresh-skill", "--dry-run"],
    keys: ["name", "type", "path", "files"],
  },
  {
    golden: "skills-init.preview",
    argv: ["skills", "init", "--dry-run"],
    keys: ["alreadyExists", "created", "repo"],
  },
  {
    golden: "agents-ls",
    argv: ["agents", "ls"],
    keys: [
      "agents.name",
      "agents.description",
      "agents.model",
      "agents.path",
      "agents.tools",
    ],
  },
  {
    golden: "agents-status",
    argv: ["agents", "status"],
    keys: ["drift", "lockfilePresent", "outputs"],
    worldOnly: ["outputs.name", "outputs.client", "outputs.present"],
  },
  {
    golden: "agents-sync.preview",
    argv: ["agents", "sync", "--dry-run"],
    keys: [
      "agentCount",
      "agents.name",
      "agents.outputFiles.client",
      "agents.outputFiles.files",
    ],
  },
  {
    golden: "agents-lint",
    argv: ["agents", "lint"],
    keys: ["errors", "warnings"],
  },
  {
    golden: "agents-audit",
    argv: ["agents", "audit"],
    keys: ["findings"],
  },
  {
    golden: "agents-uninstall.preview",
    argv: ["agents", "uninstall", "code-scout", "--dry-run"],
    keys: ["name", "outputs"],
  },
  {
    golden: "styles-ls",
    argv: ["styles", "ls"],
    options: { styles: true },
    keys: ["styles.name", "styles.description", "styles.synced"],
  },
  {
    golden: "styles-sync.preview",
    argv: ["styles", "sync", "--dry-run"],
    options: { styles: true },
    keys: ["styleCount", "outputs"],
  },
  {
    golden: "styles-status",
    argv: ["styles", "status"],
    options: { applied: true },
    keys: ["lockfilePresent", "native", "applyRemove"],
    worldOnly: [
      "native.client",
      "native.name",
      "native.styles",
      "applyRemove.client",
      "applyRemove.name",
      "applyRemove.active",
    ],
  },
  {
    golden: "styles-apply.preview",
    argv: ["styles", "apply", "plain", "--client", "cursor", "--dry-run"],
    options: { styles: true },
    keys: [
      "name",
      "dryRun",
      "scope",
      "applied.client",
      "applied.path",
      "appliedCount",
      "replaced",
      "active.client",
      "active.style",
      "nativeClients",
    ],
  },
  {
    golden: "styles-apply.apply",
    argv: ["styles", "apply", "plain", "--client", "cursor"],
    options: { styles: true },
    keys: ["name", "dryRun", "applied.client", "applied.path", "active.client"],
  },
  {
    golden: "styles-remove.preview",
    argv: ["styles", "remove", "--client", "cursor", "--dry-run"],
    options: { applied: true },
    keys: [
      "dryRun",
      "hadActive",
      "removed.client",
      "removed.style",
      "removed.path",
      "active",
      "clientKeys",
    ],
  },
  {
    golden: "skills-tap-link.preview",
    argv: ["skills", "tap", "link", "acme-skills", "--dry-run"],
    keys: [
      "tap",
      "dryRun",
      "scope",
      "changes.action",
      "changes.key",
      "changes.path",
      "conflicts",
      "plan.summary",
      "plan.steps.op",
      "plan.steps.detail",
      "plan.warnings",
      "plan.undo",
      "result",
    ],
  },
  {
    golden: "skills-tap-link.apply",
    argv: ["skills", "tap", "link", "acme-skills"],
    keys: [
      "tap",
      "dryRun",
      "changes.key",
      "result.applied",
      "result.changed",
      "result.undo",
    ],
  },
  {
    golden: "skills-tap-unlink.not_linked",
    argv: ["skills", "tap", "unlink", "acme-skills", "--dry-run"],
    keys: ["tap", "dryRun", "changes", "plan.summary", "plan.steps.detail", "plan.undo"],
  },
];

describe("the Library fixture world", () => {
  it("test01_every_key_the_screen_reads_is_in_the_Rust_golden_and_in_the_world", () => {
    for (const read of READS) {
      const real = golden(read.golden);
      const world = reply(read.argv, read.options);
      for (const key of [...read.keys, ...(read.worldOnly ?? [])]) {
        if (!read.worldOnly?.includes(key))
          expect(
            at(real, key),
            `${read.golden}: golden lacks ${key}`,
          ).not.toBeUndefined();
        expect(
          at(world, key),
          `${read.argv.join(" ")}: world lacks ${key}`,
        ).not.toBeUndefined();
      }
    }
  });

  it("test02_apply_writes_the_plan_and_the_next_dry_run_is_empty_with_an_undo_waiting", () => {
    const world = createLibraryWorld({ scenario: "plan" });
    const before = world.run(["skills", "sync", "--dry-run"]).data as {
      plan: { changes: unknown[] };
    };
    expect(before.plan.changes.length).toBeGreaterThan(10);
    world.run(["skills", "sync", "--apply-held"]);
    const after = world.run(["skills", "sync", "--dry-run"]).data as {
      plan: { changes: unknown[] };
    };
    expect(after.plan.changes).toEqual([]);
    expect(world.run(["skills", "undo", "--dry-run"]).ok).toBe(true);
  });

  it("test03_undo_brings_the_plan_back_and_pauses_auto_sync", () => {
    const world = createLibraryWorld({ scenario: "plan" });
    world.run(["skills", "sync", "--apply-held"]);
    world.run(["skills", "undo"]);
    expect(world.run(["skills", "auto", "status"]).data).toMatchObject({
      paused: true,
      reason: "undo",
    });
    const plan = world.run(["skills", "sync", "--dry-run"]).data as {
      plan: { changes: unknown[] };
    };
    expect(plan.plan.changes.length).toBeGreaterThan(10);
    expect(world.run(["skills", "undo", "--dry-run"]).ok).toBe(false);
  });

  it("test04_a_sync_without_apply_held_leaves_the_held_changes_in_the_plan", () => {
    const world = createLibraryWorld({ scenario: "plan" });
    world.run(["skills", "sync"]);
    const plan = world.run(["skills", "sync", "--dry-run"]).data as {
      held: unknown[];
    };
    expect(plan.held.length).toBeGreaterThan(0);
  });

  it("test05_each_scenario_answers_the_sync_the_way_its_state_needs", () => {
    expect(
      createLibraryWorld({ scenario: "empty" }).run(["skills", "sync", "--dry-run"]),
    ).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    expect(
      createLibraryWorld({ scenario: "busy" }).run(["skills", "sync", "--dry-run"]),
    ).toMatchObject({
      ok: false,
      error: { code: "conflict" },
    });
    expect(
      createLibraryWorld({ scenario: "blocked" }).run(["skills", "sync", "--dry-run"]),
    ).toMatchObject({
      ok: false,
    });
    const held = createLibraryWorld({ scenario: "held" }).run([
      "skills",
      "sync",
      "--dry-run",
      "--unattended",
    ]).data as { held: unknown[] };
    expect(held.held.length).toBeGreaterThan(0);
  });

  it("test06_auto_sync_starts_off_turns_on_and_off_and_keeps_the_pause_apart", () => {
    const world = createLibraryWorld();
    const status = () =>
      world.run(["skills", "auto", "status"]).data as {
        paused: boolean;
        lastRun: unknown;
        schedule: { enabled: boolean; nextRun: string | null };
      };
    expect(status().schedule).toMatchObject({ enabled: false, nextRun: null });
    expect(status().lastRun).toBeNull();
    const preview = world.run(["skills", "auto", "on", "--dry-run"]).data as {
      dryRun: boolean;
    };
    expect(preview.dryRun).toBe(true);
    expect(status().schedule.enabled).toBe(false);
    world.run(["skills", "auto", "on"]);
    expect(status().schedule.enabled).toBe(true);
    expect(Date.parse(status().schedule.nextRun ?? "")).toBeGreaterThan(Date.now());
    world.run(["skills", "auto", "pause"]);
    expect(status()).toMatchObject({ paused: true, schedule: { enabled: true } });
    world.run(["skills", "auto", "off"]);
    expect(status().schedule.enabled).toBe(false);
  });

  it("test07_an_unattended_run_records_what_it_held_and_a_manual_apply_clears_it", () => {
    const world = createLibraryWorld({ scenario: "held", auto: "on" });
    world.run(["skills", "sync", "--unattended"]);
    const run = () =>
      (world.run(["skills", "auto", "status"]).data as { lastRun: { held: unknown[] } })
        .lastRun;
    expect(run().held).toHaveLength(5);
    world.run(["skills", "sync"]);
    expect(run().held).toHaveLength(5);
    world.run(["skills", "sync", "--apply-held"]);
    expect(run().held).toEqual([]);
  });

  it("test08_the_seeded_states_are_off_on_held_failed_and_paused", () => {
    const last = (auto: LibraryWorldOptions["auto"]) =>
      createLibraryWorld({ scenario: "held", auto }).run(["skills", "auto", "status"])
        .data as { lastRun: { held: unknown[]; error?: string } | null; paused: boolean };
    expect(last("off").lastRun).toBeNull();
    expect(last("on").lastRun?.held).toEqual([]);
    expect(last("held").lastRun?.held).toHaveLength(5);
    expect(last("failed").lastRun?.error).toBe("no skills repository found");
    expect(last("paused")).toMatchObject({ paused: true, lastRun: null });
  });

  it("test09_a_tap_that_is_a_marketplace_has_the_keys_of_the_Rust_listing", () => {
    const real = (
      golden("skills-tap-link.listed") as {
        taps: Array<{ marketplace?: Record<string, unknown> }>;
      }
    ).taps.find((tap) => tap.marketplace)?.marketplace as {
      plugins: Array<Record<string, unknown>>;
    };
    const world = (
      reply(["skills", "tap", "ls"]) as {
        taps: Array<{ name: string; marketplace?: Record<string, unknown> }>;
      }
    ).taps.find((tap) => tap.name === "acme-skills")?.marketplace as {
      plugins: Array<Record<string, unknown>>;
    };
    expect(Object.keys(world).sort()).toEqual(Object.keys(real).sort());
    expect(Object.keys(world.plugins[0]).sort()).toEqual(
      Object.keys(real.plugins[0]).sort(),
    );
  });

  it("test10_the_frontmatter_tool_answers_with_the_keys_of_the_Rust_preview", () => {
    const real = JSON.parse(
      readFileSync(
        join(goldens, "../selfmcp-envelopes/skills_edit_frontmatter.preview.json"),
        "utf8",
      ),
    ).result as Record<string, unknown>;
    const out = createLibraryWorld({}).tool("skills_edit_frontmatter", {
      name: "triage",
      patch: { description: "Sort new issues" },
      dry_run: true,
    });
    const world = (out?.data as { result: Record<string, unknown> }).result;
    expect(Object.keys(world).sort()).toEqual(Object.keys(real).sort());
    const row = (real.lint as Array<Record<string, unknown>>)[0];
    expect(
      Object.keys((world.lint as Array<Record<string, unknown>>)[0] ?? row).sort(),
    ).toEqual(Object.keys(row).sort());
  });
});
