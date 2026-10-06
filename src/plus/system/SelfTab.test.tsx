import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { open, write } from "./harness";
import { SelfTab } from "./SelfTab";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const installedDoctor = () => ({
  state: "enabled",
  activeProfile: { id: "default", enabled: true, optedOut: false },
  clientProfiles: [{ id: "work", enabled: true, optedOut: false }],
  checks: golden("mcp-doctor").checks.map((check: object) => ({ ...check, ok: true })),
});

describe("Self-management tab: reading", () => {
  it("says the server is not installed, with the catalogue size and the doctor checks", async () => {
    await open(<SelfTab />, bridge);
    expect(
      await screen.findByText("Not installed", { selector: "p" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("101 tools, 11 resources", { selector: "dd" }),
    ).toBeInTheDocument();
    const checks = screen.getByRole("list", { name: "Self-management checks" });
    expect(within(checks).getAllByRole("listitem")).toHaveLength(7);
    expect(within(checks).getAllByText("Fix")).toHaveLength(3);
    expect(within(checks).getByText("101 tools, 11 resources")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install…" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Turn off…" })).toBeNull();
  });

  it("shows the state and the profiles it is enabled in once it is installed", async () => {
    bridge.set("mcp doctor", installedDoctor());
    await open(<SelfTab />, bridge);
    expect(await screen.findByText("Enabled")).toBeInTheDocument();
    const state = within(screen.getByRole("group", { name: "Self-management MCP" }));
    expect(state.getByText("default")).toBeInTheDocument();
    expect(state.getByText("work")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Turn off…" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Enable in a profile…" }),
    ).toBeInTheDocument();
  });

  it("explains the tiers and the confirmation a tool needs", async () => {
    await open(<SelfTab />, bridge);
    const tiers = await screen.findByRole("group", { name: "Tiers" });
    expect(within(tiers).getByText("Read-only")).toBeInTheDocument();
    expect(within(tiers).getByText("Destructive")).toBeInTheDocument();
    const list = await screen.findByRole("list", { name: "Tools" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(101);
    expect(
      within(list).getAllByText("Needs confirmation every time").length,
    ).toBeGreaterThan(0);
    expect(
      within(list).getAllByText("Needs confirmation unless it is a dry run").length,
    ).toBeGreaterThan(0);
  });

  it("filters the catalogue by text and by tier", async () => {
    const { user } = await open(<SelfTab />, bridge);
    const list = await screen.findByRole("list", { name: "Tools" });
    await user.type(screen.getByLabelText("Filter tools"), "skills_list");
    expect(within(list).getAllByRole("listitem").length).toBeLessThan(5);
    expect(within(list).getByText("skills_list")).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Filter tools"));
    await user.selectOptions(screen.getByLabelText("Tier"), "Destructive");
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(10);
    for (const row of rows)
      expect(within(row).getByText("Destructive")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Filter tools"), "no-such-tool-name");
    expect(await screen.findByText("No tool matches the filter.")).toBeInTheDocument();
  });

  it("lists the resources", async () => {
    await open(<SelfTab />, bridge);
    const resources = await screen.findByRole("list", { name: "Resources" });
    expect(within(resources).getAllByRole("listitem")).toHaveLength(11);
    expect(within(resources).getByText("mcpm://router/status")).toBeInTheDocument();
  });

  it("tells a client how to connect, with the command the doctor found", async () => {
    await open(<SelfTab />, bridge);
    const card = await screen.findByRole("group", { name: "Connect a client" });
    expect(within(card).getByLabelText("Client configuration")).toHaveTextContent(
      '"toolport-plus-self"',
    );
    expect(within(card).getByLabelText("Client configuration")).toHaveTextContent(
      '"command": "<BIN>/toolport-selfmcp"',
    );
    expect(
      within(card).getByText(
        "claude mcp add toolport-plus-self -- <BIN>/toolport-selfmcp",
      ),
    ).toBeInTheDocument();
    expect(
      within(card).getByRole("button", { name: "Copy configuration" }),
    ).toBeEnabled();
  });

  it("shows the failures with Retry and no duplicate message for the doctor", async () => {
    bridge.set("mcp doctor", failure("mcp", "the registry is locked"));
    bridge.set("mcp tools", failure("mcp", "the catalogue is unavailable"));
    await open(<SelfTab />, bridge);
    expect(await screen.findAllByText("the registry is locked")).toHaveLength(1);
    expect(
      (await screen.findAllByText("the catalogue is unavailable")).length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
  });

  it("shows a skeleton while the doctor runs", async () => {
    bridge.set("mcp doctor", () => new Promise(() => {}));
    await open(<SelfTab />, bridge);
    expect(screen.getAllByRole("status", { name: "Loading" }).length).toBeGreaterThan(0);
  });
});

describe("Self-management tab: install and turn off", () => {
  it("installs with a confirmation that says there is no preview", async () => {
    bridge.set("mcp install", golden("mcp-install.apply"));
    const { user } = await open(<SelfTab />, bridge);
    await screen.findByText("101 tools, 11 resources", { selector: "dd" });
    await user.click(screen.getByRole("button", { name: "Install…" }));
    const box = await screen.findByRole("dialog");
    expect(
      within(box).getByText(/Register the self-management server$/),
    ).toBeInTheDocument();
    expect(within(box).getByText(/no preview/)).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Install" }));
    expect(
      await screen.findByText("Self-management server installed"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("mcp install");
  });

  it("enables it in a named profile", async () => {
    bridge.set("mcp install --profile default", golden("mcp-install.profile"));
    const { user } = await open(<SelfTab />, bridge);
    await screen.findByText("101 tools, 11 resources", { selector: "dd" });
    await user.type(screen.getByLabelText("Profile (optional)"), "default");
    await write(user, "Install…", "Install", {
      plan: /Register the self-management server and enable it in default/,
      done: "Self-management server installed",
    });
    expect(bridge.ran()).toContain("mcp install --profile default");
  });

  it("turns it off only after a typed confirmation", async () => {
    bridge.set("mcp doctor", installedDoctor());
    bridge.set("mcp uninstall", golden("mcp-uninstall.apply"));
    const { user } = await open(<SelfTab />, bridge);
    await screen.findByText("Enabled");
    await write(user, "Turn off…", "Turn off", {
      plan: "Remove the self-management server and keep it removed",
      typed: "mcp uninstall",
      done: "Self-management server turned off",
    });
    expect(bridge.ran()).toContain("mcp uninstall");
  });

  it("runs the doctor again on request", async () => {
    const { user } = await open(<SelfTab />, bridge);
    await screen.findByText("101 tools, 11 resources", { selector: "dd" });
    await user.click(
      screen.getByRole("button", { name: "Run the self-management doctor again" }),
    );
    await screen.findByText("101 tools, 11 resources", { selector: "dd" });
    expect(bridge.count("mcp doctor")).toBe(2);
  });
});
