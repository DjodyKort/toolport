import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import type { TaskWorldOptions } from "../fixtures/taskWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import TasksPage from "./TasksPage";

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
const autostart = vi.fn();
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  isAutostartEnabled: () => autostart(),
}));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const COVERS = [
  "tasks.list",
  "tasks.detail",
  "tasks.run",
  "tasks.continue",
  "tasks.cancel",
  "tasks.history",
  "tasks.manage",
  "tasks.ledger",
  "tasks.switch",
] as const;

const openSettings = vi.fn();

function open(tasks: TaskWorldOptions = {}) {
  resetPlusFixture({ tasks: { stepMs: 600_000, ...tasks } });
  invokeLog.length = 0;
  return render(<TasksPage onOpenSettings={openSettings} />);
}

const ran = (...argv: string[]) =>
  ranArgv().some((a) => argv.every((word, i) => a[i] === word));
const stdinOf = (verb: string) =>
  JSON.parse(
    invokeLog
      .filter((e) => e.command === "plus_ctl")
      .map((e) => e.args as { argv: string[]; stdinSecret: string | null })
      .filter((a) => a.argv[1] === verb && a.stdinSecret)
      .at(-1)!.stdinSecret!,
  );

beforeEach(() => {
  vi.clearAllMocks();
  autostart.mockResolvedValue(true);
});

