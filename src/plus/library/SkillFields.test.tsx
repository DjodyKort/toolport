import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { plusFixtureIpc, resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
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

/** What each `skills_edit_frontmatter` call received on stdin. */
function calls(): Array<Record<string, unknown>> {
  return invokeLog
    .filter(
      (entry) =>
        entry.command === "plus_ctl" &&
        (entry.args as { argv: string[] }).argv[2] === "skills_edit_frontmatter",
    )
    .map((entry) => JSON.parse((entry.args as { stdinSecret: string }).stdinSecret));
}
const writes = () => calls().filter((call) => call.confirm === true);
const previews = () => calls().filter((call) => call.dry_run === true);

async function openFields(skill = "task-runner") {
  resetPlusFixture({});
  render(<LibraryScreen />);
  await screen.findByText("8 changes can sync; 3 skills are held back");
  const list = screen.getByRole("region", { name: "Library items" });
  await userEvent.type(within(list).getByLabelText("Search library"), skill);
  await userEvent.click(
    within(list).getByRole("button", { name: new RegExp(`^${skill}`) }),
  );
  await screen.findByRole("heading", { name: skill });
  await userEvent.click(await screen.findByRole("button", { name: "Edit fields…" }));
  const dialog = await screen.findByRole("dialog");
  await within(dialog).findByTestId("field-description");
  return dialog;
}

const undoAction = () =>
  vi.mocked(toast.message).mock.calls.at(-1)?.[1] as unknown as {
    action: { onClick: () => void };
    duration: number;
  };

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, edit the fields of a skill", () => {
  it("test01_library_skill_fields_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.skill-fields"]).toMatchObject({
      status: "built",
      route: "library",
      test: "src/plus/library/SkillFields.test.tsx",
    });
    expect(guiParity.tools.skills_edit_frontmatter).toMatchObject({
      route: "library",
      action: "library.skill-fields",
    });
    expect(guiParity.tools.skills_delete).toMatchObject({ route: "library" });
  });

  it("test02_each_field_is_read_from_the_file_and_the_ones_the_app_cannot_change_say_so", async () => {
    const dialog = await openFields();
    expect(within(dialog).getByTestId("field-name")).toHaveTextContent(
      "The name follows the folder",
    );
    expect(within(dialog).getByLabelText("description")).toHaveValue(
      "Run a named task and report each step",
    );
    expect(within(dialog).getByLabelText("allowed-tools")).toHaveValue("Read, Grep");
    expect(within(dialog).getByLabelText("disable-model-invocation")).toHaveValue(
      "false",
    );
    expect(within(dialog).getByTestId("field-hooks")).toHaveTextContent(
      "Edit it in your editor.",
    );
    expect(within(dialog).getByRole("button", { name: "Review changes" })).toBeDisabled();
    expect(calls()).toEqual([{ name: "task-runner", patch: {}, dry_run: true }]);
  });

  it("test03_a_change_is_reviewed_as_the_changed_lines_before_anything_is_written", async () => {
    const dialog = await openFields();
    const box = within(dialog).getByLabelText("description");
    await userEvent.clear(box);
    await userEvent.type(box, "Run a task");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    const diff = await within(dialog).findByLabelText("Changes");
    expect(diff).toHaveTextContent("Run a named task and report each step");
    expect(diff).toHaveTextContent("description: Run a task");
    expect(previews().at(-1)).toEqual({
      name: "task-runner",
      patch: { description: "Run a task" },
      dry_run: true,
    });
    expect(writes()).toEqual([]);
    expect(within(dialog).getByLabelText("Lint of the result")).toHaveTextContent(
      "Lint finds nothing.",
    );
  });

  it("test04_saving_sends_only_the_changed_fields_and_puts_the_skill_in_the_plan", async () => {
    const dialog = await openFields();
    await userEvent.selectOptions(
      within(dialog).getByLabelText("disable-model-invocation"),
      "true",
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save fields" }),
    );
    await waitFor(() =>
      expect(writes()).toEqual([
        {
          name: "task-runner",
          patch: { "disable-model-invocation": true },
          confirm: true,
        },
      ]),
    );
    expect(undoAction().duration).toBe(30000);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("test05_a_field_is_removed_with_null_and_added_with_the_type_chosen", async () => {
    const dialog = await openFields();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Remove allowed-tools" }),
    );
    expect(within(dialog).getByTestId("field-allowed-tools")).toHaveTextContent(
      "Removed when you save",
    );
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "when_to_use");
    await userEvent.type(within(add).getByLabelText("Value"), "When a task has steps");
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await within(dialog).findByLabelText("Changes");
    expect(previews().at(-1)?.patch).toEqual({
      "allowed-tools": null,
      when_to_use: "When a task has steps",
    });
  });

  it("test06_a_field_name_the_skill_already_has_is_refused_even_when_spelled_with_an_underscore", async () => {
    const dialog = await openFields();
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "allowed_tools");
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    expect(await within(add).findByRole("alert")).toHaveTextContent(
      "already has a field called allowed_tools",
    );
  });

  it("test07_the_listing_length_is_counted_as_you_type_and_marked_when_over", async () => {
    const dialog = await openFields();
    expect(dialog).toHaveTextContent("Claude Code reads 37 characters of 1,536");
    fireEvent.change(within(dialog).getByLabelText("description"), {
      target: { value: "d".repeat(1600) },
    });
    expect(dialog).toHaveTextContent("1,600 characters of 1,536");
    expect(dialog).toHaveTextContent("too long for the skill to be read");
  });

  it("test08_a_change_the_skill_check_refuses_is_shown_inline_and_nothing_is_written", async () => {
    const dialog = await openFields();
    fireEvent.change(within(dialog).getByLabelText("description"), {
      target: { value: "d".repeat(1600) },
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "description must be 1-1536 characters",
    );
    expect(within(dialog).queryByRole("button", { name: "Save fields" })).toBeNull();
    expect(writes()).toEqual([]);
  });

  it("test09_when_to_use_counts_with_the_description_in_the_refusal", async () => {
    const dialog = await openFields();
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "when_to_use");
    fireEvent.change(within(add).getByLabelText("Value"), {
      target: { value: "u".repeat(1530) },
    });
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "description (37) and when_to_use (1530) together must be at most 1536",
    );
  });

  it("test10_lint_of_the_result_shows_a_warning_and_an_error_blocks_the_save", async () => {
    const dialog = await openFields();
    fireEvent.change(within(dialog).getByLabelText("description"), {
      target: { value: "d".repeat(1100) },
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    const lint = await within(dialog).findByLabelText("Lint of the result");
    expect(within(lint).getByText("Warning")).toBeVisible();
    expect(lint).toHaveTextContent("Agent Skills spec allows 1024");
    expect(within(dialog).getByRole("button", { name: "Save fields" })).toBeEnabled();

    await userEvent.click(
      within(dialog).getByRole("button", { name: "Back to editing" }),
    );
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "priority");
    await userEvent.type(within(add).getByLabelText("Value"), "high");
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await within(dialog).findByText("Error");
    expect(within(dialog).getByRole("button", { name: "Save fields" })).toBeDisabled();
    expect(dialog).toHaveTextContent("Lint would hold this skill back");
  });

  it("test11_a_number_field_that_is_not_a_number_stops_the_review", async () => {
    const dialog = await openFields();
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "priority");
    await userEvent.selectOptions(within(add).getByLabelText("Type"), "number");
    await userEvent.type(within(add).getByLabelText("Value"), "x");
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "priority must be a number",
    );
    expect(within(dialog).getByRole("button", { name: "Review changes" })).toBeDisabled();
  });

  it("test12_undo_puts_the_old_values_back_and_removes_a_field_the_edit_added", async () => {
    const dialog = await openFields();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Remove allowed-tools" }),
    );
    const add = within(dialog).getByRole("group", { name: "Add a field" });
    await userEvent.type(within(add).getByLabelText("Field name"), "license");
    await userEvent.type(within(add).getByLabelText("Value"), "MIT");
    await userEvent.click(within(add).getByRole("button", { name: "Add" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save fields" }),
    );
    await waitFor(() => expect(writes()).toHaveLength(1));
    undoAction().action.onClick();
    await waitFor(() => expect(writes()).toHaveLength(2));
    expect(writes()[1]).toEqual({
      name: "task-runner",
      patch: { "allowed-tools": "Read, Grep", license: null },
      confirm: true,
    });
    await waitFor(() =>
      expect(toast.message).toHaveBeenCalledWith(
        "The previous fields of task-runner are back",
      ),
    );
  });

  it("test13_undo_leaves_a_field_that_changed_again_alone", async () => {
    const dialog = await openFields();
    const box = within(dialog).getByLabelText("description");
    await userEvent.clear(box);
    await userEvent.type(box, "First");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save fields" }),
    );
    await waitFor(() => expect(writes()).toHaveLength(1));
    const job = plusFixtureIpc("plus_ctl", {
      argv: ["mcp", "call", "skills_edit_frontmatter", "--args-stdin"],
      stdinSecret: JSON.stringify({
        name: "task-runner",
        patch: { description: "Edited elsewhere" },
        confirm: true,
      }),
    });
    plusFixtureIpc("plus_ctl_result", { job });
    undoAction().action.onClick();
    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalled());
    expect(vi.mocked(toast.error).mock.calls[0][0]).toBe(
      "task-runner was not changed back",
    );
    expect(writes()).toHaveLength(1);
  });

  it("test14_at_390_px_the_dialog_scrolls_inside_the_screen_and_its_fields_wrap", async () => {
    const dialog = await openFields();
    expect(dialog).toHaveClass("max-h-[90vh]", "overflow-y-auto");
    expect(
      within(dialog).getByRole("group", { name: "Add a field" }).firstElementChild
        ?.nextElementSibling,
    ).toHaveClass("flex-wrap");
  });
});
