import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { invokeLog } from "../servers/testkit";
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

beforeEach(() => {
  invokeLog.length = 0;
});

describe("Library screen, the listing length of a skill", () => {
  it("test01_the_checks_row_of_a_skill_over_the_limit_shows_the_combined_count_and_both_parts", async () => {
    resetPlusFixture({});
    render(<LibraryScreen />);
    await screen.findByText("8 changes can sync; 3 skills are held back");
    const row = (await screen.findByRole("img", { name: /1,602 characters/ })).closest(
      "li",
    );
    expect(row).toHaveTextContent(
      "1,602 characters (description 1,200 + when_to_use 402)",
    );
    expect(row).toHaveTextContent("Held back from sync.");
  });

  it("test02_the_skill_page_shows_the_same_combined_count", async () => {
    resetPlusFixture({});
    render(<LibraryScreen />);
    await screen.findByText("8 changes can sync; 3 skills are held back");
    const list = screen.getByRole("region", { name: "Library items" });
    await userEvent.type(within(list).getByLabelText("Search library"), "ticket-intake");
    await userEvent.click(within(list).getByRole("button", { name: /^ticket-intake/ }));
    const summary = await screen.findByRole("region", { name: "Summary" });
    expect(
      await within(summary).findByText(
        "1,602 characters (description 1,200 + when_to_use 402)",
      ),
    ).toBeVisible();
    expect(summary).toHaveTextContent("1,536 Claude Code limit");
  });
});
