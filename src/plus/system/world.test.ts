import { describe, expect, it } from "vitest";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { createSystemWorld } from "./world";

const REPO = "git@git.example.com:me/toolport-sync.git";
const INIT = ["sync", "init", "--repo", REPO, "--passphrase-stdin"];
const snapshot = (world: ReturnType<typeof createSystemWorld>) =>
  JSON.stringify(world.state);
const failed = (value: unknown) => value instanceof CtlReplyFailure;

describe("System world", () => {
  it("never changes anything for a dry run", () => {
    const world = createSystemWorld();
    world.reply(INIT, "x");
    const before = snapshot(world);
    for (const argv of [
      ["sync", "push", "--dry-run"],
      ["sync", "pull", "--dry-run"],
      ["update", "--apply", "--dry-run"],
      ["update", "--init", "--dry-run"],
      ["import", "mcpm", "/old/mcpm", "--dry-run"],
      [
        "import",
        "rename-refs",
        "/old/mcpm",
        "--tools",
        "/t.json",
        "--paths",
        "/n",
        "--dry-run",
      ],
    ]) {
      expect(failed(world.reply(argv, "x")), argv.join(" ")).toBe(false);
    }
    expect(snapshot(world)).toBe(before);
  });

  it("needs the passphrase on stdin and a configured machine", () => {
    const world = createSystemWorld();
    expect(failed(world.reply(INIT, null))).toBe(true);
    expect(failed(world.reply(["sync", "push"]))).toBe(true);
    expect(world.reply(INIT, "x")).toMatchObject({ freshRemote: true, branch: "main" });
    expect(failed(world.reply(INIT, "x"))).toBe(true);
    expect(world.reply([...INIT, "--reconfigure"], "x")).toBeTruthy();
  });

  it("moves the bundle on a push and brings another machine's files on a pull", () => {
    const world = createSystemWorld();
    world.reply(INIT, "x");
    expect(world.reply(["sync", "diff"])).toMatchObject({ noRemote: true });
    world.reply(["sync", "push"]);
    expect(world.reply(["sync", "status"])).toMatchObject({
      tracked: 3,
      lastDirection: "push",
    });
    world.remoteEdit("registry.json", "v2");
    expect(world.reply(["sync", "diff"])).toMatchObject({
      changes: { modified: ["registry.json"] },
    });
    world.localEdit("registry.json", "local");
    expect(world.reply(["sync", "pull", "--dry-run"])).toMatchObject({
      conflicts: ["registry.json"],
    });
    expect(world.reply(["sync", "pull"])).toMatchObject({ keptLocal: ["registry.json"] });
    expect(world.state.sync.local["registry.json"]).toBe("local");
    expect(world.reply(["sync", "pull", "--force"])).toMatchObject({
      applied: ["registry.json"],
    });
    expect(world.state.sync.local["registry.json"]).toBe("v2");
  });

  it("moves a server to its latest version and stores a source once", () => {
    const world = createSystemWorld();
    const report = (argv: string[]) =>
      world.reply(argv) as { counts: Record<string, number>; mode: string };
    expect(report(["update", "--check"]).counts).toEqual({
      "update-available": 2,
      auto: 2,
      skipped: 1,
    });
    expect(report(["update", "srv-git", "--apply"]).mode).toBe("apply");
    expect(report(["update", "--check"]).counts["update-available"]).toBe(1);
    expect(report(["update", "--init"]).mode).toBe("init");
    expect(report(["update", "--check"]).counts.skipped).toBeUndefined();
    expect(failed(world.reply(["update", "srv-none", "--check"]))).toBe(true);
  });

  it("does not run an argv it does not know", () => {
    expect(createSystemWorld().reply(["server", "ls"])).toBeUndefined();
  });
});
