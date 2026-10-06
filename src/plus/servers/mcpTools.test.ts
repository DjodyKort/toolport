import { describe, expect, it } from "vitest";
import { commandsGolden } from "../fixtures/servers";
import {
  addTagPlan,
  applyUpdatePlan,
  callResult,
  forkSyncArgs,
  forkSyncDefaults,
  forkSyncDefaultsFor,
  forkSyncPlan,
  forkSyncProblems,
  forkSyncResultPlan,
  gitStateOf,
  profileMatch,
  sourceOf,
  syncBlocker,
  toolPolicyOf,
  updateOf,
  type SourceInfo,
} from "./mcpTools";
import { serversWorld } from "../fixtures/servers";
import { createToolsWorld } from "./world";

describe("toolPolicyOf", () => {
  it("reads the tier from the registry and asks for confirm from tier 3", () => {
    expect(toolPolicyOf(commandsGolden, "servers_check_updates")).toEqual({
      name: "servers_check_updates",
      tier: "read",
      needsConfirm: false,
    });
    expect(toolPolicyOf(commandsGolden, "servers_add_profile_tag")?.needsConfirm).toBe(
      false,
    );
    for (const tool of [
      "servers_apply_update",
      "servers_set_mode",
      "servers_fork_sync",
    ]) {
      expect(toolPolicyOf(commandsGolden, tool)).toMatchObject({
        tier: "write",
        needsConfirm: true,
      });
    }
  });

  it("does not know a tool the registry lacks, or any tool without mcp call", () => {
    expect(toolPolicyOf(commandsGolden, "servers_nope")).toBeNull();
    const without = {
      ...commandsGolden,
      commands: commandsGolden.commands.filter((row) => row.id !== "mcp call"),
    };
    expect(toolPolicyOf(without, "servers_check_updates")).toBeNull();
    expect(toolPolicyOf(null, "servers_check_updates")).toBeNull();
  });
});

describe("callResult", () => {
  it("unwraps the result of a call and refuses an error result", () => {
    expect(callResult({ isError: false, result: { a: 1 }, tier: 1, tool: "t" })).toEqual({
      ok: true,
      result: { a: 1 },
    });
    expect(callResult({ isError: true, result: { error: { message: "no" } } })).toEqual({
      ok: false,
      message: "no",
    });
    expect(callResult(null)).toMatchObject({ ok: false });
  });
});

describe("the real goldens read as the screen needs them", () => {
  const world = createToolsWorld();
  const ok = (tool: string, args: Record<string, unknown>) => {
    const parsed = callResult(world.call(tool, args));
    if (!parsed.ok) throw new Error(parsed.message);
    return parsed.result;
  };

  it("reads a source, a git state and an update", () => {
    expect(sourceOf(ok("servers_detect_source", { name: "docs-search" }))).toMatchObject({
      kind: "git",
      stored: true,
      branch: "main",
    });
    const git = gitStateOf(ok("servers_git_status", { name: "docs-search" }));
    expect(git).toMatchObject({ isGit: true, ahead: 1, behind: 2, dirty: false });
    expect(syncBlocker(git)).toBeNull();
    expect(syncBlocker({ ...git, dirty: true })).toMatch(/uncommitted/);
    expect(
      syncBlocker(gitStateOf(ok("servers_git_status", { name: "wiki-reader" }))),
    ).toMatch(/not git/);
    expect(updateOf(ok("servers_check_updates", { name: "docs-search" }))?.status).toBe(
      "update-available",
    );
  });

  it("always says the stored update command runs, with or without one in the report", () => {
    const withHook = updateOf(ok("servers_check_updates", { name: "docs-search" }));
    const plan = applyUpdatePlan("docs-search", withHook);
    expect(plan.steps.map((step) => step.detail)).toContain(
      "Runs the stored update command (post_update): npm run build",
    );
    expect(plan.warnings.join(" ")).toMatch(/stored update command/);
    expect(applyUpdatePlan("x", null).steps.at(-1)?.detail).toMatch(/post_update/);
  });
});

describe("profile tags", () => {
  it("matches a profile by id or by name in any case, as the tool does", () => {
    expect(profileMatch(serversWorld.profileLs, "work")?.id).toBe("work");
    expect(profileMatch(serversWorld.profileLs, "RESEARCH")?.id).toBe("research");
    expect(profileMatch(serversWorld.profileLs, "lab")).toBeUndefined();
  });

  it("warns that a new name creates the profile", () => {
    expect(addTagPlan("a", "Lab", false).warnings[0]).toMatch(/creates one/);
    expect(addTagPlan("a", "Work", true).warnings).toEqual([]);
  });
});

