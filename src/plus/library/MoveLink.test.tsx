import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
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

beforeEach(() => {
  invokeLog.length = 0;
});

describe("Library screen, move a linked file into the skill", () => {
  it("test01_a_link_that_leaves_the_folder_is_fixed_after_a_preview_of_the_copy_and_the_lines", async () => {
    resetPlusFixture({});
    render(<LibraryScreen />);
    await screen.findByText("8 changes can sync; 3 skills are held back");
    const row = (await screen.findAllByText(/links \.\.\/shared\/glossary\.md/))[0];
    const item = row.closest("li") as HTMLElement;
    await userEvent.click(
      within(item).getByRole("button", { name: "Move into the skill…" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/spec-writer\/references\/glossary\.md/),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("Lines that change")).toHaveTextContent(
      "+ Read the [glossary](references/glossary.md) before you start.",
    );
    expect(
      ranExactly(
        "skills",
        "fix",
        "move-link",
        "spec-writer",
        "../shared/glossary.md",
        "--dry-run",
      ),
    ).toBe(true);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Move into the skill" }),
    );
    await waitFor(() =>
      expect(
        ranExactly("skills", "fix", "move-link", "spec-writer", "../shared/glossary.md"),
      ).toBe(true),
    );
    await waitFor(() =>
      expect(screen.queryByText(/links \.\.\/shared\/glossary\.md/)).toBeNull(),
    );
  });

  it("test02_the_skill_page_offers_the_same_fix_on_the_link", async () => {
    resetPlusFixture({});
    render(<LibraryScreen />);
    await screen.findByText("8 changes can sync; 3 skills are held back");
    await userEvent.type(screen.getByLabelText("Search library"), "spec-writer");
    const list = screen.getByRole("region", { name: "Library items" });
    await userEvent.click(within(list).getByRole("button", { name: /spec-writer/ }));
    const links = await screen.findByRole("region", { name: "Links" });
    await userEvent.click(
      within(links).getByRole("button", { name: "Move into the skill…" }),
    );
    expect(await screen.findByRole("dialog")).toHaveTextContent("references/glossary.md");
  });
});
