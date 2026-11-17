import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import LibraryScreen from "./LibraryScreen";

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

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

async function openCompare() {
  resetPlusFixture({});
  render(<LibraryScreen />);
  await screen.findByText("8 changes can sync; 3 skills are held back");
  const plan = screen.getByRole("region", { name: "Sync plan" });
  await userEvent.click(within(plan).getByRole("button", { name: /changelog/ }));
  await userEvent.click(await screen.findByRole("button", { name: "Compare SKILL.md" }));
  return screen.findByRole("dialog");
}

beforeEach(() => {
  invokeLog.length = 0;
});

describe("Library screen, Compare a held file", () => {
  it("test01_library_compare_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.compare"]).toMatchObject({
      status: "built",
      route: "library",
      test: "src/plus/library/Compare.test.tsx",
    });
  });

  it("test02_a_hand_edited_file_opens_as_a_unified_diff_of_library_against_installed", async () => {
    const dialog = await openCompare();
    expect(await within(dialog).findByLabelText("Differences")).toHaveTextContent(
      "+Mention the ticket id.",
    );
    expect(ranExactly("skills", "diff", "changelog", "--file", "SKILL.md")).toBe(true);
    expect(within(dialog).getByText(/1 line added, 0 lines removed/)).toBeVisible();
  });

  it("test03_keep_mine_records_the_decision_and_the_row_leaves_the_plan", async () => {
    const dialog = await openCompare();
    await within(dialog).findByLabelText("Differences");
    await userEvent.click(within(dialog).getByRole("button", { name: "Keep mine" }));
    await waitFor(() =>
      expect(ranExactly("skills", "keep", "changelog", "SKILL.md")).toBe(true),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(
        within(screen.getByRole("region", { name: "Sync plan" })).queryByText(
          /changelog/,
        ),
      ).not.toBeInTheDocument(),
    );
  });

  it("test04_use_library_copy_replaces_only_that_file_and_syncs_nothing_else", async () => {
    const dialog = await openCompare();
    await within(dialog).findByLabelText("Differences");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Use library copy" }),
    );
    await waitFor(() =>
      expect(
        ranExactly(
          "skills",
          "sync",
          "--only",
          "changelog",
          "--take",
          "changelog/SKILL.md",
        ),
      ).toBe(true),
    );
  });
});