describe("fork sync", () => {
  it("sends only what was filled in, and the confirm", () => {
    expect(forkSyncArgs("a", forkSyncDefaults)).toEqual({
      name: "a",
      upstream_remote: "upstream",
      upstream_branch: "main",
      mode: "rebase",
      confirm: true,
    });
    expect(
      forkSyncArgs("a", {
        ...forkSyncDefaults,
        mode: "onto-author",
        authorEmail: " me@example.test ",
        push: true,
        runPostUpdate: true,
      }),
    ).toMatchObject({
      author_email: "me@example.test",
      push: true,
      run_post_update: true,
    });
    expect(forkSyncArgs("a", forkSyncDefaults)).not.toHaveProperty("target_branch");
  });

  it("names what is wrong with the options", () => {
    expect(forkSyncProblems(forkSyncDefaults)).toEqual([]);
    expect(forkSyncProblems({ ...forkSyncDefaults, upstreamRemote: " " })).toHaveLength(
      1,
    );
    expect(forkSyncProblems({ ...forkSyncDefaults, mode: "onto-author" })).toHaveLength(
      1,
    );
  });

  it("starts from the stored upstream, not from upstream/main", () => {
    const source = (extra: Partial<SourceInfo>): SourceInfo => ({
      kind: "git",
      stored: true,
      remote: "origin",
      remotes: ["origin", "vendor"],
      branches: { vendor: ["dev", "master"] },
      ...extra,
    });
    expect(forkSyncDefaultsFor(undefined)).toEqual(forkSyncDefaults);
    expect(
      forkSyncDefaultsFor(source({ upstream: { remote: "vendor", branch: "dev" } })),
    ).toMatchObject({ upstreamRemote: "vendor", upstreamBranch: "dev" });
    expect(forkSyncDefaultsFor(source({}))).toMatchObject({
      upstreamRemote: "vendor",
      upstreamBranch: "master",
    });
    expect(
      forkSyncDefaultsFor(source({ remotes: ["origin", "upstream", "vendor"] })),
    ).toMatchObject({ upstreamRemote: "upstream", upstreamBranch: "main" });
  });

  it("plans a worktree without a new branch and words a conflict as a stopped sync", () => {
    const git = gitStateOf({
      isGit: true,
      branch: "main",
      summaries: ["a b"],
      path: "/p",
    });
    const plan = forkSyncPlan("a", git, forkSyncDefaults);
    expect(plan.summary).toBe("Sync a with upstream/main");
    expect(plan.steps.some((step) => step.detail === "Upstream: a b")).toBe(true);
    expect(plan.steps.some((step) => /-synced-/.test(step.detail ?? ""))).toBe(false);
    expect(plan.steps.some((step) => /Push/.test(step.detail ?? ""))).toBe(false);
    const pushing = forkSyncPlan("a", git, { ...forkSyncDefaults, push: true });
    expect(pushing.steps.some((step) => /Push main/.test(step.detail ?? ""))).toBe(true);
    const stopped = forkSyncResultPlan(
      {
        synced: false,
        conflict: true,
        branch: "main",
        conflictedPaths: ["f.ts"],
        worktree: "/p/.git/toolport-sync/main-1",
        next: "go on",
      },
      "a",
    );
    expect(stopped.summary).toBe("The sync of a stopped on a conflict");
    expect(stopped.steps.map((step) => step.path ?? step.detail)).toContain("f.ts");
    expect(stopped.warnings[0]).toMatch(/not changed/);
  });

  it("lists old sync branches and reports a failed push", () => {
    const done = forkSyncResultPlan(
      {
        synced: true,
        branch: "main",
        mode: "merge",
        upstream: "upstream/main",
        staleBranches: ["main-synced-20260101"],
        push: { pushed: false, remote: "origin", error: "rejected" },
      },
      "a",
    );
    expect(done.summary).toBe("a synced: main now follows upstream/main");
    expect(done.steps.map((step) => step.detail)).toContain(
      "Push to origin failed: rejected",
    );
    expect(done.steps.some((step) => step.path === "main-synced-20260101")).toBe(true);
    expect(done.warnings).toHaveLength(1);
  });
});
