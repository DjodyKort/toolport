import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { openFromLibrary, write } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Agents tab walked the way a person uses it, against a world that changes: a sync
 * writes the outputs, a clean removes them and keeps the lockfile, an uninstall removes the
 * agent. Each test is named by the parity action it proves (`src/plus/gui-parity.json`). */
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
      (line) => !/^(commands|agents|styles|mcp call (agents|styles)_\w+)( |$)/.test(line),
    );
  expect(stray, "the tab only runs its own commands").toEqual([]);
  expect(
    bridge
      .ran()
      .some(
        (line) => !line.startsWith("mcp call ") && /secret|--reveal|stdin/.test(line),
      ),
  ).toBe(false);
});

const open = async () => {
  const user = await openFromLibrary("Agents");
  await screen.findByRole("list", { name: "Output of scout per client" });
  return user;
};
const group = (name: string) => screen.getByRole("group", { name });
const clientRows = () =>
  within(screen.getByRole("list", { name: "Output of scout per client" })).getAllByRole(
    "listitem",
  );

const synced = (user: UserEvent) =>
  write(user, "Sync…", "Sync", { done: "Wrote 1 agent to 4 clients" });
const cleaned = (user: UserEvent) =>
  write(user, "Clean outputs…", "Remove", {
    typed: "clean agents",
    done: "Removed 4 synced agent files",
  });

