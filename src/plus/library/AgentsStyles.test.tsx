import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { LibraryWorldOptions } from "../fixtures/libraryWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import AgentsStyles from "./AgentsStyles";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const COVERS = ["library.agents", "library.styles"] as const;

function setup(library: LibraryWorldOptions = {}) {
  resetPlusFixture({ library });
  return render(<AgentsStyles />);
}

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, Agents & styles tab", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/library/AgentsStyles.test.tsx",
      });
    }
  });

  it("test01_an_agent_shows_its_model_tools_and_how_claude_code_gets_it", async () => {
    setup();
    const agents = await screen.findByRole("region", { name: "Agents" });
    expect(within(agents).getByText("code-scout")).toBeVisible();
    expect(within(agents).getByText("sonnet")).toBeVisible();
    expect(within(agents).getByText("Read")).toBeVisible();
    expect(agents).toHaveTextContent("~/.claude/agents/code-scout.md");
    expect(ranExactly("agents", "ls")).toBe(true);
    expect(ranExactly("agents", "status")).toBe(true);
  });

  it("test02_the_agent_checks_are_counted_and_a_check_that_cannot_run_is_not_a_green_zero", async () => {
    setup();
    const checks = await screen.findByRole("region", { name: "Agent checks" });
    await waitFor(() => expect(within(checks).getAllByText("None")).toHaveLength(3));
    expect(ranExactly("agents", "lint")).toBe(true);
    expect(ranExactly("agents", "audit")).toBe(true);
  });

  it("test03_sync_agents_previews_the_files_then_writes", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: /Sync agents/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(".claude/agents/code-scout.md")).toBeVisible();
    expect(ranExactly("agents", "sync", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Sync agents" }));
    await waitFor(() => expect(ranExactly("agents", "sync")).toBe(true));
    expect(await screen.findByText("Agents are in sync")).toBeVisible();
  });

  it("test04_uninstall_an_agent_names_the_copy_it_removes", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Uninstall…" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/agents\/code-scout\.md/)).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Uninstall" }));
    await waitFor(() =>
      expect(ranExactly("agents", "uninstall", "code-scout")).toBe(true),
    );
    expect(await screen.findByText("No agents in your library")).toBeVisible();
  });

  it("test05_no_styles_is_an_empty_state_that_explains_itself", async () => {
    setup();
    const styles = await screen.findByRole("region", { name: "Output styles" });
    expect(await within(styles).findByText("No output styles")).toBeVisible();
    expect(styles).toHaveTextContent("You have none");
    expect(ranExactly("styles", "ls")).toBe(true);
    expect(ranExactly("styles", "lint")).toBe(true);
  });

  it("test06_styles_sync_and_remove_preview_first", async () => {
    setup({ styles: true });
    const styles = await screen.findByRole("region", { name: "Output styles" });
    expect(await within(styles).findByText("plain")).toBeVisible();
    expect(
      await within(styles).findByText("Styles were never synced to Claude Code."),
    ).toBeVisible();
    await userEvent.click(within(styles).getByRole("button", { name: /Sync styles/ }));
    let dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/output-styles\/plain\.md/)).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Sync styles" }));
    await waitFor(() => expect(ranExactly("styles", "sync")).toBe(true));

    await userEvent.click(
      await within(styles).findByRole("button", { name: /Remove applied style/ }),
    );
    dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Remove applied style" }),
    );
    await waitFor(() => expect(ranExactly("styles", "remove")).toBe(true));
    expect(ranExactly("styles", "status")).toBe(true);
  });

  it("test20_an_agent_that_does_not_parse_is_an_error_row_with_the_limit_in_its_reason", async () => {
    setup({ longAgent: true });
    const checks = await screen.findByRole("region", { name: "Agent checks" });
    const findings = await within(checks).findByRole("list", { name: "Lint findings" });
    expect(within(findings).getByText("Error")).toBeVisible();
    expect(findings).toHaveTextContent("long-agent (agent)");
    expect(findings).toHaveTextContent("description must be 1-1536 characters");
    expect(within(checks).getByText("1")).toBeVisible();
  });
});
