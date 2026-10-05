import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PluginsTab } from "./PluginsTab";
import {
  ccApply,
  ccPreview,
  createPluginsBridge,
  failure,
  FOLDER,
  golden,
  goldenFailure,
  lsArgv,
  plainShow,
  SENTINEL,
  showArgv,
  wire,
  type Bridge,
} from "./testkit";

let bridge: ReturnType<typeof createPluginsBridge>;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createPluginsBridge();
  wire({ invoke, listen }, bridge as Bridge);
});

const SET =
  "--set hook_profile=minimal --set gateguard=off --set gateguard_exempt_globs=docs/**,scripts/*.sh";
const CONFIG = `plugins config ecc@ecc --cwd ${FOLDER} ${SET}`;
const dialog = (name: RegExp) => screen.findByRole("dialog", { name });

async function open(folder = true) {
  const user = userEvent.setup();
  render(<PluginsTab />);
  await screen.findByRole("region", { name: "Plugin ecc" });
  if (folder) {
    await user.type(screen.getByLabelText("Folder"), FOLDER);
    await user.click(screen.getByRole("button", { name: "Use folder" }));
    await waitFor(() =>
      expect(bridge.count(showArgv("ecc@ecc", FOLDER))).toBeGreaterThan(0),
    );
    await screen.findByRole("region", { name: "Plugin ecc" });
  }
  return user;
}

async function fillSettings(user: ReturnType<typeof userEvent.setup>) {
  await user.selectOptions(screen.getByLabelText("Hook profile"), "minimal");
  await user.selectOptions(screen.getByLabelText("GateGuard"), "off");
  await user.type(
    screen.getByLabelText("GateGuard exempt paths"),
    "docs/**,scripts/*.sh",
  );
}

describe("Library > Plugins: reading", () => {
  it("lists the plugins and shows what the selected one brings", async () => {
    await open(false);
    const list = screen.getByRole("list", { name: "Plugins" });
    expect(within(list).getByText("ecc")).toBeInTheDocument();
    expect(within(list).getByText("up to date")).toBeInTheDocument();
    const brings = screen.getByRole("list", { name: "What it brings" });
    expect(within(brings).getByText("23 hooks")).toBeInTheDocument();
    expect(within(brings).getByText("2 MCP servers")).toBeInTheDocument();
    expect(bridge.ran()).toContain(lsArgv());
    expect(bridge.ran()).toContain(showArgv("ecc@ecc"));
  });

  it("labels the projected cost and the measured cost apart", async () => {
    await open(false);
    const dl = document.querySelector('dl[aria-label="Cost"]') as HTMLElement;
    expect(within(dl).getByText("Projected by Claude Code")).toBeInTheDocument();
    expect(within(dl).getByText("Measured by Toolport")).toBeInTheDocument();
    expect(within(dl).getByText(/overstates/)).toBeInTheDocument();
  });

  it("tells where the plugin is on, per scope", async () => {
    await open(false);
    const where = screen.getByRole("list", { name: "Where it is on" });
    expect(within(where).getByText("all your folders")).toBeInTheDocument();
    expect(within(where).getAllByText("not set").length).toBe(2);
  });

  it("explains a plugin without an adapter instead of showing switches", async () => {
    bridge.set(showArgv("ecc@ecc"), () => plainShow());
    render(<PluginsTab />);
    await screen.findByRole("region", { name: "Plugin demo-plugin" });
    expect(screen.getByText(/has no adapter/)).toBeInTheDocument();
    expect(screen.getByText(/no per-hook switch/)).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Plugin settings" })).toBeNull();
  });

  it("marks the plugin's servers as not governed", async () => {
    await open(false);
    const servers = screen.getByRole("list", { name: "MCP servers of the plugin" });
    expect(within(servers).getAllByText("not governed")).toHaveLength(2);
  });
});

