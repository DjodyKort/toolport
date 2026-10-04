import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { View } from "@/lib/types";
import { commandsFixture } from "./fixtures/commandsRegistry";
import { PLUS_SCREENS, PLUS_VIEWS, type PlusView } from "./nav";
import { NOT_BUILT_TABS } from "./notBuiltTabs";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "./PlusViews";
import { createBridge } from "./servers/testkit";

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
  it.each(PLUS_VIEWS.filter((view) => view !== "commands" && view !== "control"))(
    "marks %s as not built yet and names the item that builds it",
    async (view) => {
      render(<Harness start={view} />);
      const screenInfo = PLUS_SCREENS[view];
      const first = NOT_BUILT_TABS[view]?.[0];
      expect(await screen.findByText("Not built yet")).toBeInTheDocument();
      expect(
        screen.getByText(
          new RegExp(`built by ${first?.builtBy ?? screenInfo.builtBy}\\b`),
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Open All commands" }),
      ).toBeInTheDocument();
    },
  );

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

  it("shows the tabs of the mockup and a placeholder per tab", async () => {
    const user = userEvent.setup();
    render(<Harness start="library" />);
    const tabs = await screen.findByRole("tablist", { name: "Library sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Skills", "Agents", "Styles", "Plugins", "Sources"]);
    expect(screen.getByText(/built by MIG-GUI-3\b/)).toBeInTheDocument();
    await user.click(within(tabs).getByRole("tab", { name: "Plugins" }));
    expect(screen.getByText(/built by MIG-GUI-12\b/)).toBeInTheDocument();
  });

  it("opens the All commands page on the group of the tab it was left from", async () => {
    const user = userEvent.setup();
    render(<Harness start="system" />);
    await user.click(await screen.findByRole("tab", { name: "Council" }));
    await user.click(screen.getByRole("button", { name: "Open All commands" }));
    expect(screen.getByLabelText("view")).toHaveTextContent("commands");
    const list = await screen.findByRole("list", { name: "Commands" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByRole("combobox", { name: "Group" })).toHaveTextContent(
      "council (1)",
    );
  });
});
