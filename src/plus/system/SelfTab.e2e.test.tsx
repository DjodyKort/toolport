import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { button, confirm, group, openSystem } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Self-management tab walked the way a person uses it: the state and its doctor, the
 * install into a profile, the typed turn-off, the tool catalogue and the way to connect a
 * client. Each test is named by the parity action it proves (`src/plus/gui-parity.json`) and
 * asserts that the next read changed. */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter(
      (line) =>
        !/^(commands|sync|update|council|mcp|import|secret set council)( |$)/.test(line),
    );
  expect(stray, "the screen only runs its own command groups").toEqual([]);
});

const checks = () =>
  within(screen.getByRole("list", { name: "Self-management checks" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent)
    .join("|");

describe("Self-management tab, end to end", () => {
  it("system.mcp-doctor, system.mcp-tools: a server that is not installed fails its first check in view, and the catalogue lists every tool with its tier", async () => {
    await openSystem(bridge, "Self-management");
    expect(await screen.findAllByText("Not installed")).not.toHaveLength(0);
    await screen.findByRole("list", { name: "Self-management checks" });
    expect(checks()).toMatch(/FixRegistry entryrun `toolportctl mcp install`/);
    expect(checks()).toMatch(/OKCatalogue100 tools, 11 resources/);
    const tools = await screen.findByRole("list", { name: "Tools" });
    expect(within(tools).getAllByRole("listitem").length).toBeGreaterThan(50);
    expect(within(tools).getAllByText("Destructive").length).toBeGreaterThan(0);
    expect(await screen.findByRole("list", { name: "Resources" })).toBeVisible();
    expect(bridge.count("mcp doctor")).toBe(1);
    expect(bridge.count("mcp tools")).toBe(1);
  });

  it("system.mcp-install: the install into a profile is confirmed and the state, the profiles and the doctor change", async () => {
    const user = await openSystem(bridge, "Self-management");
    await user.type(await screen.findByLabelText("Profile (optional)"), "work");
    await user.click(button("Install…"));
    const box = await screen.findByRole("dialog", {
      name: "Install the self-management server?",
    });
    expect(within(box).getByText("Enable it in work")).toBeVisible();
    expect(bridge.world!.state.self.state).toBe("missing");
    await confirm(user, "Install", { done: "Self-management server installed" });
    expect(bridge.ran()).toContain("mcp install --profile work");
    expect(await screen.findByText("Enabled")).toBeVisible();
    expect(group("Self-management MCP").getByText("work")).toBeVisible();
    await waitFor(() => expect(checks()).toMatch(/OKRegistry entry/));
    expect(
      await screen.findByRole("button", { name: "Enable in a profile…" }),
    ).toBeVisible();
  });

  it("system.mcp-uninstall: Escape cancels and returns focus, the typed turn-off changes the state", async () => {
    bridge.world!.state.self = { state: "enabled", profiles: ["default"] };
    const user = await openSystem(bridge, "Self-management");
    const opener = await screen.findByRole("button", { name: "Turn off…" });
    await user.click(opener);
    const box = await screen.findByRole("dialog", {
      name: "Turn off the self-management server?",
    });
    expect(within(box).getByText(/Agents that use it lose the tools/)).toBeVisible();
    expect(within(box).getByRole("button", { name: "Turn off" })).toBeDisabled();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();
    expect(bridge.count("mcp uninstall")).toBe(0);

    await user.click(opener);
    await confirm(user, "Turn off", {
      typed: "mcp uninstall",
      done: "Self-management server turned off",
    });
    expect(bridge.world!.state.self).toEqual({ state: "opted-out", profiles: [] });
    expect(await screen.findByText("Turned off")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Turn off…" })).toBeNull();
  });

  it("system.mcp-doctor: the client snippet names the server and the command the doctor found", async () => {
    await openSystem(bridge, "Self-management");
    const snippet = await screen.findByLabelText("Client configuration");
    expect(snippet).toHaveTextContent("toolport-plus-self");
    expect(snippet).toHaveTextContent("/fixture/bin/toolport-selfmcp");
    expect(screen.getByRole("button", { name: "Copy command" })).toBeVisible();
  });
});