describe("Tasks screen: list", () => {
  it("test01_the_list_is_sorted_by_what_needs_you_and_a_waiting_run_has_a_banner", async () => {
    open();
    await screen.findByText("Export notes is waiting for you");
    const titles = [
      "Export notes",
      "Sync the notes mirror",
      "Weekly digest",
      "Renew the beta-mail token",
      "Tidy old exports",
    ];
    const rows = screen
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? "")
      .filter((label) => titles.some((t) => label === `Open ${t}`))
      .map((label) => label.replace("Open ", ""));
    expect(rows).toEqual(titles);
    expect(screen.getByText("Needs you")).toBeTruthy();
    expect(screen.getByText("Last run failed")).toBeTruthy();
    expect(screen.getByText(/Scheduler last checked/)).toBeTruthy();
    expect(ran("task", "ls", "--all")).toBe(true);
    expect(COVERS).toContain("tasks.list");
  });

  it("test02_launch_at_login_off_is_a_line_with_a_link_to_settings_that_writes_nothing", async () => {
    autostart.mockResolvedValue(false);
    open();
    await screen.findByText("Launch at login is off");
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(openSettings).toHaveBeenCalledOnce();
    expect(ranArgv().flat().join(" ")).not.toMatch(/autostart/);
  });

  it("test03_launch_at_login_on_shows_no_line", async () => {
    open();
    await screen.findByText("Export notes is waiting for you");
    expect(screen.queryByText("Launch at login is off")).toBeNull();
  });

  it("test04_the_unreadable_state_of_launch_at_login_offers_retry", async () => {
    autostart.mockRejectedValue(new Error("no plugin"));
    open();
    await screen.findByText("Launch at login is unknown");
    autostart.mockResolvedValue(true);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(screen.queryByText("Launch at login is unknown")).toBeNull(),
    );
  });

  it("test05_the_switch_is_one_click_with_an_undo_that_turns_it_back", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("switch", { name: "Tidy old exports is off" }),
    );
    await waitFor(() => expect(ran("task", "enable", "tidy-exports")).toBe(true));
    const call = vi.mocked(toast.message).mock.calls.at(-1)!;
    expect(call[0]).toBe("Tidy old exports is on");
    const action = (call[1] as { action: { label: string; onClick: () => void } }).action;
    expect(action.label).toBe("Undo");
    action.onClick();
    await waitFor(() => expect(ran("task", "disable", "tidy-exports")).toBe(true));
    expect(COVERS).toContain("tasks.switch");
  });

  it("test06_an_empty_install_says_what_a_task_is", async () => {
    open({ scenario: "empty" });
    await screen.findByText("No tasks yet");
    expect(
      screen.getAllByRole("button", { name: /New task|New from a command/ }).length,
    ).toBeGreaterThan(1);
  });

  it("test07_a_failed_list_says_which_command_failed_and_retries", async () => {
    open({ listError: "the data folder is not readable" });
    await screen.findByText("Could not read the tasks");
    expect(
      screen.getByText(/task ls --all failed: the data folder is not readable/),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  it("test08_unreadable_files_are_named_above_the_tasks_and_a_routine_task_cannot_run", async () => {
    open({ scenario: "edge" });
    await screen.findByText("1 task file could not be read");
    expect(screen.getByText("tidy-old.json")).toBeTruthy();
    const row = screen.getByRole("button", { name: "Open Refresh the report cache" });
    expect(within(row.parentElement!).getByText("Cannot run")).toBeTruthy();
    expect(
      within(row.parentElement!).getByText(/names the routine reports-refresh/),
    ).toBeTruthy();
    expect(
      within(row.parentElement!).queryByRole("button", { name: /Run .* now/ }),
    ).toBeNull();
  });
});

describe("Tasks screen: detail, run and stop", () => {
  it("test10_the_detail_shows_how_it_starts_the_steps_and_the_runs_with_their_chips", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await screen.findByText("Last run failed");
    expect(screen.getAllByText(/Every 4 hours at :07/).length).toBeGreaterThan(0);
    expect(screen.getByText("What it does")).toBeTruthy();
    expect(screen.getAllByText("Catch-up").length).toBeGreaterThan(0);
    expect(screen.getByText(/Toolport was not running for 2 slots/)).toBeTruthy();
    expect(ran("task", "show", "nightly-sync")).toBe(true);
    expect(COVERS).toContain("tasks.detail");
  });

  it("test11_a_skipped_slot_is_a_dimmed_row_and_a_too_old_one_offers_run_now", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Weekly digest" }),
    );
    const skipped = await waitFor(() => {
      const row = document.querySelector('[data-skipped="weekly-digest"]');
      expect(row).toBeTruthy();
      return row as HTMLElement;
    });
    expect(within(skipped).getByText("Too old to run")).toBeTruthy();
    expect(within(skipped).queryByRole("button", { name: "Log" })).toBeNull();
    await userEvent.click(within(skipped).getByRole("button", { name: "Run now" }));
    await screen.findByRole("region", { name: "Plan before the run" });
    expect(COVERS).toContain("tasks.ledger");
  });

  it("test12_run_now_shows_the_plan_first_then_runs_and_stop_run_cancels", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: /Run now/ }));
    const plan = await screen.findByRole("region", { name: "Plan before the run" });
    expect(ran("task", "run", "nightly-sync", "--dry-run")).toBe(true);
    expect(ranArgv().some((a) => a[1] === "run" && !a.includes("--dry-run"))).toBe(false);
    expect(within(plan).getByText(/3 steps: 3 run on their own/)).toBeTruthy();
    await userEvent.click(within(plan).getByRole("button", { name: "Run now" }));
    await waitFor(() =>
      expect(ranArgv().some((a) => a[1] === "run" && !a.includes("--dry-run"))).toBe(
        true,
      ),
    );
    await screen.findByRole("button", { name: /Running/ });
    const edit = screen.getByRole("button", { name: "Edit" });
    expect(edit.hasAttribute("disabled")).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Stop run" }));
    await waitFor(() => expect(ranArgv().some((a) => a[1] === "cancel")).toBe(true));
    await screen.findAllByText("Cancelled");
    expect(COVERS).toEqual(expect.arrayContaining(["tasks.run", "tasks.cancel"]));
  });

  it("test13_a_second_start_names_the_live_run_and_the_page_shows_it", async () => {
    open({ refuseStart: true });
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: /Run now/ }));
    const plan = await screen.findByRole("region", { name: "Plan before the run" });
    await userEvent.click(within(plan).getByRole("button", { name: "Run now" }));
    await waitFor(() => expect(toast.message).toHaveBeenCalled());
    expect(vi.mocked(toast.message).mock.calls[0][0]).toBe(
      "Sync the notes mirror is already running",
    );
    expect(JSON.stringify(vi.mocked(toast.message).mock.calls[0][1])).toContain(
      "run-by-schedule",
    );
    await screen.findByText(/by the schedule/);
  });

  it("test14_a_run_that_waits_has_continue_on_the_banner_and_in_the_header", async () => {
    open();
    await userEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await waitFor(() => expect(ran("task", "resume", "run-export-waiting")).toBe(true));
    expect(COVERS).toContain("tasks.continue");
  });

  it("test15_a_waiting_run_whose_process_is_gone_says_so_and_offers_stop_run", async () => {
    open({ runnerGone: true });
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Export notes" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await screen.findByText("The run cannot continue");
    expect(screen.getAllByRole("button", { name: "Stop run" }).length).toBeGreaterThan(0);
  });

  it("test16_a_task_that_is_off_cannot_be_run_and_says_why", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Tidy old exports" }),
    );
    const run = await screen.findByRole("button", { name: /Run now/ });
    expect(run.hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/Turn the task on to run it/)).toBeTruthy();
  });

  it("test17_a_log_opens_in_place_with_the_redacted_output", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    const failed = await waitFor(() => {
      const row = document.querySelector('[data-run="run-sync-1"]');
      expect(row).toBeTruthy();
      return row as HTMLElement;
    });
    await userEvent.click(within(failed).getByRole("button", { name: "Log" }));
    await within(failed).findByText(/permission denied/);
    expect(within(failed).getByText(/Secret values are replaced with/)).toBeTruthy();
    expect(ran("task", "history", "--run", "run-sync-1")).toBe(true);
  });

  it("test18_delete_needs_the_id_typed_and_a_live_run_blocks_it", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Tidy old exports" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "More" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete task" }));
    const card = await screen.findByRole("region", { name: "Delete task" });
    expect(ran("task", "rm", "tidy-exports", "--dry-run")).toBe(true);
    const confirm = within(card).getByRole("button", { name: "Delete task" });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    await userEvent.type(within(card).getByRole("textbox"), "tidy-exports");
    await userEvent.click(confirm);
    await waitFor(() =>
      expect(ranArgv().some((a) => a[1] === "rm" && !a.includes("--dry-run"))).toBe(true),
    );
    await screen.findByText("Sorted by what needs you, then by next run");
  });
});

