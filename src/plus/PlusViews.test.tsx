import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { View } from "@/lib/types";
import { commandsFixture } from "./fixtures/commandsRegistry";
import { PLUS_VIEWS, type PlusView } from "./nav";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));
vi.mock("./plugins/PluginsTab", () => ({ PluginsTab: () => <p>Plugins panel</p> }));
vi.mock("./skills/SkillsTab", () => ({ SkillsTab: () => <p>Skills panel</p> }));
vi.mock("./compression/CompressionTab", () => ({
  CompressionTab: () => <p>Compression panel</p>,
}));
vi.mock("./usage/UsageTab", () => ({ UsageTab: () => <p>Usage panel</p> }));

import { PlusViews } from "./PlusViews";
import { createBridge } from "./servers/testkit";
import { createBridge as createSystemBridge } from "./system/testkit";
import { createBridge as createAttentionBridge } from "./attention/testkit";
import { ServersScreen } from "./servers/ServersScreen";

beforeEach(() => {
  listen.mockReset().mockResolvedValue(() => {});
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) => {
    if (command === "plus_ctl") return "job-1";
    if (command === "plus_ctl_result")
      return {
        job: "job-1",
        exitCode: 0,
        signal: null,
        cancelled: false,
        envelope: {
          ok: true,
          command: "commands",
          schemaVersion: 1,
          data: commandsFixture,
        },
        parseError: null,
        stderr: [],
        truncated: false,
      };
    throw new Error(`unexpected invoke ${command}`);
  });
});

function Harness({ start }: { start: PlusView }) {
  const [view, setView] = useState<View>(start);
  const plus = PLUS_VIEWS.find((candidate) => candidate === view);
  return (
    <>
      <output aria-label="view">{view}</output>
      {plus ? <PlusViews view={plus} onSelectView={setView} /> : <p>upstream</p>}
    </>
  );
}

describe("PlusViews", () => {
  it("opens the Servers screen on the control view and the classic page from it", async () => {
    invoke.mockReset().mockImplementation(createBridge().invoke);
    const user = userEvent.setup();
    render(<Harness start="control" />);
    expect(
      await screen.findByRole("tablist", { name: "Servers sections" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Classic view" }));
    expect(screen.getByLabelText("view")).toHaveTextContent("servers");
    expect(screen.getByText("upstream")).toBeInTheDocument();
  });

  it("shows the tabs of the mockup and the Skills panel", async () => {
    const user = userEvent.setup();
    render(<Harness start="library" />);
    const tabs = await screen.findByRole("tablist", { name: "Library sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Skills", "Agents", "Styles", "Plugins", "Sources"]);
    expect(await screen.findByText("Skills panel")).toBeInTheDocument();
    await user.click(within(tabs).getByRole("tab", { name: "Plugins" }));
    expect(await screen.findByText("Plugins panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
  });

  it("shows the Tokens screen with the Usage and Compression panels", async () => {
    const user = userEvent.setup();
    render(<Harness start="tokens" />);
    const tabs = await screen.findByRole("tablist", { name: "Tokens sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Usage", "Compression"]);
    expect(await screen.findByText("Usage panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
    await user.click(within(tabs).getByRole("tab", { name: "Compression" }));
    expect(await screen.findByText("Compression panel")).toBeInTheDocument();
  });

  it("opens the Attention screen from the sidebar view and the All commands page from its offline state", async () => {
    const bridge = createAttentionBridge();
    bridge.down = true;
    invoke.mockReset().mockImplementation(bridge.invoke);
    const user = userEvent.setup();
    render(<Harness start="attention" />);
    expect(await screen.findByText("Toolport can't run toolportctl")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
    bridge.down = false;
    await user.click(screen.getByRole("button", { name: "Open doctor" }));
    expect(screen.getByLabelText("view")).toHaveTextContent("commands");
    expect(await screen.findByRole("list", { name: "Commands" })).toBeInTheDocument();
  });

  it("opens the Servers screen on the tab a link names", async () => {
    invoke.mockReset().mockImplementation(createBridge().invoke);
    render(<ServersScreen initialTab="secrets" onOpenCommands={() => {}} pollMs={0} />);
    const tabs = await screen.findByRole("tablist", { name: "Servers sections" });
    expect(within(tabs).getByRole("tab", { name: "Secrets" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("opens the System screen with its five tabs and the plugin updates", async () => {
    invoke.mockReset().mockImplementation(createSystemBridge().invoke);
    const user = userEvent.setup();
    render(<Harness start="system" />);
    const tabs = await screen.findByRole("tablist", { name: "System sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Sync", "Updates", "Council", "Import", "Self-management"]);
    await user.click(within(tabs).getByRole("tab", { name: "Updates" }));
    expect(
      await screen.findByRole("group", { name: "Claude Code plugins" }),
    ).toBeInTheDocument();
  });
});
