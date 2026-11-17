import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FIXTURE_FOLDER } from "../fixtures/contextWorld";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import MemoryOverview from "./MemoryOverview";

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

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
  resetPlusFixture();
});

describe("Memory and history overview", () => {
  it("test00_memory_overview_is_built_and_names_this_file", () => {
    expect(guiParity.actions["memory.overview"]).toMatchObject({
      status: "built",
      test: "src/plus/settings/ToolportPlusPage.test.tsx",
    });
    expect(guiParity.routes.memory).toMatchObject({ status: "built" });
  });

  it("test01_memory_overview_lists_one_row_per_repository_and_the_other_folders", async () => {
    render(<MemoryOverview />);
    const row = (await screen.findByRole("row", { name: /web-app/ })) as HTMLElement;
    expect(within(row).getByText(/3 files/)).toBeVisible();
    expect(screen.getByRole("row", { name: /other/ })).toHaveTextContent("Off");
    expect(screen.getByText(/Other folders: 1 folder/)).toBeVisible();
    expect(ran("memory", "overview", "--all")).toBe(true);
  });

  it("test02_memory_overview_filters_ask_the_command", async () => {
    render(<MemoryOverview />);
    await screen.findByRole("row", { name: /web-app/ });
    await userEvent.click(screen.getByRole("button", { name: "Memory off" }));
    await waitFor(() => expect(ran("--filter", "memory-off")).toBe(true));
    await waitFor(() =>
      expect(screen.queryByRole("row", { name: /web-app/ })).toBeNull(),
    );
    expect(screen.getByRole("row", { name: /other/ })).toBeVisible();
  });

  it("test03_memory_overview_turns_a_repository_off_with_undo", async () => {
    render(<MemoryOverview />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Turn off auto memory for web-app" }),
    );
    await waitFor(() =>
      expect(ran("memory", "set", "off", "--cwd", FIXTURE_FOLDER)).toBe(true),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Turn off auto memory for web-app" }),
      ).toBeDisabled(),
    );
  });

  it("test04_memory_overview_clears_old_sessions_after_a_plan_and_a_confirmation", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<MemoryOverview />);
    const days = await screen.findByLabelText("Days for bulk clear");
    await userEvent.clear(days);
    await userEvent.type(days, "7");
    await userEvent.click(
      await screen.findByRole("button", { name: "Clear old sessions of web-app" }),
    );
    await waitFor(() =>
      expect(ranArgv().some((a) => a.includes("clear") && !a.includes("--dry-run"))).toBe(
        true,
      ),
    );
    expect(ran("sessions", "clear", "--dry-run")).toBe(true);
    expect(confirm).toHaveBeenCalled();
  });

  it("test05_memory_overview_opens_a_repository_in_context", async () => {
    const onOpen = vi.fn();
    render(<MemoryOverview onOpen={onOpen} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Open web-app in Context" }),
    );
    expect(onOpen).toHaveBeenCalledWith(FIXTURE_FOLDER);
  });
});
