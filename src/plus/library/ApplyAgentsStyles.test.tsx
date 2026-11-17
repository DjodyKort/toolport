import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
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

describe("Library screen, agents and styles in the plan", () => {
  it("test01_the_plan_has_a_group_for_agents_and_one_for_output_styles", async () => {
    resetPlusFixture({ library: { extras: true } });
    render(<LibraryScreen />);
    const plan = await screen.findByRole("region", { name: "Sync plan" });
    expect(within(plan).getByText("Agents")).toBeVisible();
    expect(within(plan).getByText("Output styles")).toBeVisible();
    expect(within(plan).getByText("code-scout")).toBeVisible();
    expect(within(plan).getByText("plain")).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply 10 changes" })).toBeVisible();
    await userEvent.click(within(plan).getByRole("button", { name: /code-scout/ }));
    const files = await screen.findByRole("list", { name: "Files of code-scout" });
    expect(within(files).getByText(".claude/agents/code-scout.md")).toBeVisible();
    expect(ranArgv().some((argv) => argv.join(" ") === "skills files code-scout")).toBe(
      false,
    );
  });

  it("test02_one_apply_covers_them_and_undo_lists_their_files", async () => {
    resetPlusFixture({ library: { extras: true } });
    render(<LibraryScreen />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Apply 10 changes" }),
    );
    expect(
      await screen.findByText(/Synced 10 changes\. Checked after writing/),
    ).toBeVisible();
    expect(ranExactly("skills", "sync", "--apply-held")).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/\.claude\/agents\/code-scout\.md/),
    ).toBeVisible();
    expect(within(dialog).getByText(/\.claude\/output-styles\/plain\.md/)).toBeVisible();
  });
});
