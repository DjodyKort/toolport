import { describe, expect, it } from "vitest";
import type { SkillsInventory } from "@/lib/types";
import type {
  AutoSchedule,
  AutoStatus,
  LibrarySkill,
  LintData,
  LintRow,
  PlanChange,
  RemoteStatus,
  StatusData,
  SyncData,
} from "./data";
import {
  applyChoice,
  autoLine,
  bytes,
  claudeState,
  fileBadge,
  filterItems,
  groupPlan,
  headline,
  heldBack,
  libraryItems,
  lintGroups,
  lengthText,
  meter,
  pullBlockedReason,
  remoteState,
  rowCount,
  tapSkillNames,
  tickLabel,
  tickNeeds,
  tilde,
  verifyLine,
} from "./model";

const change = (skill: string, path: string, op: string, held = false): PlanChange => ({
  skill,
  client: "claude-code",
  path,
  op,
  held,
});

function sync(changes: PlanChange[], extra: Partial<SyncData> = {}): SyncData {
  return {
    dryRun: true,
    unattended: false,
    paused: false,
    repo: "/lib",
    skillCount: 3,
    ruleCount: 1,
    targetedClients: ["claude-code"],
    entries: [
      { name: "alpha", type: "skill", clientsSynced: ["claude-code"], warnings: [] },
      { name: "style", type: "rule", clientsSynced: ["claude-code"], warnings: [] },
    ],
    collisions: [],
    replaced: 0,
    kept: 0,
    plan: {
      counts: {
        add: 0,
        update: 0,
        adopt: 0,
        unchanged: 0,
        differs: 0,
        edited: 0,
        remove: 0,
        hook: 0,
        kept: 0,
      },
      held: changes.filter((c) => c.held).length,
      changes,
      skipped: [],
    },
    held: [],
    snapshot: null,
    verify: null,
    syncedAt: "",
    outputRoot: "/home",
    backupRoot: "/home/.b",
    ...extra,
  };
}

const remote = (extra: Partial<RemoteStatus> = {}): RemoteStatus => ({
  repo: "/lib",
  branch: "main",
  remote: "origin",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  uncommitted: 0,
  lastFetch: null,
  auth: { checked: false, method: "git-credential", ok: true },
  fetch: { requested: false, ok: null, error: null },
  duplicateClones: [],
  ...extra,
});

const lintRow = (extra: Partial<LintRow>): LintRow => ({
  code: "x",
  consequence: "other",
  level: "warning",
  message: "m",
  name: "alpha",
  ...extra,
});