describe("Tasks screen: create and edit", () => {
  it("test20_edit_reviews_with_a_dry_run_then_saves_and_returns_to_the_detail", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    const name = await screen.findByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Sync the mirror");
    await userEvent.click(screen.getByRole("button", { name: "Review changes" }));
    const review = await screen.findByRole("region", { name: "Review changes" });
    expect(ran("task", "edit", "nightly-sync", "--file", "-", "--dry-run")).toBe(true);
    expect(stdinOf("edit").title).toBe("Sync the mirror");
    expect(within(review).getByText(/Backs up the old file first/)).toBeTruthy();
    await userEvent.click(within(review).getByRole("button", { name: "Save task" }));
    await waitFor(() =>
      expect(ranArgv().some((a) => a[1] === "edit" && !a.includes("--dry-run"))).toBe(
        true,
      ),
    );
    await screen.findByText("Sync the mirror");
    expect(COVERS).toContain("tasks.manage");
  });

  it("test21_the_schedule_editor_shows_the_next_runs_from_the_dry_run_in_utc_and_local", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    await screen.findByText("Next runs");
    await waitFor(
      () => expect(screen.getAllByText(/UTC$/).length).toBeGreaterThanOrEqual(5),
      { timeout: 3000 },
    );
    expect(ran("task", "edit", "nightly-sync", "--file", "-", "--dry-run")).toBe(true);
    expect(screen.getByLabelText("Cron, read in UTC")).toBeTruthy();
  });

  it("test22_a_new_task_is_named_given_a_step_reviewed_and_created", async () => {
    open({ scenario: "empty" });
    await userEvent.click(
      (await screen.findAllByRole("button", { name: "New task" }))[0],
    );
    await userEvent.type(await screen.findByLabelText("Name"), "Daily export");
    expect((screen.getByLabelText("ID") as HTMLInputElement).value).toBe("daily-export");
    await userEvent.click(screen.getByRole("button", { name: /Add step/ }));
    await userEvent.click(screen.getByRole("menuitem", { name: /Run a program/ }));
    await userEvent.type(await screen.findByLabelText("Program"), "notes-export");
    await userEvent.click(screen.getByRole("button", { name: "Review changes" }));
    const review = await screen.findByRole("region", { name: "Review changes" });
    expect(ran("task", "add", "daily-export", "--file", "-", "--dry-run")).toBe(true);
    await userEvent.click(within(review).getByRole("button", { name: "Create task" }));
    await waitFor(() =>
      expect(ran("task", "add", "daily-export", "--file", "-")).toBe(true),
    );
    expect(stdinOf("add").steps[0]).toMatchObject({
      type: "exec",
      program: "notes-export",
    });
  });

  it("test23_a_refusal_from_the_command_is_shown_and_nothing_is_saved", async () => {
    open({ scenario: "empty" });
    await userEvent.click(
      (await screen.findAllByRole("button", { name: "New task" }))[0],
    );
    await userEvent.type(await screen.findByLabelText("Name"), "Bad cron");
    await userEvent.click(screen.getByRole("button", { name: /Add step/ }));
    await userEvent.click(screen.getByRole("menuitem", { name: /Run a program/ }));
    await userEvent.click(screen.getByLabelText("On a schedule"));
    await userEvent.click(screen.getByLabelText("Repeat"));
    await userEvent.selectOptions(screen.getByLabelText("Repeat"), "custom");
    await userEvent.clear(screen.getByLabelText("Cron, read in UTC"));
    await userEvent.type(screen.getByLabelText("Cron, read in UTC"), "every day");
    await userEvent.click(screen.getByRole("button", { name: "Review changes" }));
    const review = await screen.findByRole("region", { name: "Review changes" });
    await within(review).findByRole("alert");
    expect(within(review).queryByRole("button", { name: "Create task" })).toBeNull();
    expect(ranArgv().some((a) => a[1] === "add" && !a.includes("--dry-run"))).toBe(false);
  });

  it("test24_a_routine_step_is_replaced_with_a_script_before_it_can_be_saved", async () => {
    open({ scenario: "edge" });
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Refresh the report cache" }),
    );
    await screen.findAllByText("Cannot run");
    await userEvent.click(screen.getAllByRole("button", { name: "Fix" })[0]);
    const review = await screen.findByRole("button", { name: "Review changes" });
    expect(review.hasAttribute("disabled")).toBe(false);
    await userEvent.click(review);
    await screen.findByRole("region", { name: "Review changes" });
    expect(stdinOf("edit").steps[1]).toMatchObject({ type: "script", script: "" });
    expect(JSON.stringify(stdinOf("edit"))).not.toContain("routineId");
  });

  it("test25_duplicate_opens_a_copy_that_starts_off", async () => {
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open Sync the notes mirror" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "More" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(((await screen.findByLabelText("Name")) as HTMLInputElement).value).toBe(
      "Sync the notes mirror (copy)",
    );
    expect((screen.getByLabelText("ID") as HTMLInputElement).value).toBe(
      "nightly-sync-copy",
    );
    expect(screen.getByRole("switch", { name: "On" }).getAttribute("aria-checked")).toBe(
      "false",
    );
  });

  it("test26_new_from_a_command_reviews_a_draft_that_starts_off", async () => {
    open();
    await userEvent.click(
      (await screen.findAllByRole("button", { name: /New from a command/ }))[0],
    );
    await userEvent.type(
      await screen.findByLabelText("Command file"),
      "/home/u/.claude/commands/sync-notes.md",
    );
    expect((screen.getByLabelText("ID") as HTMLInputElement).value).toBe("sync-notes");
    await userEvent.click(screen.getByRole("button", { name: "Review draft" }));
    await screen.findByRole("region", { name: "Review draft" });
    expect(
      ran(
        "task",
        "add",
        "sync-notes",
        "--from-command",
        "/home/u/.claude/commands/sync-notes.md",
        "--dry-run",
      ),
    ).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Create task" }));
    await waitFor(() =>
      expect(
        ranArgv().some((a) => a[3] === "--from-command" && !a.includes("--dry-run")),
      ).toBe(true),
    );
  });
});

describe("Tasks screen: history", () => {
  it("test30_history_mixes_tasks_with_chips_and_skipped_slots_behind_a_switch", async () => {
    open();
    await userEvent.click(await screen.findByRole("tab", { name: /History/ }));
    await screen.findByText("Today", {}, { timeout: 5000 });
    expect(screen.getAllByText("Catch-up").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Busy: slot not run").length).toBe(1);
    await userEvent.click(screen.getByRole("switch", { name: "Skipped slots" }));
    expect(screen.queryByText("Busy: slot not run")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /^Failed/ }));
    expect(screen.queryAllByText("ok").length).toBe(0);
    expect(ran("task", "history", "--limit", "200")).toBe(true);
    expect(ran("task", "missed")).toBe(true);
    expect(COVERS).toContain("tasks.history");
  });
});

describe("Tasks screen: parity", () => {
  it("test40_every_action_of_the_tasks_route_is_covered_here", () => {
    const ids = Object.entries(guiParity.actions)
      .filter(([, a]) => a.route === "tasks")
      .map(([id]) => id)
      .sort();
    expect(ids).toEqual([...COVERS].sort());
  });
});
