import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { FIXTURE_FOLDER } from "../fixtures/contextWorld";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import MemoryCard from "./MemoryCard";

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

const COVERS = ["context.memory", "context.memory-clear", "context.sessions"] as const;

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

function pressUndo(call: unknown[]) {
  (call[1] as unknown as { action: { onClick: () => void } }).action.onClick();
}

async function open(tab?: string) {
  render(<MemoryCard folder={FIXTURE_FOLDER} />);
  await screen.findByRole("tablist", { name: "Memory and history" });
  if (tab) await userEvent.click(screen.getByRole("tab", { name: tab }));
}

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
  resetPlusFixture();
});

describe("Memory and history card", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/memory/MemoryCard.test.tsx",
      });
    }
  });

  it("test01_context_memory_shows_the_switch_with_who_set_it_and_the_files", async () => {
    await open();
    expect(await screen.findByText(/Auto memory is on for web-app/)).toBeInTheDocument();
    expect(screen.getByText(/Set by Claude Code's default/)).toBeInTheDocument();
    expect(screen.getByText("feedback_testing.md")).toBeInTheDocument();
    expect(
      within(screen.getByRole("group", { name: "Auto memory switch" })).getByRole(
        "button",
        { name: "Default" },
      ),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("test02_context_memory_off_writes_the_repository_switch_and_offers_undo", async () => {
    await open();
    const group = await screen.findByRole("group", { name: "Auto memory switch" });
    await userEvent.click(within(group).getByRole("button", { name: "Off" }));
    await waitFor(() => expect(ran("memory", "set", "off", "--cwd")).toBe(true));
    expect(await screen.findByText(/Auto memory is off for web-app/)).toBeInTheDocument();
    const call = vi.mocked(toast.success).mock.calls[0];
    expect(call[0]).toBe("Auto memory is off for web-app");
    pressUndo(call);
    await waitFor(() => expect(ran("memory", "set", "default")).toBe(true));
  });

  it("test03_context_memory_sets_the_global_default_with_its_own_switch", async () => {
    await open();
    const group = await screen.findByRole("group", { name: "Global default switch" });
    await userEvent.click(within(group).getByRole("button", { name: "Off" }));
    await waitFor(() => expect(ran("memory", "set", "off", "--global")).toBe(true));
  });

  it("test04_context_memory_clear_previews_the_files_and_moves_them_to_the_trash", async () => {
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Clear memory…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Clear the auto memory of web-app/),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("list", { name: "Files to clear" }),
    ).toHaveTextContent("user_role.md");
    expect(ran("memory", "clear", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Move to trash" }));
    await waitFor(() =>
      expect(ranArgv().some((a) => a.includes("clear") && !a.includes("--dry-run"))).toBe(
        true,
      ),
    );
    expect(
      await screen.findByText("No memory is kept for this repository."),
    ).toBeVisible();
    const call = vi.mocked(toast.success).mock.calls[0];
    pressUndo(call);
    await waitFor(() => expect(ran("memory", "undo")).toBe(true));
  });

  it("test05_context_memory_clear_for_good_needs_the_repository_name_typed", async () => {
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Clear memory…" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      within(dialog).getByLabelText("Delete for good instead of the trash"),
    );
    const button = await within(dialog).findByRole("button", { name: "Delete for good" });
    expect(button).toBeDisabled();
    await userEvent.type(
      within(dialog).getByLabelText("Type web-app to confirm"),
      "web-app",
    );
    expect(button).toBeEnabled();
    await userEvent.click(button);
    await waitFor(() => expect(ran("--forever", "--confirm", "web-app")).toBe(true));
  });

  it("test06_context_memory_clear_can_also_turn_memory_off", async () => {
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Clear memory…" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(within(dialog).getByLabelText("Also turn auto memory off"));
    await waitFor(() => expect(ran("clear", "--also-off", "--dry-run")).toBe(true));
  });

  it("test07_context_memory_instruction_files_only_switch_where_a_setting_exists", async () => {
    await open("Instruction files");
    expect(
      await screen.findByLabelText("CLAUDE.md (project) loads here"),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("CLAUDE.md (managed) loads here")).toBeNull();
    expect(screen.getByText(/has no setting to leave it out/)).toBeVisible();
    await userEvent.click(screen.getByLabelText("CLAUDE.md (project) loads here"));
    await waitFor(() => expect(ran("leave-out", "add")).toBe(true));
  });

  it("test08_context_sessions_shows_counts_and_the_global_retention_without_a_repository_setting", async () => {
    await open("Sessions");
    expect(await screen.findByText(/3 sessions/)).toBeVisible();
    expect(
      screen.getByText(/after 30 days; one value for all repositories/),
    ).toBeVisible();
    expect(screen.getByText(/no setting per repository/)).toBeVisible();
    expect(screen.getByText(/expires in 3 days/)).toBeVisible();
  });

  it("test09_context_sessions_clear_to_the_archive_then_restore_it", async () => {
    await open("Sessions");
    await userEvent.click(await screen.findByRole("button", { name: "Clear sessions…" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Archive" }));
    await waitFor(() =>
      expect(ran("sessions", "clear", "--to", "archive", "--dry-run")).toBe(true),
    );
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Move to archive" }),
    );
    const restore = await screen.findByRole("button", { name: /Restore archive/ });
    await userEvent.click(restore);
    await waitFor(() => expect(ran("sessions", "restore")).toBe(true));
    expect(await screen.findByText(/3 sessions/)).toBeVisible();
  });

  it("test10_context_sessions_clear_to_the_trash_offers_undo", async () => {
    await open("Sessions");
    await userEvent.click(await screen.findByRole("button", { name: "Clear sessions…" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Move to trash" }),
    );
    expect(
      await screen.findByRole("button", { name: "Put sessions back" }),
    ).toBeVisible();
    const call = vi.mocked(toast.success).mock.calls[0];
    pressUndo(call);
    await waitFor(() => expect(ran("sessions", "undo")).toBe(true));
  });
});
