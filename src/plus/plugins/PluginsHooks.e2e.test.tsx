import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, screen, waitFor, within } from "@testing-library/react";
import type { HooksLsData } from "../types/plugins";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import {
  chooseFolder,
  createBridge,
  CtlReplyFailure,
  FOLDER,
  openHooks,
  openView,
  openPlugins,
  openUpdates,
  pageText,
  SENTINEL,
  type Bridge,
} from "./e2e";

/** Library > Plugins, Context > Hooks and the System > Updates plugin card walked through the
 * real screens against the stateful plugins world: an applied settings write, server deny or
 * update changes the next read of `plugins show`, `hooks ls` and `cc list`, a preview never
 * does. Each test is named by the parity action it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge();
  invoke.mockReset().mockImplementation(bridge.invoke);
  listen.mockReset().mockResolvedValue(() => {});
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const ran = bridge.ran();
  expect(
    ran.some((line) =>
      /--home|--data-dir|secret|--reveal|stdin/.test(line.replace(/ --args-stdin$/, "")),
    ),
    "no secret flag",
  ).toBe(false);
  expect(
    ran.some((line) => /^plugins show\s*$/.test(line)),
    "no read without a plugin id",
  ).toBe(false);
  expect(
    ran.some((line) => /^claude\b|\bclaude plugin\b/.test(line)),
    "the app never runs claude itself",
  ).toBe(false);
  expect(
    invoke.mock.calls
      .map(([command]) => String(command))
      .filter((command) => !/^plus_ctl(_result|_cancel)?$/.test(command)),
    "the app only talks to toolportctl",
  ).toEqual([]);
  const switches = ran.filter((line) => /^plugins (off|on|disable|enable)\b/.test(line));
  expect(
    switches.every((line) =>
      /^plugins (off|on) \S+ --cwd \S+( --dry-run)?$|^plugins (disable|enable) \S+( --dry-run)?$/.test(
        line,
      ),
    ),
    "a switch runs with exactly the plugin id and the folder",
  ).toBe(true);
  ran.forEach((line, i) => {
    if (/^plugins (off|on|disable|enable)\b/.test(line) && !line.endsWith("--dry-run")) {
      expect(ran.slice(0, i), `${line} was previewed first`).toContain(
        `${line} --dry-run`,
      );
    }
  });
});

const dialog = (name: RegExp) => screen.findByRole("dialog", { name });
const closeResult = async (user: Awaited<ReturnType<typeof openPlugins>>) => {
  await screen.findByText("Done");
  await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
};
const CONFIG = `plugins config ecc@ecc --cwd ${FOLDER}`;

async function applyGateGuardOff(user: Awaited<ReturnType<typeof openPlugins>>) {
  await chooseFolder(user, "Use folder");
  await screen.findByRole("region", { name: "Plugin ecc" });
  await user.selectOptions(screen.getByLabelText("Hook profile"), "minimal");
  await user.selectOptions(screen.getByLabelText("GateGuard"), "off");
  await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
  const box = await dialog(/^Apply ecc settings to a folder\?$/);
  expect(within(box).getByText(/still starts a process/)).toBeVisible();
  expect(bridge.count(`${CONFIG} --set hook_profile=minimal --set gateguard=off`)).toBe(
    0,
  );
  await user.click(within(box).getByRole("button", { name: "Apply" }));
  await closeResult(user);
}

describe("plugins.ls", () => {
  it("lists the plugins with version, source and update state, and reads again after Retry", async () => {
    bridge.set(
      "plugins ls",
      () => new CtlReplyFailure("bridge", "claude could not be started"),
    );
    const user = await openView("library", "Plugins");
    expect(await screen.findByText(/claude could not be started/)).toBeVisible();
    bridge.set("plugins ls", () => bridge.world.reply(["plugins", "ls"]));
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    const list = await screen.findByRole("list", { name: "Plugins" });
    expect(within(list).getByText("ecc")).toBeVisible();
    expect(within(list).getByText("demo-plugin")).toBeVisible();
    expect(within(list).getByText(/update to 2.2.0|2.2.0/)).toBeVisible();
    expect(bridge.count("plugins ls")).toBe(2);
  });
});

describe("plugins.show", () => {
  it("shows what the plugin brings, the cost labelled projected and measured, where it is on and its servers as not governed", async () => {
    await openPlugins();
    const brings = screen.getByRole("list", { name: "What it brings" });
    expect(within(brings).getByText("23 hooks")).toBeVisible();
    const cost = document.querySelector('dl[aria-label="Cost"]') as HTMLElement;
    expect(within(cost).getByText("Projected by Claude Code")).toBeVisible();
    expect(within(cost).getByText("Measured by Toolport")).toBeVisible();
    expect(
      within(screen.getByRole("list", { name: "Where it is on" })).getByText(
        "all your folders",
      ),
    ).toBeVisible();
    expect(
      within(
        screen.getByRole("list", { name: "MCP servers of the plugin" }),
      ).getAllByText("not governed"),
    ).toHaveLength(2);
    expect(bridge.count("plugins show ecc@ecc")).toBeGreaterThanOrEqual(1);
  });

  it("explains a plugin without an adapter and never shows its leaked option value", async () => {
    bridge.set("plugins show ecc@ecc", () => {
      const data = bridge.world.reply(["plugins", "show", "ecc@ecc"]) as {
        options: Array<{ sensitive: boolean; current: unknown }>;
      };
      return {
        ...data,
        options: data.options.map((o) => (o.sensitive ? { ...o, current: SENTINEL } : o)),
      };
    });
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    expect(pageText()).not.toContain(SENTINEL);
    expect(JSON.stringify(bridge.calls)).not.toContain(SENTINEL);
    await user.click(
      within(screen.getByRole("list", { name: "Plugins" })).getByText("demo-plugin"),
    );
    await screen.findByRole("region", { name: "Plugin demo-plugin" });
    expect(screen.getByText(/no per-hook switch/)).toBeVisible();
  });
});

describe("plugins.config", () => {
  it("applies the ecc settings to a folder after the plan, then Undo in a folder puts them back", async () => {
    const user = await openPlugins();
    await applyGateGuardOff(user);
    expect(bridge.world.state.folders[FOLDER].env).toMatchObject({
      ECC_HOOK_PROFILE: "minimal",
      ECC_GATEGUARD: "off",
    });
    await user.click(screen.getByRole("button", { name: "Undo in a folder…" }));
    const box = await dialog(/^Undo ecc settings in a folder\?$/);
    await user.click(within(box).getByRole("button", { name: "Apply" }));
    await closeResult(user);
    await waitFor(() => expect(bridge.world.state.folders[FOLDER].env).toEqual({}));
    expect(bridge.ran().filter((line) => line.startsWith("plugins config"))).toHaveLength(
      4,
    );
  });

  it("applies nothing when Escape dismisses the plan, and the focus returns to the button", async () => {
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    await screen.findByRole("region", { name: "Plugin ecc" });
    await user.selectOptions(screen.getByLabelText("GateGuard"), "off");
    const opener = screen.getByRole("button", { name: "Apply to a folder…" });
    await user.click(opener);
    const box = await dialog(/^Apply ecc settings to a folder\?$/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
    expect(bridge.world.state.folders[FOLDER]?.env ?? {}).toEqual({});
    expect(
      bridge
        .ran()
        .filter(
          (line) => line.startsWith("plugins config") && !line.endsWith("--dry-run"),
        ),
    ).toEqual([]);
  });
});

describe("plugins.mcp", () => {
  it("denies a plugin server in a folder after the plan, and Allow again puts it back", async () => {
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    await screen.findByRole("region", { name: "Plugin ecc" });
    const servers = screen.getByRole("list", { name: "MCP servers of the plugin" });
    await user.click(
      within(servers).getAllByRole("button", { name: "Deny in a folder…" })[0],
    );
    const box = await dialog(/^Deny chrome-devtools in this folder\?$/);
    expect(within(box).getByText(/mcp__plugin_ecc_chrome-devtools__/)).toBeVisible();
    expect(bridge.world.state.folders[FOLDER].denied).toEqual([]);
    await user.click(within(box).getByRole("button", { name: "Deny" }));
    await closeResult(user);
    expect(bridge.world.state.folders[FOLDER].denied).toEqual([
      "plugin:ecc:chrome-devtools",
    ]);
    await user.click(await screen.findByRole("button", { name: "Allow again…" }));
    const back = await dialog(/^Allow chrome-devtools again\?$/);
    await user.click(within(back).getByRole("button", { name: "Allow again" }));
    await closeResult(user);
    await waitFor(() => expect(bridge.world.state.folders[FOLDER].denied).toEqual([]));
  });
});

const ECC = "ecc@ecc";
const OFF = `plugins off ecc@ecc --cwd ${FOLDER}`;
const ON = `plugins on ecc@ecc --cwd ${FOLDER}`;
const reseed = (seed: Parameters<typeof createBridge>[0]) => {
  bridge = createBridge(seed);
  invoke.mockReset().mockImplementation(bridge.invoke);
};
const offIn = (...cwds: string[]) =>
  Object.fromEntries(cwds.map((cwd) => [cwd, { env: {}, denied: [], off: [ECC] }]));
const eccHooks = (cwd: string) =>
  (bridge.world.reply(["hooks", "ls", "--cwd", cwd]) as HooksLsData).hooks.filter(
    (hook) => hook.owner.name === ECC,
  );

describe("plugins.off", () => {
  it("turns ecc off in the chosen folder after the plan: the folder reads off, nothing else changes, and its hooks leave Context > Hooks", async () => {
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    const region = await screen.findByRole("region", { name: "Plugin ecc" });
    expect(within(region).getByText("on here")).toBeVisible();
    expect(eccHooks(FOLDER).length).toBeGreaterThan(0);
    await user.click(
      within(region).getByRole("button", { name: "Turn off in a folder…" }),
    );
    const box = await dialog(/^Turn ecc off in this folder\?$/);
    expect(within(box).getByText(`Turn off ecc@ecc in ${FOLDER}`)).toBeVisible();
    expect(within(box).getByText(/goes away in this folder: 3 skills/)).toBeVisible();
    expect(within(box).getByText(/stays: your own skills/)).toBeVisible();
    expect(bridge.count(`${OFF} --dry-run`)).toBe(1);
    expect(bridge.count(OFF)).toBe(0);
    expect(bridge.world.state.folders[FOLDER]?.off ?? []).toEqual([]);
    await user.click(within(box).getByRole("button", { name: "Turn off" }));
    await screen.findByText(`Turned ecc off in ${FOLDER}`);
    expect(
      screen.getByText(`toolportctl plugins on ecc@ecc --cwd ${FOLDER}`),
    ).toBeVisible();
    await closeResult(user);
    expect(bridge.count(OFF)).toBe(1);
    expect(bridge.world.state.folders[FOLDER].off).toEqual([ECC]);
    expect(bridge.world.state.disabledUser).toEqual([]);
    const after = await screen.findByRole("region", { name: "Plugin ecc" });
    await waitFor(() => expect(within(after).getByText("off here")).toBeVisible());
    expect(
      within(screen.getByRole("list", { name: "Plugins" })).getByText("off here"),
    ).toBeVisible();
    expect(eccHooks(FOLDER)).toEqual([]);
    expect(eccHooks("/home/demo/work/other").length).toBeGreaterThan(0);
    cleanup();
    const hooks = await openHooks();
    await chooseFolder(hooks, "Show");
    await screen.findByRole("group", { name: "Hook counts" });
    expect(screen.queryAllByText(/run-with-flags\.js/)).toEqual([]);
  });

  it("applies nothing when the folder already has it off: the plan says so and offers no confirm", async () => {
    reseed({ folders: offIn(FOLDER) });
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    await screen.findByRole("region", { name: "Plugin ecc" });
    await user.click(
      await screen.findByRole("button", { name: "Turn off in a folder…" }),
    );
    const box = await dialog(/^Turn ecc off in this folder$/);
    expect(within(box).getByText(/Nothing to do: already turned off/)).toBeVisible();
    expect(within(box).queryByRole("button", { name: "Turn off" })).toBeNull();
    expect(bridge.count(OFF)).toBe(0);
  });

  it("shows a refusal of the preview as the CLI's words and applies nothing", async () => {
    bridge.set(
      `${OFF} --dry-run`,
      () => new CtlReplyFailure("not_found", "plugin 'ecc@ecc' is not installed"),
    );
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    await screen.findByRole("region", { name: "Plugin ecc" });
    await user.click(screen.getByRole("button", { name: "Turn off in a folder…" }));
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("not_found")).toBeVisible();
    expect(within(alert).getByText(/plugin 'ecc@ecc' is not installed/)).toBeVisible();
    expect(bridge.count(OFF)).toBe(0);
  });
});

describe("plugins.on", () => {
  it("turns ecc back on in the folder after the plan, and its hooks come back", async () => {
    reseed({ folders: offIn(FOLDER) });
    const user = await openPlugins();
    await chooseFolder(user, "Use folder");
    const region = await screen.findByRole("region", { name: "Plugin ecc" });
    expect(within(region).getByText("off here")).toBeVisible();
    expect(eccHooks(FOLDER)).toEqual([]);
    await user.click(
      within(region).getByRole("button", { name: "Turn back on in this folder…" }),
    );
    const box = await dialog(/^Turn ecc back on in this folder\?$/);
    expect(within(box).getByText(/comes back in this folder: 3 skills/)).toBeVisible();
    expect(bridge.count(ON)).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Turn on" }));
    await screen.findByText(`Turned ecc back on in ${FOLDER}`);
    expect(
      screen.getByText(`toolportctl plugins off ecc@ecc --cwd ${FOLDER}`),
    ).toBeVisible();
    await closeResult(user);
    expect(bridge.count(ON)).toBe(1);
    expect(bridge.world.state.folders[FOLDER].off).toEqual([]);
    expect(eccHooks(FOLDER).length).toBeGreaterThan(0);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Turn back on/ })).toBeNull(),
    );
  });
});

describe("plugins.disable", () => {
  it("asks for the plugin id, runs only plugins disable, and the plugin reads off everywhere afterwards", async () => {
    const user = await openPlugins();
    const opener = await screen.findByRole("button", { name: "Disable everywhere…" });
    await user.click(opener);
    const box = await dialog(/^Disable ecc everywhere\?$/);
    expect(
      within(box).getByText("claude plugin disable ecc@ecc --scope user"),
    ).toBeVisible();
    const confirm = within(box).getByRole("button", { name: "Disable" });
    expect(confirm).toBeDisabled();
    const field = within(box).getByRole("textbox");
    await user.type(field, "ecc");
    expect(confirm).toBeDisabled();
    await user.type(field, "@ecc");
    expect(confirm).toBeEnabled();
    expect(bridge.count("plugins disable ecc@ecc")).toBe(0);
    await user.click(confirm);
    await screen.findByText("Disabled ecc everywhere");
    expect(screen.getByText("toolportctl plugins enable ecc@ecc")).toBeVisible();
    await closeResult(user);
    expect(bridge.count("plugins disable ecc@ecc")).toBe(1);
    expect(bridge.world.state.disabledUser).toEqual([ECC]);
    expect(Object.values(bridge.world.state.folders).flatMap((f) => f.off)).toEqual([]);
    expect(
      await screen.findByRole("button", { name: "Enable everywhere…" }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Disable everywhere…" })).toBeNull();
    expect(
      within(screen.getByRole("list", { name: "Plugins" })).getByText("off here"),
    ).toBeVisible();
  });

  it("runs nothing when Escape dismisses the confirmation, and the focus returns to the button", async () => {
    const user = await openPlugins();
    const opener = await screen.findByRole("button", { name: "Disable everywhere…" });
    opener.focus();
    await user.keyboard("{Enter}");
    const box = await dialog(/^Disable ecc everywhere\?$/);
    await user.type(within(box).getByRole("textbox"), "ecc@ecc");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
    expect(bridge.count("plugins disable ecc@ecc")).toBe(0);
    expect(bridge.world.state.disabledUser).toEqual([]);
  });

  it("shows a missing claude as a conflict and applies nothing", async () => {
    bridge.set(
      "plugins disable ecc@ecc --dry-run",
      () =>
        new CtlReplyFailure(
          "conflict",
          "claude not found on PATH: `claude plugin disable ecc@ecc --scope user` needs it",
        ),
    );
    const user = await openPlugins();
    await user.click(await screen.findByRole("button", { name: "Disable everywhere…" }));
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("conflict")).toBeVisible();
    expect(within(alert).getByText(/claude not found on PATH/)).toBeVisible();
    expect(bridge.count("plugins disable ecc@ecc")).toBe(0);
  });
});

describe("plugins.enable", () => {
  it("enables the plugin again everywhere after the plan, without a typed confirmation", async () => {
    reseed({ disabledUser: [ECC] });
    const user = await openPlugins();
    expect(screen.queryByRole("button", { name: "Disable everywhere…" })).toBeNull();
    await user.click(await screen.findByRole("button", { name: "Enable everywhere…" }));
    const box = await dialog(/^Enable ecc everywhere\?$/);
    expect(within(box).queryByRole("textbox")).toBeNull();
    expect(
      within(box).getByText("claude plugin enable ecc@ecc --scope user"),
    ).toBeVisible();
    expect(bridge.count("plugins enable ecc@ecc")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Enable" }));
    await screen.findByText("Enabled ecc everywhere");
    await closeResult(user);
    expect(bridge.count("plugins enable ecc@ecc")).toBe(1);
    expect(bridge.world.state.disabledUser).toEqual([]);
    expect(
      await screen.findByRole("button", { name: "Disable everywhere…" }),
    ).toBeVisible();
  });
});

describe("system.cc-update", () => {
  it("previews the update first, applies it from Library > Plugins and shows the new version in System > Updates", async () => {
    const user = await openPlugins();
    await user.click(screen.getByRole("button", { name: "Update…" }));
    const box = await dialog(/^Update ecc\?$/);
    expect(bridge.count("cc update ecc --dry-run")).toBe(1);
    expect(bridge.count("cc update ecc")).toBe(0);
    expect(bridge.world.state.plugins[0].installed).toBe("2.1.0");
    await user.click(within(box).getByRole("button", { name: "Update" }));
    expect((await screen.findAllByText(/Restart Claude Code/)).length).toBeGreaterThan(0);
    expect(bridge.world.state.plugins[0]).toMatchObject({
      installed: "2.2.0",
      available: null,
    });
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    cleanup();
    await openUpdates();
    const row = within(screen.getByRole("list", { name: "Plugin updates" }));
    expect(row.getByText(/2\.2\.0/)).toBeVisible();
  });

  it("updates every plugin from System > Updates with a preview and a confirm", async () => {
    const user = await openUpdates();
    await user.click(screen.getByRole("button", { name: "Update all plugins…" }));
    const box = await dialog(/^Update all plugins\?$/);
    expect(bridge.count("cc update --dry-run")).toBe(1);
    await user.click(within(box).getByRole("button", { name: /^Update/ }));
    expect((await screen.findAllByText(/Restart Claude Code/)).length).toBeGreaterThan(0);
    expect(bridge.count("cc update")).toBe(1);
    expect(bridge.world.state.restartRequired).toBe(true);
  });
});

describe("system.cc-list", () => {
  it("lists the plugins Claude Code has installed in the Updates tab, with the update state", async () => {
    await openUpdates();
    const list = within(screen.getByRole("list", { name: "Plugin updates" }));
    expect(list.getByText("ecc")).toBeVisible();
    expect(list.getByText("demo-plugin")).toBeVisible();
    expect(bridge.count("cc list")).toBeGreaterThanOrEqual(1);
  });

  it("says why the card is empty when the plugin list cannot be read, and Retry reads again", async () => {
    bridge.set(
      "cc list",
      () => new CtlReplyFailure("bridge", "claude could not be started"),
    );
    const user = await openView("system", "Updates");
    expect(await screen.findByText("Couldn't read the plugins")).toBeVisible();
    bridge.set("cc list", () => bridge.world.reply(["cc", "list"]));
    await user.click(
      within(
        screen.getByText("Couldn't read the plugins").closest("div")!.parentElement!,
      ).getByRole("button", { name: /Retry/ }),
    );
    await screen.findByRole("list", { name: "Plugin updates" });
  });
});

describe("hooks.ls", () => {
  it("shows the hooks around each tool with their owner and switch, and changes after ecc settings are applied", async () => {
    const user = await openHooks();
    await chooseFolder(user, "Show");
    await screen.findByRole("group", { name: "Hook counts" });
    expect(
      within(screen.getByRole("group", { name: "Hook counts" })).getByText("12"),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Edit" }));
    const before = screen.getByRole("region", { name: "Before Edit runs" });
    expect(within(before).queryByText("off here")).toBeNull();
    cleanup();
    const plugins = await openPlugins();
    await applyGateGuardOff(plugins);
    cleanup();
    const again = await openHooks();
    await chooseFolder(again, "Show");
    await again.click(screen.getByRole("button", { name: "Edit" }));
    const after = screen.getByRole("region", { name: "Before Edit runs" });
    expect(within(after).getAllByText("off here").length).toBeGreaterThan(0);
    expect(
      within(screen.getByRole("group", { name: "Hook counts" })).getByText(
        /counted from|per answer|Per answer/i,
      ),
    ).toBeVisible();
  });

  it("shows an error with Retry when the hooks cannot be read", async () => {
    bridge.set(
      "hooks ls",
      () => new CtlReplyFailure("bridge", "the settings file could not be read"),
    );
    const user = await openView("context", "Hooks");
    expect(await screen.findByText(/settings file could not be read/)).toBeVisible();
    bridge.set("hooks ls", () => bridge.world.reply(["hooks", "ls"]));
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    await screen.findByRole("group", { name: "Hook counts" });
  });
});

describe("context.bundle.show", () => {
  it("shows the plugin settings and MCP denies of a profile as read-only rows", async () => {
    bridge.set("context bundle show acme-dev", () => {
      return {
        agents: { off: [] },
        appliedTo: [],
        bind: [],
        description: "",
        issues: [],
        layers: { add: [], exclude: [] },
        legacy: false,
        mcp: { deny: ["plugin:ecc:chrome-devtools"] },
        name: "acme-dev",
        path: "/home/demo/.config/toolport/bundles/acme-dev.yaml",
        plugins: {
          config: { "ecc@ecc": { hook_profile: "minimal", gateguard: "off" } },
          off: [],
        },
        servers: null,
        skills: { allow: [], nameOnly: [], off: [] },
        yaml: "format: 1\n",
      };
    });
    await openView("context", "Profiles");
    const region = await screen.findByRole("region", { name: "Profile acme-dev" });
    expect(within(region).getByText("Plugin settings")).toBeVisible();
    expect(within(region).getByText(/hook_profile/)).toBeVisible();
    expect(within(region).getByText("plugin:ecc:chrome-devtools")).toBeVisible();
    expect(
      bridge.ran().some((line) => /^context bundle (add|edit|apply)/.test(line)),
    ).toBe(false);
  });
});
