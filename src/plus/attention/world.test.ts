import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { plusCtlFixtures, plusCtlResult, plusCtlStart } from "../fixtures/plusCtl";
import { attentionDismissData, attentionLsData } from "../types/attention";
import { actionArgvs, stockItems } from "./fixtures";
import { DISMISS_CHOICES, dismissArgv, untilDate } from "./model";
import { createAttentionWorld } from "./world";

const ls = (world: ReturnType<typeof createAttentionWorld>, ...args: string[]) =>
  world.reply(["attention", "ls", ...args]) as ReturnType<typeof world.list>;

describe("the attention world", () => {
  it("answers in the shape of the real attention ls and attention dismiss", () => {
    const world = createAttentionWorld();
    expect(check(attentionLsData, ls(world))).toEqual([]);
    const preview = world.reply([
      "attention",
      "dismiss",
      "skills:invisible",
      "--dry-run",
    ]);
    expect(check(attentionDismissData, preview)).toEqual([]);
    const applied = world.reply(["attention", "dismiss", "skills:invisible"]);
    expect(check(attentionDismissData, applied)).toEqual([]);
  });

  it("counts every level while --level filters only the rows", () => {
    const world = createAttentionWorld();
    const only = ls(world, "--level", "needs-you");
    expect(only.items.every((item) => item.level === "needs-you")).toBe(true);
    expect(only.counts).toEqual(ls(world).counts);
  });

  it("hides a row until its date, and a date of today brings it back", () => {
    const world = createAttentionWorld();
    const id = "skills:invisible";
    world.reply(["attention", "dismiss", id, "--until", untilDate("week")!]);
    expect(ls(world).items.map((item) => item.id)).not.toContain(id);
    const today = untilDate("tomorrow", new Date(Date.now() - 86_400_000))!;
    world.reply(["attention", "dismiss", id, "--until", today]);
    expect(ls(world).items.map((item) => item.id)).toContain(id);
    world.reply(["attention", "dismiss", id]);
    expect(ls(world).items.map((item) => item.id)).not.toContain(id);
  });

  it("changes nothing for a dry run, and removes the row an action solved", () => {
    const world = createAttentionWorld();
    const before = ls(world).items.length;
    world.reply(["task", "run", "nightly-report", "--dry-run"]);
    expect(ls(world).items).toHaveLength(before);
    world.reply(["task", "run", "nightly-report"]);
    expect(ls(world).items.map((item) => item.id)).not.toContain(
      "tasks:nightly-report:failed",
    );
  });

  it("refuses an action no row offers", () => {
    const world = createAttentionWorld();
    expect(world.reply(["task", "run", "unknown-task"])).toMatchObject({
      code: "not_found",
    });
  });
});

describe("the attention rows of the dev browser fixture", () => {
  const argvs = (id: string) =>
    DISMISS_CHOICES.flatMap(({ id: choice }) => {
      const argv = dismissArgv(id, untilDate(choice));
      return [argv.join(" "), [...argv, "--dry-run"].join(" ")];
    });

  it("has a row for the list, the counter, every dismissal and every action with its twin", () => {
    const items = stockItems();
    const wanted = [
      "attention ls",
      "attention ls --level needs-you",
      ...items.flatMap((item) => argvs(item.id)),
      ...actionArgvs(items).flatMap((argv) => [
        argv.join(" "),
        `${argv.join(" ")} --dry-run`,
      ]),
    ];
    expect(wanted.filter((argv) => !plusCtlFixtures.has(argv))).toEqual([]);
  });

  it("answers the counter's read with the same count as the list", () => {
    const read = (argv: string[]) =>
      plusCtlResult(plusCtlStart(argv)).envelope?.data as ReturnType<
        ReturnType<typeof createAttentionWorld>["list"]
      >;
    expect(read(["attention", "ls", "--level", "needs-you"]).counts.needsYou).toBe(
      read(["attention", "ls"]).counts.needsYou,
    );
  });
});
