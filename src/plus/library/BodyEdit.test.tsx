import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { plusFixtureIpc, resetPlusFixture } from "../fixtures/ipc";
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

/** The JSON each `mcp call` of the tool received on stdin. */
function stdinsOf(tool: string): Array<Record<string, unknown>> {
  return invokeLog
    .filter(
      (entry) =>
        entry.command === "plus_ctl" &&
        (entry.args as { argv: string[] }).argv[2] === tool,
    )
    .map((entry) =>
      JSON.parse((entry.args as { stdinSecret: string }).stdinSecret),
    ) as Array<Record<string, unknown>>;
}

async function openSkill(name: string) {
  resetPlusFixture({});
  render(<LibraryScreen />);
  await screen.findByText("8 changes can sync; 3 skills are held back");
  const list = screen.getByRole("region", { name: "Library items" });
  await userEvent.type(within(list).getByLabelText("Search library"), name);
  await userEvent.click(
    within(list).getByRole("button", { name: new RegExp(`^${name}`) }),
  );
  await screen.findByRole("heading", { name });
}

async function editBody(text: string) {
  await userEvent.click(await screen.findByRole("button", { name: "Edit body…" }));
  const dialog = await screen.findByRole("dialog");
  const box = await within(dialog).findByLabelText("Body");
  await userEvent.clear(box);
  await userEvent.type(box, text);
  return { dialog, box };
}

const toastOptions = () =>
  vi.mocked(toast.message).mock.calls.at(-1)?.[1] as unknown as {
    action: { label: string; onClick: () => void };
    duration: number;
  };

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, edit a skill body", () => {
  it("test01_library_edit_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.edit"]).toMatchObject({
      status: "built",
      route: "library",
      test: "src/plus/library/BodyEdit.test.tsx",
    });
    for (const tool of ["skills_get", "skills_edit_body"]) {
      expect(guiParity.tools[tool]).toMatchObject({
        route: "library",
        action: "library.edit",
      });
    }
  });

  it("test02_the_body_of_a_skill_is_read_and_the_save_stays_off_until_it_changes", async () => {
    await openSkill("git-tidy");
    await userEvent.click(await screen.findByRole("button", { name: "Edit body…" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByLabelText("Body")).toHaveValue(
      "# git-tidy\n\nSteps for git-tidy.\n",
    );
    expect(within(dialog).getByRole("button", { name: "Review changes" })).toBeDisabled();
    expect(stdinsOf("skills_get")).toEqual([{ name: "git-tidy" }]);
    expect(stdinsOf("skills_edit_body")).toEqual([]);
  });

  it("test03_the_changes_are_shown_as_a_diff_before_anything_is_written", async () => {
    await openSkill("git-tidy");
    const { dialog } = await editBody("# git-tidy\n\nSteps for git-tidy, newest first.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    const diff = await within(dialog).findByLabelText("Changes");
    expect(diff).toHaveTextContent("Steps for git-tidy.");
    expect(diff).toHaveTextContent("Steps for git-tidy, newest first.");
    expect(within(dialog).getByText(/1 line added, 1 line removed/)).toBeVisible();
    expect(stdinsOf("skills_edit_body")).toEqual([]);

    await userEvent.click(
      within(dialog).getByRole("button", { name: "Back to editing" }),
    );
    expect(await within(dialog).findByLabelText("Body")).toBeVisible();
    expect(stdinsOf("skills_edit_body")).toEqual([]);
  });

  it("test04_saving_writes_the_body_and_puts_the_skill_in_the_plan", async () => {
    await openSkill("git-tidy");
    const { dialog } = await editBody("# git-tidy\n\nSteps for git-tidy, newest first.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save body" }),
    );
    await waitFor(() =>
      expect(stdinsOf("skills_edit_body")).toEqual([
        {
          name: "git-tidy",
          new_body: "# git-tidy\n\nSteps for git-tidy, newest first.",
          confirm: true,
        },
      ]),
    );
    expect(toastOptions().action.label).toBe("Undo");
    expect(toastOptions().duration).toBe(30000);
    await userEvent.click(await screen.findByRole("button", { name: /Library$/ }));
    expect(await screen.findByRole("button", { name: "Apply 9 changes" })).toBeVisible();
  });

  it("test05_undo_on_the_message_puts_the_previous_body_back", async () => {
    await openSkill("git-tidy");
    const { dialog } = await editBody("Something else entirely.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save body" }),
    );
    await waitFor(() => expect(toastOptions()).toBeDefined());
    toastOptions().action.onClick();
    await waitFor(() => expect(stdinsOf("skills_edit_body")).toHaveLength(2));
    expect(stdinsOf("skills_edit_body")[1]).toEqual({
      name: "git-tidy",
      new_body: "# git-tidy\n\nSteps for git-tidy.\n",
      confirm: true,
    });
    await waitFor(() =>
      expect(toast.message).toHaveBeenCalledWith("The previous body of git-tidy is back"),
    );
  });

  it("test06_undo_leaves_a_body_that_changed_again_alone", async () => {
    await openSkill("git-tidy");
    const { dialog } = await editBody("First version.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save body" }),
    );
    await waitFor(() => expect(toastOptions()).toBeDefined());
    const job = plusFixtureIpc("plus_ctl", {
      argv: ["mcp", "call", "skills_edit_body", "--args-stdin"],
      stdinSecret: JSON.stringify({
        name: "git-tidy",
        new_body: "Edited somewhere else.",
        confirm: true,
      }),
    });
    plusFixtureIpc("plus_ctl_result", { job });
    invokeLog.length = 0;
    toastOptions().action.onClick();
    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalled());
    expect(vi.mocked(toast.error).mock.calls[0][0]).toBe("git-tidy was not changed back");
    expect(ranArgv().some((argv) => argv[2] === "skills_get")).toBe(true);
    expect(stdinsOf("skills_edit_body")).toEqual([]);
  });
});