describe("Library > Plugins: states", () => {
  it("shows an error with Retry when the list cannot be read", async () => {
    bridge.set(lsArgv(), failure("bridge", "claude could not be started"));
    const user = userEvent.setup();
    render(<PluginsTab />);
    expect(await screen.findByText(/claude could not be started/)).toBeInTheDocument();
    bridge.set(lsArgv(), () => golden("plugins-ls.measured"));
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    await screen.findByRole("list", { name: "Plugins" });
  });

  it("says so when no plugin is installed", async () => {
    bridge.set(lsArgv(), () => ({
      ...golden<object>("plugins-ls.measured"),
      plugins: [],
    }));
    render(<PluginsTab />);
    expect(await screen.findByText("No plugins installed")).toBeInTheDocument();
  });

  it("shows a loading state before the list arrives", async () => {
    bridge.set(lsArgv(), () => new Promise(() => {}));
    render(<PluginsTab />);
    expect(await screen.findByRole("status", { name: /Loading/ })).toBeInTheDocument();
  });

  it("notes an offline machine and a refresh that failed", async () => {
    vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
    bridge.set(lsArgv(), () => ({
      ...golden<object>("plugins-ls.measured"),
      refreshError: "marketplace unreachable",
    }));
    render(<PluginsTab />);
    expect(await screen.findByText(/You are offline/)).toBeInTheDocument();
    expect(await screen.findByText(/marketplace unreachable/)).toBeInTheDocument();
  });
});

