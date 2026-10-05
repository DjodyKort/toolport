import { describe, expect, it } from "vitest";
import { createPluginsWorld } from "./world";
import type {
  HooksLsData,
  PluginsFolderSwitchData,
  PluginsLsData,
  PluginsShowData,
  PluginsUserSwitchData,
} from "../types/plugins";

const CWD = "/home/demo/work/side-project";
const run = (world: ReturnType<typeof createPluginsWorld>, line: string) =>
  world.reply(line.split(" "));

describe("plugins world", () => {
  it("leaves every read alone after a preview and changes it after an apply", () => {
    const world = createPluginsWorld();
    const set = `plugins config ecc@ecc --cwd ${CWD} --set gateguard=off`;
    run(world, `${set} --dry-run`);
    const before = run(world, `hooks ls --cwd ${CWD}`) as HooksLsData;
    expect(before.hooks.every((hook) => hook.active)).toBe(true);
    run(world, set);
    const after = run(world, `hooks ls --cwd ${CWD}`) as HooksLsData;
    expect(after.hooks.filter((hook) => !hook.active).length).toBeGreaterThan(0);
    const show = run(world, `plugins show ecc@ecc --cwd ${CWD}`) as PluginsShowData;
    expect(show.knobs.find((knob) => knob.key === "gateguard")?.current).toEqual({
      value: "off",
      from: "folder-env",
    });
    const other = run(world, "plugins show ecc@ecc") as PluginsShowData;
    expect(other.knobs.find((knob) => knob.key === "gateguard")?.current.from).toBe(
      "default",
    );
  });

  it("denies and allows a server per folder and refuses what the plugin does not have", () => {
    const world = createPluginsWorld();
    run(world, `plugins mcp deny ecc@ecc memory --cwd ${CWD}`);
    const show = run(world, `plugins show ecc@ecc --cwd ${CWD}`) as PluginsShowData;
    expect(show.mcpServers.find((s) => s.name === "memory")?.denied.local).toBe(true);
    run(world, `plugins mcp allow ecc@ecc memory --cwd ${CWD}`);
    expect(world.state.folders[CWD].denied).toEqual([]);
    expect(run(world, `plugins mcp deny ecc@ecc nothing --cwd ${CWD}`)).toMatchObject({
      code: "usage",
    });
  });

  it("moves a plugin to its new version through cc update and asks for a restart", () => {
    const world = createPluginsWorld();
    run(world, "cc update ecc --dry-run");
    expect(world.state.plugins[0].installed).toBe("2.1.0");
    run(world, "cc update ecc");
    const list = run(world, "cc list") as { restartRequired: boolean };
    expect(list.restartRequired).toBe(true);
    expect(world.state.plugins[0]).toMatchObject({ installed: "2.2.0", available: null });
  });

  it("turns a plugin off in one folder, drops its hooks there and turns it back on", () => {
    const world = createPluginsWorld();
    const off = `plugins off ecc@ecc --cwd ${CWD}`;
    const plan = run(world, `${off} --dry-run`) as PluginsFolderSwitchData;
    expect(plan).toMatchObject({ dryRun: true, scope: "folder", cwd: CWD, result: null });
    expect(plan.changes).toEqual([
      { action: "set", key: "enabledPlugins.ecc@ecc", value: false },
    ]);
    expect(world.state.folders[CWD]?.off ?? []).toEqual([]);
    const done = run(world, off) as PluginsFolderSwitchData;
    expect(done.result?.applied).toBe(true);
    const show = run(world, `plugins show ecc@ecc --cwd ${CWD}`) as PluginsShowData;
    expect(show.enabled).toEqual({
      user: true,
      project: null,
      local: false,
      effective: false,
    });
    const elsewhere = run(world, "plugins show ecc@ecc") as PluginsShowData;
    expect(elsewhere.enabled.effective).toBe(true);
    const hooks = run(world, `hooks ls --cwd ${CWD}`) as HooksLsData;
    expect(hooks.hooks.some((hook) => hook.owner.name === "ecc@ecc")).toBe(false);
    expect(hooks.counts.byOwner["plugin:ecc@ecc"]).toBeUndefined();
    expect(
      (run(world, "hooks ls") as HooksLsData).hooks.some(
        (hook) => hook.owner.name === "ecc@ecc",
      ),
    ).toBe(true);
    const again = run(world, `${off} --dry-run`) as PluginsFolderSwitchData;
    expect(again.changes[0].action).toBe("none");
    expect(again.ledger).toBeNull();
    run(world, `plugins on ecc@ecc --cwd ${CWD}`);
    expect(world.state.folders[CWD].off).toEqual([]);
    const back = run(world, `plugins ls --cwd ${CWD}`) as PluginsLsData;
    expect(back.plugins[0].enabled.effective).toBe(true);
  });

  it("words the notes of a plugin without hooks from what it brings", () => {
    const world = createPluginsWorld();
    const plan = run(
      world,
      `plugins off demo-plugin@fake-market --cwd ${CWD} --dry-run`,
    ) as PluginsFolderSwitchData;
    expect(plan.plan.summary).toContain("demo-plugin@fake-market");
    expect(plan.plan.steps.map((step) => step.detail).join("\n")).toContain(
      "goes away in this folder: 3 skills, 2 agents, 2 commands",
    );
  });

  it("disables and enables a plugin for the user scope, and a preview changes nothing", () => {
    const world = createPluginsWorld();
    const plan = run(world, "plugins disable ecc@ecc --dry-run") as PluginsUserSwitchData;
    expect(plan.changes[0].command).toEqual([
      "claude",
      "plugin",
      "disable",
      "ecc@ecc",
      "--scope",
      "user",
    ]);
    expect(world.state.disabledUser).toEqual([]);
    run(world, "plugins disable ecc@ecc");
    const show = run(world, `plugins show ecc@ecc --cwd ${CWD}`) as PluginsShowData;
    expect(show.enabled).toMatchObject({ user: false, effective: false });
    const enable = run(
      world,
      "plugins enable ecc@ecc --dry-run",
    ) as PluginsUserSwitchData;
    expect(enable.plan.warnings.some((w) => w.startsWith("already on"))).toBe(false);
    run(world, "plugins enable ecc@ecc");
    expect(world.state.disabledUser).toEqual([]);
  });

  it("refuses an unknown plugin and a folder switch without a folder", () => {
    const world = createPluginsWorld();
    expect(run(world, "plugins disable nope@nowhere --dry-run")).toMatchObject({
      code: "not_found",
    });
    expect(run(world, "plugins off nope@nowhere --cwd /x --dry-run")).toMatchObject({
      code: "not_found",
    });
    expect(run(world, "plugins off ecc@ecc")).toMatchObject({ code: "usage" });
  });

  it("answers nothing for an argv it does not run", () => {
    expect(run(createPluginsWorld(), "plugins frobnicate ecc@ecc")).toBeUndefined();
  });
});
