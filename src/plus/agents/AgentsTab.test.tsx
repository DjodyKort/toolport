import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { AgentsTab } from "./AgentsTab";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

async function open() {
  const user = userEvent.setup();
  render(<AgentsTab />);
  await screen.findByRole("list", { name: "Output of scout per client" });
  return user;
}

const outputList = () => screen.getByRole("list", { name: "Output of scout per client" });
const rowOf = (client: string) =>
  within(outputList())
    .getAllByRole("listitem")
    .find((li) => within(li).queryByText(client))!;

describe("Agents tab: reading", () => {
  it("shows the agent with its model, tools and file", async () => {
    await open();
    expect(screen.getByText("sonnet")).toBeInTheDocument();
    expect(screen.getByText("Read")).toBeInTheDocument();
    expect(screen.getByText("Grep")).toBeInTheDocument();
    expect(screen.getByText(/Reads a repository/)).toBeInTheDocument();
    expect(
      screen.getByText("/fixture/skills-repo/agents/scout/AGENT.md"),
    ).toBeInTheDocument();
  });

  it("shows one agent in four clients with the dropped tools of codex and cursor", async () => {
    await open();
    const rows = within(outputList()).getAllByRole("listitem");
    expect(rows).toHaveLength(4);
    expect(
      rows.map(
        (li) => within(li).getAllByText(/Claude|Codex|Cursor|Gemini/)[0].textContent,
      ),
    ).toEqual(["Claude Code", "Codex CLI", "Cursor", "Gemini CLI"]);
    expect(
      within(rowOf("Codex CLI")).getByText(/is dropped for Codex CLI/),
    ).toBeInTheDocument();
    expect(
      within(rowOf("Codex CLI")).getByText(/\.codex\/agents\/scout\.toml/),
    ).toBeInTheDocument();
    expect(
      within(rowOf("Cursor")).getByText(/is dropped for Cursor/),
    ).toBeInTheDocument();
    expect(within(rowOf("Claude Code")).queryByText(/dropped/)).toBeNull();
    expect(within(rowOf("Gemini CLI")).queryByText(/dropped/)).toBeNull();
  });

  it("says the outputs are not written before the first sync, and writes nothing to read", async () => {
    await open();
    expect(within(rowOf("Cursor")).getByText("Not written yet")).toBeInTheDocument();
    await screen.findByText("Never synced: every agent is new");
    await screen.findByText("Not synced yet. Sync writes the outputs.");
    const writes = bridge.ran().filter((line) => !line.includes("--dry-run"));
    expect(writes.sort()).toEqual([
      "agents audit",
      "agents diff",
      "agents lint",
      "agents ls",
      "agents status",
      "commands",
      "mcp call agents_list_transpilers --args-stdin",
    ]);
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
  });

  it("marks each output in sync or missing once a lockfile exists", async () => {
    bridge.set("agents status", {
      drift: true,
      lockedCount: 1,
      lockfilePresent: true,
      outputRoot: "/fixture/home",
      outputs: [
        { name: "scout", client: "claude-code", present: true },
        { name: "scout", client: "codex-cli", present: false },
      ],
      repo: "/fixture/skills-repo",
    });
    await open();
    await waitFor(() =>
      expect(within(rowOf("Claude Code")).getByText("In sync")).toBeInTheDocument(),
    );
    expect(within(rowOf("Codex CLI")).getByText("Missing")).toBeInTheDocument();
    expect(
      await screen.findByText("1 output missing since the last sync"),
    ).toBeInTheDocument();
    expect(screen.getByText(/is missing for Codex CLI/)).toBeInTheDocument();
  });

  it("lists lint messages, audit findings and what changed since the last sync", async () => {
    bridge.set("agents lint", {
      agentCount: 1,
      discoveryWarnings: [],
      errors: 1,
      infos: 0,
      warnings: 1,
      repo: "r",
      messages: [
        { level: "error", name: "scout", message: "description is missing" },
        { level: "warning", name: "scout", message: "model is not a known alias" },
      ],
    });
    bridge.set(
      "agents audit",
      failure("failed", "high findings", {
        agentCount: 1,
        clean: false,
        discoveryWarnings: [],
        high: 1,
        medium: 0,
        low: 0,
        repo: "r",
        findings: [
          {
            severity: "high",
            agent: "scout",
            message: "pipes a download into a shell",
            line: 12,
          },
        ],
      }),
    );
    bridge.set("agents diff", {
      clean: false,
      discoveryWarnings: [],
      new: ["reviewer"],
      modified: ["scout"],
      removed: ["old"],
      noLockfile: false,
      unchanged: 3,
      repo: "r",
    });
    await open();
    expect(await screen.findByText("1 error, 1 warning, 0 notes")).toBeInTheDocument();
    expect(screen.getByText("description is missing")).toBeInTheDocument();
    expect(
      await screen.findByText(/pipes a download into a shell \(line 12\)/),
    ).toBeInTheDocument();
    expect(screen.getByText("1 high, 0 medium, 0 low")).toBeInTheDocument();
    const diff = screen.getByRole("group", { name: "Changes since last sync" });
    expect(await within(diff).findByText("(modified)")).toBeInTheDocument();
    expect(within(diff).getByText("reviewer")).toBeInTheDocument();
    expect(within(diff).getByText("(removed)")).toBeInTheDocument();
    expect(within(diff).getByText("3 unchanged")).toBeInTheDocument();
  });

  it("shows an error with Retry, then the agents once the read works", async () => {
    const user = userEvent.setup();
    bridge.set("agents ls", failure("failed", "Cannot read the skills repository"));
    render(<AgentsTab />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Cannot read the skills repository");
    bridge.set("agents ls", undefined);
    bridge.set(
      "agents ls",
      (await import("../fixtures/agents")).agentsCtlFixtures.get("agents ls"),
    );
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("sonnet")).toBeInTheDocument();
  });

  it("offers to create the first agent when there are none", async () => {
    bridge.set("agents ls", { agents: [], discoveryWarnings: [], repo: "r" });
    render(<AgentsTab />);
    expect(await screen.findByText("No agents yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Create your first agent/ })).toBeEnabled();
  });

  it("offers Edit body now that the tools exist", async () => {
    await open();
    expect(screen.getByRole("button", { name: "Edit body of scout" })).toBeEnabled();
  });
});

describe("Agents tab: writing", () => {
  const dialog = () => screen.findByRole("dialog");

  it("previews a sync with its warnings, and writes only after Apply", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const box = await dialog();
    expect(within(box).getByText("Write 1 agent to 4 clients")).toBeInTheDocument();
    expect(
      within(box).getByText("/fixture/home/.codex/agents/scout.toml"),
    ).toBeInTheDocument();
    expect(
      within(box).getByText(/scout: cursor: 'tools' field not supported, dropped/),
    ).toBeInTheDocument();
    expect(within(box).getByText("toolportctl agents sync")).toBeInTheDocument();
    expect(bridge.count("agents sync")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText("Wrote 1 agent to 4 clients");
    expect(bridge.count("agents sync")).toBe(1);
    await waitFor(() => expect(bridge.count("agents ls")).toBe(2));
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
  });

  it("syncs one client with --client and previews that first", async () => {
    bridge.set(
      "agents sync --client=codex-cli --dry-run",
      (await import("../fixtures/agents")).agentsSyncData(true),
    );
    bridge.set(
      "agents sync --client=codex-cli",
      (await import("../fixtures/agents")).agentsSyncData(false),
    );
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync scout to Codex CLI" }));
    const box = await dialog();
    expect(bridge.count("agents sync --client=codex-cli --dry-run")).toBe(1);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText(/^Wrote 1 agent/);
    expect(bridge.count("agents sync --client=codex-cli")).toBe(1);
  });

  it("makes you type the phrase before cleaning, and previews the files first", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    const box = await dialog();
    expect(within(box).getByText("Remove 4 synced agent files")).toBeInTheDocument();
    expect(bridge.count("agents clean --dry-run")).toBe(1);
    const confirm = within(box).getByRole("button", { name: "Remove" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "clean agent");
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "s");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    await screen.findByText("Removed 4 synced agent files");
    expect(bridge.count("agents clean")).toBe(1);
  });

  it("uninstalls an agent after typing its name", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Uninstall scout" }));
    const box = await dialog();
    expect(
      within(box).getByText("Remove the agent 'scout' and its 4 outputs"),
    ).toBeInTheDocument();
    const confirm = within(box).getByRole("button", { name: "Uninstall" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "scout");
    await user.click(confirm);
    await waitFor(() => expect(bridge.count("agents uninstall scout")).toBe(1));
  });

  it("creates an agent from the template: name check, preview, confirm", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "New agent…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByRole("textbox"), "Bad Name");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText(/lowercase/)).toBeInTheDocument();
    expect(bridge.count("agents add Bad Name --dry-run")).toBe(0);
    await user.clear(within(form).getByRole("textbox"));
    await user.type(within(form).getByRole("textbox"), "reviewer");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await dialog();
    expect(within(box).getByText(/Create the agent 'reviewer'/)).toBeInTheDocument();
    expect(bridge.count("agents add reviewer")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText(/^Created the agent 'reviewer'/);
    expect(bridge.count("agents add reviewer")).toBe(1);
  });

  it("never applies when the preview failed", async () => {
    bridge.set("agents clean --dry-run", failure("failed", "cannot read the lockfile"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    expect(await screen.findByText("cannot read the lockfile")).toBeInTheDocument();
    expect(bridge.count("agents clean")).toBe(0);
  });

  it("refuses a write the registry does not classify", async () => {
    bridge.set("commands", { commands: [], tools: [] });
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    expect(
      await screen.findByText(/does not know how safe `agents sync` is/),
    ).toBeInTheDocument();
    expect(bridge.count("agents sync")).toBe(0);
  });
});
