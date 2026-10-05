import { describe, expect, it } from "vitest";
import { createPluginsWorld } from "./world";
import type { HooksLsData, PluginsShowData } from "../types/plugins";

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

  it("answers nothing for an argv it does not run", () => {
    expect(run(createPluginsWorld(), "plugins enable ecc@ecc")).toBeUndefined();
  });
});
