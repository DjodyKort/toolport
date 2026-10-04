import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { SystemScreen } from "./SystemScreen";
import { createBridge, wire, type Bridge } from "./testkit";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

describe("System screen", () => {
  it("has the five tabs of the mockup and opens on Sync", async () => {
    render(<SystemScreen onOpenCommands={() => {}} />);
    const tabs = await screen.findByRole("tablist", { name: "System sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Sync", "Updates", "Council", "Import", "Self-management"]);
    expect(within(tabs).getByRole("tab", { name: "Sync" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByText("Not set up")).toBeInTheDocument();
  });

  it("moves between the tabs with the arrow keys, Home and End, and shows each panel", async () => {
    const user = userEvent.setup();
    render(<SystemScreen onOpenCommands={() => {}} />);
    const tabs = await screen.findByRole("tablist", { name: "System sections" });
    within(tabs).getByRole("tab", { name: "Sync" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(within(tabs).getByRole("tab", { name: "Updates" })).toHaveFocus();
    expect(await screen.findByText("srv-alpha")).toBeInTheDocument();
    await user.keyboard("{ArrowRight}");
    expect(await screen.findByRole("group", { name: "Doctor" })).toBeInTheDocument();
    await user.keyboard("{ArrowRight}");
    expect(
      await screen.findByRole("group", { name: "Import from mcpm" }),
    ).toBeInTheDocument();
    await user.keyboard("{End}");
    expect(within(tabs).getByRole("tab", { name: "Self-management" })).toHaveFocus();
    expect(
      await screen.findByRole("group", { name: "Tool catalogue" }),
    ).toBeInTheDocument();
    await user.keyboard("{Home}");
    expect(within(tabs).getByRole("tab", { name: "Sync" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("opens on the tab it is given", async () => {
    render(<SystemScreen initialTab="council" onOpenCommands={() => {}} />);
    expect(
      await screen.findByRole("tab", { name: "Council", selected: true }),
    ).toBeInTheDocument();
  });

  it("never runs a command a panel did not ask for while only reading", async () => {
    render(<SystemScreen onOpenCommands={() => {}} />);
    await screen.findByText("Not set up");
    const writes = bridge
      .ran()
      .filter((line) => !/^(commands|sync (status|diff|git-sync --status))$/.test(line));
    expect(writes).toEqual([]);
  });
});