describe("library model", () => {
  it("test01_plan_groups_skills_rules_and_hooks_and_orders_new_before_removals", () => {
    const groups = groupPlan(
      sync([
        change("old", "a/SKILL.md", "remove", true),
        change("fresh", "b/SKILL.md", "add"),
        change("fresh", "b/x.md", "add"),
        change("alpha", "c/SKILL.md", "update"),
        change("alpha", ".claude/settings.json#Stop", "hook", true),
        change("style", "rules/style.md", "update"),
      ]),
    );
    expect(groups.skills.map((row) => row.skill)).toEqual(["fresh", "alpha", "old"]);
    expect(groups.rules.map((row) => row.skill)).toEqual(["style"]);
    expect(groups.hooks.map((row) => row.skill)).toEqual(["alpha"]);
    expect(groups.skills[0].summary).toBe("New. 2 files.");
    expect(groups.skills[2].summary).toContain(
      "Deletes the 1 file sync installed earlier",
    );
    expect(groups).toMatchObject({ written: 4, removed: 1, hooksAdded: 1, held: 2 });
  });

  it("test02_a_plan_counts_by_skill_not_by_file", () => {
    expect(
      rowCount([
        change("a", "1", "add"),
        change("a", "2", "add"),
        change("b", "3", "update"),
      ]),
    ).toBe(2);
  });

  it("test03_a_hook_from_a_tap_needs_the_tick_but_a_hook_of_your_own_library_does_not", () => {
    const plan = sync([
      change("own", "s#Stop", "hook", true),
      change("tapped", "s#Stop", "hook", true),
    ]);
    const own = tickNeeds(plan, new Set());
    expect(own).toEqual({ differs: 0, tapHooks: 0 });
    expect(applyChoice(plan, own, false, null).argv).toEqual([
      "skills",
      "sync",
      "--apply-held",
    ]);
    const tapped = tickNeeds(plan, new Set(["tapped"]));
    expect(tapped.tapHooks).toBe(1);
    expect(applyChoice(plan, tapped, false, null)).toEqual({
      argv: ["skills", "sync"],
      count: 0,
    });
    expect(applyChoice(plan, tapped, true, null).argv).toEqual([
      "skills",
      "sync",
      "--apply-held",
      "--allow-tap-hooks",
    ]);
    expect(tickLabel(tapped)).toBe("Install 1 hook that come from a tap");
  });

  it("test04_files_that_differ_wait_for_the_tick_while_the_rest_of_the_plan_applies", () => {
    const plan = sync([
      change("a", "1", "update"),
      change("b", "2", "differs", true),
      change("c", "3", "remove", true),
    ]);
    const needs = tickNeeds(plan, new Set());
    expect(applyChoice(plan, needs, false, ["claude-code", "agents-md"])).toEqual({
      argv: ["skills", "sync", "--client", "claude-code", "--client", "agents-md"],
      count: 1,
    });
    expect(applyChoice(plan, needs, true, null)).toEqual({
      argv: ["skills", "sync", "--apply-held"],
      count: 3,
    });
  });

  it("test05_a_lint_error_holds_back_only_that_skill", () => {
    const lint: LintData = {
      errors: 2,
      warnings: 1,
      limits: { description: 1536, specDescription: 1024 },
      consequences: {},
      messages: [
        lintRow({ name: "b", level: "error" }),
        lintRow({ name: "b", level: "error", code: "y" }),
        lintRow({ name: "a", level: "error" }),
        lintRow({ name: "c", level: "warning" }),
      ],
    };
    expect(heldBack(lint).map((h) => [h.skill, h.rows.length])).toEqual([
      ["a", 1],
      ["b", 2],
    ]);
    expect(heldBack(null)).toEqual([]);
  });

  it("test06_lint_groups_follow_what_claude_does_and_count_the_deny_list_rows_once", () => {
    const lint: LintData = {
      errors: 1,
      warnings: 1,
      limits: { description: 1536, specDescription: 1024 },
      consequences: {},
      messages: [
        lintRow({ level: "error", consequence: "hidden" }),
        lintRow({ level: "warning" }),
        lintRow({ level: "info", consequence: "weak-trigger" }),
        lintRow({ level: "info", consequence: "not-copied", name: "a" }),
        lintRow({ level: "info", consequence: "not-copied", name: "a" }),
        lintRow({ level: "info", consequence: "not-copied", name: "b" }),
      ],
    };
    const groups = lintGroups(lint);
    expect(groups.map((g) => g.id)).toEqual(["hides", "catch", "advice"]);
    expect(groups[2].rows).toHaveLength(1);
    expect(groups[2].notCopied).toEqual({ files: 3, skills: 2 });
    expect(lintGroups(null)).toEqual([]);
  });

  it("test07_the_meter_shows_both_limits_and_flags_a_length_above_the_one_claude_keeps", () => {
    const over = meter(1602, 1536, 1024);
    expect(over.over).toBe(true);
    expect(over.specTick).toBeLessThan(over.limitTick);
    expect(over.fill).toBeGreaterThan(over.limitTick);
    const inside = meter(912, 1536, 1024);
    expect(inside.over).toBe(false);
    expect(inside.fill).toBeLessThan(inside.specTick);
  });

  it("test07b_the_length_text_names_both_parts_only_when_when_to_use_is_set", () => {
    expect(lengthText({ length: 912, description: 912, whenToUse: 0 })).toBe(
      "912 characters",
    );
    expect(lengthText({ length: 1602, description: 1200, whenToUse: 402 })).toBe(
      "1,602 characters (description 1,200 + when_to_use 402)",
    );
  });

  it("test08_remote_states_say_why_pull_is_off", () => {
    expect(remoteState(null).kind).toBe("none");
    expect(remoteState(remote({ remote: null })).kind).toBe("no-remote");
    expect(remoteState(remote({ behind: 3 }))).toEqual({ kind: "behind", behind: 3 });
    const dirty = remoteState(remote({ behind: 3, uncommitted: 2 }));
    expect(dirty.kind).toBe("dirty");
    expect(pullBlockedReason(dirty)).toContain("commit or stash the 2 changes");
    expect(remoteState(remote({ ahead: 1, behind: 2 })).kind).toBe("diverged");
    expect(
      remoteState(remote({ fetch: { requested: true, ok: false, error: "no route" } })),
    ).toEqual({ kind: "offline", error: "no route" });
    expect(pullBlockedReason(remoteState(remote({ behind: 1 })))).toBeNull();
  });

  it("test09_headline_tells_in_sync_first_sync_held_back_behind_and_undone_apart", () => {
    const auto: AutoStatus = {
      paused: false,
      reason: null,
      since: null,
      lastRun: null,
      schedule: null,
    };
    const one = sync([change("alpha", "1", "update")]);
    const none = remoteState(remote());
    expect(headline(sync([]), [], none, auto, true).kind).toBe("in-sync");
    expect(headline(one, [], none, auto, false).kind).toBe("first-sync");
    expect(headline(one, [], none, auto, true).kind).toBe("changes");
    expect(headline(one, [], remoteState(remote({ behind: 3 })), auto, true).kind).toBe(
      "behind",
    );
    const held = headline(one, [{ skill: "bad", rows: [] }], none, auto, true);
    expect(held.title).toBe("1 change can sync; 1 skill is held back");
    expect(
      headline(one, [], none, { ...auto, paused: true, reason: "undo", since: "x" }, true)
        .kind,
    ).toBe("undone");
    expect(
      headline(
        one,
        [],
        none,
        { ...auto, paused: true, reason: "manual", since: "x" },
        true,
      ).kind,
    ).toBe("changes");
  });

  it("test10_file_badges_use_the_words_of_the_files_list", () => {
    const file = (status: string, op: string | null = null) => ({
      client: null,
      op,
      path: "p",
      reason: null,
      size: 1,
      status,
    });
    expect(fileBadge(file("would-copy", "add")).label).toBe("New");
    expect(fileBadge(file("would-copy", "update")).label).toBe("Changed");
    expect(fileBadge(file("copied")).label).toBe("Same");
    expect(fileBadge(file("not-copied")).label).toBe("Not copied");
    expect(fileBadge(file("not-yours")).label).toBe("Not yours");
    expect(fileBadge(file("hand-edited")).label).toBe("Edited outside");
  });

  it("test11_library_items_take_their_state_from_the_plan_the_lock_and_lint", () => {
    const skills = ["alpha", "beta", "gamma", "delta"].map((name): LibrarySkill => ({
      name,
      type: "skill",
      description: `${name} does things`,
      activation: "auto",
      visible: true,
      invisibleReason: null,
      writable: true,
      path: "p",
      origin: { kind: name === "delta" ? "tap" : "library", name: "lib" },
    }));
    const status = {
      entries: [
        {
          name: "alpha",
          type: "skill",
          clientsSynced: ["claude-code"],
          drifted: false,
          knownToLockfile: true,
        },
        {
          name: "beta",
          type: "skill",
          clientsSynced: ["claude-code"],
          drifted: false,
          knownToLockfile: true,
        },
      ],
    } as StatusData;
    const inventory = {
      skills: [{ name: "alpha", states: { "claude-code": { state: "off", via: "" } } }],
    } as unknown as SkillsInventory;
    const items = libraryItems(
      skills,
      status,
      sync([change("alpha", "1", "update"), change("gamma", "2", "add")]),
      [{ skill: "beta", rows: [] }],
      inventory,
    );
    expect(Object.fromEntries(items.map((i) => [i.name, i.state]))).toEqual({
      alpha: "changed",
      beta: "held",
      gamma: "new",
      delta: "unsynced",
    });
    expect(items.find((i) => i.name === "alpha")?.claude).toBe("off");
    expect(claudeState(null, "alpha")).toBeNull();
    expect(tapSkillNames(skills)).toEqual(new Set(["delta"]));
    expect(
      filterItems(items, { query: "GAMMA", kind: "all", state: "any" }),
    ).toHaveLength(1);
    expect(
      filterItems(items, { query: "", kind: "all", state: "waiting" }).map((i) => i.name),
    ).toEqual(["alpha", "beta", "gamma"]);
  });

  it("test12_small_formatters", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(3174)).toBe("3.1 KB");
    expect(tilde("/Users/me/work/lib", "/Users/me")).toBe("~/work/lib");
    expect(tilde("/srv/lib", "/Users/me")).toBe("/srv/lib");
    expect(verifyLine({ clean: true, matched: 183, total: 183, mismatched: [] })).toBe(
      "183 of 183 library files match",
    );
  });

  describe("autoLine", () => {
    const NOW = Date.parse("2026-11-17T09:00:00Z");
    const schedule = (over: Partial<AutoSchedule> = {}): AutoSchedule => ({
      task: "skills-auto-sync",
      present: true,
      enabled: true,
      cron: "30 6 * * *",
      when: "daily 06:30 UTC",
      autoRun: true,
      nextRun: "2026-11-18T06:30:00Z",
      lastRun: null,
      ...over,
    });
    const status = (over: Partial<AutoStatus> = {}): AutoStatus => ({
      paused: false,
      reason: null,
      since: null,
      lastRun: null,
      schedule: schedule(),
      ...over,
    });
    const held = (n: number) =>
      Array.from({ length: n }, (_, i) => ({
        skill: `s${i}`,
        client: "claude-code",
        path: `p${i}`,
        op: "remove",
        reason: "removal",
      }));

    it("test13_a_task_that_is_off_offers_to_turn_it_on_with_the_time_it_would_run", () => {
      const line = autoLine(status({ schedule: schedule({ enabled: false }) }), NOW);
      expect(line.kind).toBe("off");
      expect(line.label).toBe("Auto-sync off");
      expect(line.action).toEqual({
        id: "on",
        label: expect.stringMatching(/^Turn on \(daily \d\d:\d\d\)$/),
      });
      expect(line.secondary).toBeNull();
    });

    it("test14_a_task_that_is_on_says_when_it_runs_and_how_long_ago_it_last_did", () => {
      const never = autoLine(status(), NOW);
      expect(never.kind).toBe("on");
      expect(never.label).toMatch(/^Auto-sync daily \d\d:\d\d$/);
      expect(never.detail).toBe("no run yet");
      const ran = autoLine(
        status({
          lastRun: {
            at: "2026-11-17T06:30:20Z",
            applied: 3,
            held: [],
            skipped: 0,
          },
        }),
        NOW,
      );
      expect(ran.detail).toBe("last run 3 h ago");
      expect(ran.action?.id).toBe("pause");
      expect(ran.secondary?.id).toBe("off");
    });

    it("test15_a_pause_outranks_the_schedule_and_names_what_caused_it", () => {
      const line = autoLine(
        status({ paused: true, reason: "undo", since: "2026-11-17T08:00:00Z" }),
        NOW,
      );
      expect(line.kind).toBe("paused");
      expect(line.detail).toContain("(after Undo)");
      expect(line.detail).toContain("skips its runs");
      expect(line.action).toEqual({ id: "resume", label: "Resume" });
      const off = autoLine(
        status({
          paused: true,
          reason: "manual",
          since: "2026-11-17T08:00:00Z",
          schedule: schedule({ enabled: false }),
        }),
        NOW,
      );
      expect(off.detail).toContain("(by you)");
      expect(off.detail).not.toContain("skips its runs");
    });

    it("test16_the_held_count_and_a_failure_come_from_the_last_run", () => {
      const line = autoLine(
        status({
          lastRun: { at: "2026-11-17T06:30:20Z", applied: 0, held: held(5), skipped: 0 },
        }),
        NOW,
      );
      expect(line.heldCount).toBe(5);
      expect(line.failed).toBeNull();
      const failed = autoLine(
        status({
          lastRun: {
            at: "2026-11-17T06:30:20Z",
            applied: 0,
            held: [],
            skipped: 0,
            error: "no skills repository found",
          },
        }),
        NOW,
      );
      expect(failed.failed).toBe("no skills repository found");
      const crashed = autoLine(
        status({
          schedule: schedule({
            lastRun: {
              runId: "run-1",
              status: "failed",
              trigger: "schedule",
              startedAt: "2026-11-17T06:30:00Z",
            },
          }),
        }),
        NOW,
      );
      expect(crashed.failed).toContain("failed before it recorded");
    });

    it("test17_a_removed_task_and_an_unreadable_status_offer_nothing_to_click", () => {
      const gone = autoLine(status({ schedule: schedule({ present: false }) }), NOW);
      expect(gone.kind).toBe("missing");
      expect(gone.action).toBeNull();
      expect(autoLine(null, NOW).kind).toBe("unknown");
    });
  });
});