describe("Agents tab, end to end", () => {
  it("agents.list: shows scout with its model, tools, file and a row per client", async () => {
    await open();
    expect(screen.getByRole("heading", { name: /^Agents/ })).toBeInTheDocument();
    expect(screen.getByText("sonnet")).toBeInTheDocument();
    expect(screen.getByText("Read")).toBeInTheDocument();
    expect(screen.getByText("Grep")).toBeInTheDocument();
    expect(screen.getByText("/fixture/skills-repo/agents/scout/AGENT.md")).toBeVisible();
    const rows = clientRows();
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("Claude Code"),
      expect.stringContaining("Codex CLI"),
      expect.stringContaining("Cursor"),
      expect.stringContaining("Gemini CLI"),
    ]);
    expect(rows[1]).toHaveTextContent(/tools is dropped for Codex CLI/);
    expect(rows[2]).toHaveTextContent(/tools is dropped for Cursor/);
    expect(within(rows[0]).queryByText(/dropped/)).toBeNull();
    expect(within(rows[3]).queryByText(/dropped/)).toBeNull();
    expect(bridge.missing).toEqual([]);
  });

  it("agents.list: says so when toolportctl cannot run, and recovers on Retry", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    const user = await openFromLibrary("Agents");
    const failed = (await screen.findByText("Couldn't list agents")).closest(
      '[role="alert"]',
    ) as HTMLElement;
    expect(failed).toHaveTextContent(/toolportctl could not be started/);
    expect(screen.queryByRole("list", { name: "Output of scout per client" })).toBeNull();
    down = false;
    await user.click(within(failed).getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("list", { name: "Output of scout per client" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    expect(await screen.findByText("Write 1 agent to 4 clients")).toBeVisible();
    expect(screen.queryByText(/has not loaded yet/)).toBeNull();
  });

  it("agents.lint, agents.audit: both checks read without writing anything", async () => {
    await open();
    expect(
      await within(group("Lint")).findByText("No problems in your agents"),
    ).toBeVisible();
    expect(
      await within(group("Audit")).findByText("No risky instructions found in 1 agent"),
    ).toBeVisible();
    expect(bridge.count("agents lint")).toBe(1);
    expect(bridge.count("agents audit")).toBe(1);
    expect(bridge.ran().filter((line) => line.startsWith("agents sync"))).toEqual([
      "agents sync --dry-run",
    ]);
  });

  it("agents.diff: says every agent is new before the first sync, and clean after it", async () => {
    const user = await open();
    const diff = () => group("Changes since last sync");
    expect(
      await within(diff()).findByText("Never synced: every agent is new"),
    ).toBeVisible();
    expect(within(diff()).getByText("scout")).toBeVisible();
    await synced(user);
    expect(
      await within(diff()).findByText("No changes since the last sync"),
    ).toBeVisible();
    expect(within(diff()).getByText("1 unchanged")).toBeVisible();
  });

  it("agents.status: follows the outputs from not written, to in sync, to missing", async () => {
    const user = await open();
    const drift = () => group("Drift");
    expect(
      await within(drift()).findByText("Not synced yet. Sync writes the outputs."),
    ).toBeVisible();
    expect(screen.getAllByText("Not written yet")).toHaveLength(4);
    await synced(user);
    expect(
      await within(drift()).findByText("All 1 synced agent still in place"),
    ).toBeVisible();
    expect(await screen.findAllByText("In sync")).toHaveLength(4);
    await cleaned(user);
    expect(
      await within(drift()).findByText("4 outputs missing since the last sync"),
    ).toBeVisible();
    expect(within(drift()).getAllByText("scout")).toHaveLength(4);
    expect(await screen.findAllByText("Missing")).toHaveLength(4);
    expect(screen.queryByText("In sync")).toBeNull();
  });

  it("agents.sync: previews first with the dropped fields, then writes all four outputs", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const box = await screen.findByRole("dialog");
    expect(await within(box).findByText("Write 1 agent to 4 clients")).toBeVisible();
    expect(
      within(box).getByText(/scout: cursor: 'tools' field not supported, dropped/),
    ).toBeVisible();
    expect(bridge.count("agents sync --dry-run")).toBeGreaterThan(0);
    expect(bridge.count("agents sync")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText("Wrote 1 agent to 4 clients");
    expect(bridge.count("agents sync")).toBe(1);
    expect(bridge.ran().some((line) => /--home|--path/.test(line))).toBe(false);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findAllByText("In sync")).toHaveLength(4);
  });

  it("agents.sync: is reachable from the keyboard and Escape cancels without writing", async () => {
    const user = await open();
    const sync = screen.getByRole("button", { name: "Sync…" });
    sync.focus();
    await user.keyboard("{Enter}");
    const box = await screen.findByRole("dialog");
    await within(box).findByText("Write 1 agent to 4 clients");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count("agents sync")).toBe(0);
    expect(sync).toHaveFocus();
  });

  it("agents.clean: needs the typed phrase, removes the outputs, keeps the agent", async () => {
    const user = await open();
    await synced(user);
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    const box = await screen.findByRole("dialog");
    expect(await within(box).findByText("Remove 4 synced agent files")).toBeVisible();
    expect(within(box).getByText("/fixture/home/.codex/agents/scout.toml")).toBeVisible();
    const confirm = within(box).getByRole("button", { name: "Remove" });
    await user.type(within(box).getByRole("textbox"), "clean agent");
    expect(confirm).toBeDisabled();
    expect(bridge.count("agents clean")).toBe(0);
    await user.type(within(box).getByRole("textbox"), "s");
    await user.click(confirm);
    await screen.findByText("Removed 4 synced agent files");
    expect(bridge.count("agents clean")).toBe(1);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findAllByText("Missing")).toHaveLength(4);
    expect(
      screen.getByRole("list", { name: "Output of scout per client" }),
    ).toBeVisible();
  });

  it("agents.uninstall: the agent's name is the phrase, then the empty state offers a new one", async () => {
    const user = await open();
    await write(user, "Uninstall scout", "Uninstall", {
      typed: "scout",
      done: "Removed the agent 'scout' and its 0 outputs",
    });
    expect(await screen.findByText("No agents yet")).toBeVisible();
    expect(screen.getByRole("button", { name: "Create your first agent" })).toBeVisible();
    expect(bridge.count("agents uninstall scout")).toBe(1);
  });

  it("agents.add: creates reviewer from the template after a preview, then lists it as new", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "New agent…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByRole("textbox"), "reviewer");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Create agent reviewer/ });
    expect(await within(box).findByText(/Create the agent 'reviewer'/)).toBeVisible();
    expect(
      within(box).getByText("/fixture/skills-repo/agents/reviewer/AGENT.md"),
    ).toBeVisible();
    expect(bridge.count("agents add reviewer")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText("Created the agent 'reviewer' from the template");
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(
      await screen.findByRole("list", { name: "Output of reviewer per client" }),
    ).toBeVisible();
    expect(
      await within(group("Changes since last sync")).findByText(
        "Never synced: every agent is new",
      ),
    ).toBeVisible();
    expect(bridge.count("agents add reviewer")).toBe(1);
  });
});