describe("Library > Plugins: settings write", () => {
  it("applies settings to a folder only after the plan is confirmed, and undoes them", async () => {
    bridge.set(`${CONFIG} --dry-run`, () => golden("plugins-config.folder.plan"));
    bridge.set(CONFIG, () => golden("plugins-config.folder.apply"));
    const user = await open();
    await fillSettings(user);
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const box = await dialog(/^Apply ecc settings to a folder\?$/);
    expect(within(box).getByText(/Set 3 knob\(s\) of ecc@ecc/)).toBeInTheDocument();
    expect(within(box).getByText(/still starts a process/)).toBeInTheDocument();
    expect(bridge.count(`${CONFIG} --dry-run`)).toBe(1);
    expect(bridge.count(CONFIG)).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Apply" }));
    await screen.findByText("Done");
    expect(bridge.count(CONFIG)).toBe(1);
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("keeps Undo in a folder off while nothing is set in the folder", async () => {
    await open();
    expect(screen.getByRole("button", { name: "Undo in a folder…" })).toBeDisabled();
  });

  it("offers Undo in a folder for the keys set in that folder", async () => {
    const knobs = bridge.state.show.knobs.map((knob) =>
      knob.key === "gateguard"
        ? { ...knob, current: { value: "off", from: "folder-env" as const } }
        : knob,
    );
    bridge.state.show = { ...bridge.state.show, knobs };
    const undo = `plugins config ecc@ecc --cwd ${FOLDER} --unset gateguard`;
    bridge.set(`${undo} --dry-run`, () => golden("plugins-config.folder.unset-plan"));
    bridge.set(undo, () => golden("plugins-config.folder.unset"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Undo in a folder…" }));
    const box = await dialog(/^Undo ecc settings in a folder\?$/);
    await user.click(within(box).getByRole("button", { name: "Apply" }));
    await screen.findByText("Done");
    expect(bridge.count(undo)).toBe(1);
  });

  it("keeps Apply off without a folder and without a change", async () => {
    const user = await open(false);
    expect(screen.getByRole("button", { name: "Apply to a folder…" })).toBeDisabled();
    expect(screen.getAllByText(/Choose a folder above/).length).toBeGreaterThan(0);
    await user.selectOptions(screen.getByLabelText("Hook profile"), "minimal");
    expect(screen.getByRole("button", { name: "Apply to a folder…" })).toBeDisabled();
  });

  it("never applies when the preview fails and shows the refusal", async () => {
    bridge.set(`${CONFIG} --dry-run`, () => goldenFailure("plugins-config.bad-value"));
    bridge.set(CONFIG, () => golden("plugins-config.folder.apply"));
    const user = await open();
    await fillSettings(user);
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    expect(await screen.findByText(/hook_profile must be one of/)).toBeInTheDocument();
    expect(bridge.count(CONFIG)).toBe(0);
  });

  it("applies nothing when the dialog is dismissed with Escape, and returns the focus", async () => {
    bridge.set(`${CONFIG} --dry-run`, () => golden("plugins-config.folder.plan"));
    const user = await open();
    await fillSettings(user);
    const opener = screen.getByRole("button", { name: "Apply to a folder…" });
    await user.click(opener);
    const box = await dialog(/^Apply ecc settings to a folder\?$/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
    expect(bridge.count(CONFIG)).toBe(0);
  });
});

describe("Library > Plugins: servers, updates and turning off", () => {
  const DENY = `plugins mcp deny ecc@ecc chrome-devtools --cwd ${FOLDER}`;
  const ALLOW = `plugins mcp allow ecc@ecc chrome-devtools --cwd ${FOLDER}`;

  it("denies a plugin server in a folder through plan and confirm", async () => {
    bridge.set(`${DENY} --dry-run`, () => golden("plugins-mcp.deny.plan"));
    bridge.set(DENY, () => golden("plugins-mcp.deny.apply"));
    const user = await open();
    const servers = screen.getByRole("list", { name: "MCP servers of the plugin" });
    await user.click(
      within(servers).getAllByRole("button", { name: "Deny in a folder…" })[0],
    );
    const box = await dialog(/^Deny chrome-devtools in this folder\?$/);
    expect(within(box).getByText(/Deny plugin:ecc:chrome-devtools/)).toBeInTheDocument();
    expect(
      within(box).getByText(/mcp__plugin_ecc_chrome-devtools__/),
    ).toBeInTheDocument();
    expect(bridge.count(DENY)).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Deny" }));
    await screen.findByText("Done");
    expect(bridge.count(DENY)).toBe(1);
  });

  it("offers Allow again for a server denied in the folder", async () => {
    bridge.state.show = {
      ...bridge.state.show,
      mcpServers: bridge.state.show.mcpServers.map((s) =>
        s.name === "chrome-devtools" ? { ...s, denied: { ...s.denied, local: true } } : s,
      ),
    };
    bridge.set(`${ALLOW} --dry-run`, () => golden("plugins-mcp.allow.plan"));
    bridge.set(ALLOW, () => golden("plugins-mcp.allow.apply"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Allow again…" }));
    const box = await dialog(/^Allow chrome-devtools again\?$/);
    await user.click(within(box).getByRole("button", { name: "Allow again" }));
    await screen.findByText("Done");
    expect(bridge.count(ALLOW)).toBe(1);
  });

  it("updates a plugin through cc update after a preview", async () => {
    bridge.state.show = {
      ...bridge.state.show,
      update: { state: "update", available: "2.3.0" },
    };
    bridge.set("cc update ecc --dry-run", () => ccPreview());
    bridge.set("cc update ecc", () => ccApply());
    const user = await open(false);
    await user.click(screen.getByRole("button", { name: "Update…" }));
    const box = await dialog(/^Update ecc\?$/);
    expect(within(box).getByText(/demo-plugin@fake-market: 1.0.0/)).toBeInTheDocument();
    expect(bridge.count("cc update ecc")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Update" }));
    await screen.findByText(/Restart Claude Code/);
    expect(bridge.count("cc update ecc")).toBe(1);
  });

  it("keeps Update off for a plugin that is current", async () => {
    await open(false);
    expect(screen.getByRole("button", { name: "Update…" })).toBeDisabled();
  });

  it("shows the command that turns a plugin off in a folder, and Escape closes it", async () => {
    const user = await open();
    const opener = screen.getByRole("button", { name: "Turn off in a folder…" });
    await user.click(opener);
    const box = await dialog(/^Turn ecc off in a folder$/);
    expect(within(box).getByLabelText("Command line")).toHaveTextContent(
      "claude plugin disable ecc@ecc --scope local",
    );
    expect(within(box).getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("shows the command that disables a plugin everywhere", async () => {
    const user = await open(false);
    await user.click(screen.getByRole("button", { name: "Disable everywhere…" }));
    const box = await dialog(/^Disable ecc everywhere$/);
    expect(within(box).getByLabelText("Command line")).toHaveTextContent("--scope user");
  });
});

describe("Library > Plugins: leak canary", () => {
  it("never puts a sensitive option value in the page or in an argv", async () => {
    bridge.state.show = {
      ...bridge.state.show,
      options: bridge.state.show.options.map((o) =>
        o.sensitive ? { ...o, current: SENTINEL, configured: true } : o,
      ),
    };
    await open();
    expect(document.body.textContent).not.toContain(SENTINEL);
    expect(bridge.ran().join("\n")).not.toContain(SENTINEL);
    expect(screen.getAllByText("sensitive").length).toBeGreaterThan(0);
  });
});
