import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, screen, waitFor, within } from "@testing-library/react";

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
    ran.some((line) => /--home|--data-dir|secret|--reveal|stdin/.test(line)),
    "no secret flag",
  ).toBe(false);
  expect(
    ran.some((line) => /^plugins (enable|disable|on|off)\b|^claude /.test(line)),
    "the terminal-only steps never run from the app",
  ).toBe(false);
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
