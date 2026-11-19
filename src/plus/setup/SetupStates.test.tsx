import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { invokeLog, ranArgv } from "../servers/testkit";
import { SetupPage } from "./SetupPage";

/** Holds the answer of one setup command until a test lets it through. */
const hold: { verb: string | null; release: () => void; gate: Promise<void> } = {
  verb: null,
  release: () => {},
  gate: Promise.resolve(),
};

function holdBack(verb: string) {
  hold.verb = verb;
  hold.gate = new Promise<void>((resolve) => {
    hold.release = () => {
      hold.verb = null;
      resolve();
    };
  });
}

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      const argv = (args as { argv?: string[] }).argv;
      if (hold.verb && argv?.[1] === hold.verb) await hold.gate;
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

const FILE = "/tmp/setup.zip";

type User = ReturnType<typeof userEvent.setup>;

function open(tab: string) {
  const user = userEvent.setup();
  render(<SetupPage onBack={() => {}} />);
  return { user, go: () => user.click(screen.getByRole("tab", { name: tab })) };
}

const PLAIN = { plain: true } as const;

async function planned(user: User) {
  await user.click(screen.getByRole("tab", { name: "Restore" }));
  await user.type(await screen.findByLabelText("Setup file"), FILE);
  await user.click(screen.getByRole("button", { name: "Check the file" }));
  await screen.findByRole("region", { name: "Plan" });
}

const count = (verb: string) => ranArgv().filter((a) => a[1] === verb).length;

beforeEach(() => {
  hold.release();
  resetPlusFixture();
  invokeLog.length = 0;
});

describe("the Export view states", () => {
  it("test01_the_parts_show_a_loading_line_until_they_are_read", async () => {
    holdBack("parts");
    render(<SetupPage onBack={() => {}} />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
    expect(await screen.findByText("Loading the parts…")).toBeInTheDocument();
    expect(screen.queryByLabelText("Repositories")).not.toBeInTheDocument();
    hold.release();
    expect(await screen.findByLabelText("Repositories")).toBeInTheDocument();
    expect(screen.queryByText("Loading the parts…")).not.toBeInTheDocument();
  });

  it("test02_nothing_found_leaves_every_part_tickable_and_names_the_empty_places", async () => {
    resetPlusFixture({ setup: { empty: true } });
    const { user } = open("Export");
    expect(await screen.findByText("No source folder is known yet.")).toBeInTheDocument();
    const repos = screen.getByLabelText("Repositories");
    expect(repos).toBeEnabled();
    expect(repos).toBeChecked();
    await user.click(screen.getByLabelText(/^Sessions/));
    expect(
      screen.getByText("No past conversations were found for these repositories."),
    ).toBeInTheDocument();
  });

  it("test03_a_failed_read_says_why_and_the_button_reads_the_parts_again", async () => {
    resetPlusFixture({ setup: { broken: ["parts"] } });
    const { user } = open("Export");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read the setup parts: parts failed: the disk is read-only",
    );
    expect(screen.queryByLabelText("Repositories")).not.toBeInTheDocument();
    const before = count("parts");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(count("parts")).toBe(before + 1));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("test04_a_failed_write_says_nothing_was_written_and_shows_no_saved_card", async () => {
    resetPlusFixture({ setup: { broken: ["export"] } });
    const { user } = open("Export");
    await screen.findByLabelText("Repositories");
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.click(screen.getByRole("button", { name: "Write the file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not export the setup: export failed: the disk is read-only. Nothing was written.",
    );
    expect(screen.queryByText("Export saved")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Write the file" })).toBeEnabled();
  });

  it("test05_the_buttons_wait_while_a_write_runs", async () => {
    open("Export");
    await screen.findByLabelText("Repositories");
    fireEvent.change(screen.getByLabelText("Write to"), {
      target: { value: "/tmp/out" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Write the file" }));
    expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    await screen.findByText("Export saved");
    expect(screen.getByRole("button", { name: "Write the file" })).toBeEnabled();
  });
});

describe("the Restore view states", () => {
  it("test06_before_a_file_is_chosen_the_view_says_it_starts_as_a_dry_run", async () => {
    const { go } = open("Restore");
    await go();
    expect(await screen.findByText(/Restore always starts as a dry run/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Check the file" })).toBeDisabled();
    expect(ranArgv().some((a) => a[1] === "restore")).toBe(false);
  });

  it("test07_the_check_button_waits_while_the_file_is_read", async () => {
    resetPlusFixture({ setup: PLAIN });
    const { user, go } = open("Restore");
    await go();
    await user.type(await screen.findByLabelText("Setup file"), FILE);
    fireEvent.click(screen.getByRole("button", { name: "Check the file" }));
    expect(screen.getByRole("button", { name: "Check the file" })).toBeDisabled();
    await screen.findByRole("region", { name: "Plan" });
    expect(screen.getByRole("button", { name: "Check the file" })).toBeEnabled();
  });

  it("test08_a_file_that_cannot_be_read_gives_no_plan_and_says_why", async () => {
    resetPlusFixture({ setup: { broken: ["inspect"] } });
    const { user, go } = open("Restore");
    await go();
    await user.type(await screen.findByLabelText("Setup file"), FILE);
    await user.click(screen.getByRole("button", { name: "Check the file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read the file: inspect failed",
    );
    expect(screen.queryByRole("region", { name: "Plan" })).not.toBeInTheDocument();
    expect(screen.queryByText(/^Made on/)).not.toBeInTheDocument();
  });

  it("test09_a_plan_that_cannot_be_made_keeps_the_file_choice_and_says_why", async () => {
    resetPlusFixture({ setup: { ...PLAIN, broken: ["restore"] } });
    const { user, go } = open("Restore");
    await go();
    await user.type(await screen.findByLabelText("Setup file"), FILE);
    await user.click(screen.getByRole("button", { name: "Check the file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not plan the restore: restore failed",
    );
    expect(screen.queryByRole("region", { name: "Plan" })).not.toBeInTheDocument();
    expect(screen.getByText(/^Made on Studio Mac/)).toBeInTheDocument();
  });

  it("test10_a_file_with_nothing_to_restore_plans_nothing_and_offers_no_apply", async () => {
    resetPlusFixture({ setup: { ...PLAIN, empty: true } });
    const { user, go } = open("Restore");
    await go();
    await user.type(await screen.findByLabelText("Setup file"), FILE);
    await user.click(screen.getByRole("button", { name: "Check the file" }));
    expect(
      await screen.findByText("The parts you chose hold nothing to restore."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restore" })).toBeDisabled();
  });

  it("test11_the_plan_names_what_it_adds_and_what_needs_you_before_anything_is_written", async () => {
    resetPlusFixture({ setup: PLAIN });
    const { user } = open("Export");
    await planned(user);
    const plan = screen.getByRole("region", { name: "Plan" });
    expect(within(plan).getByText("web")).toBeInTheDocument();
    expect(within(plan).getByText("needs a secret")).toBeInTheDocument();
    expect(screen.getByText("This is a dry run")).toBeInTheDocument();
    expect(ranArgv().some((a) => a.includes("--apply"))).toBe(false);
  });

  it("test12_a_refused_apply_says_nothing_changed_and_the_plan_stays_to_try_again", async () => {
    resetPlusFixture({ setup: { ...PLAIN, broken: ["apply"] } });
    const { user } = open("Export");
    await planned(user);
    await user.click(screen.getByRole("button", { name: /^Restore \d+ changes?$/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not restore the setup: apply failed: the disk is read-only. Nothing was changed.",
    );
    expect(screen.getByRole("region", { name: "Plan" })).toBeInTheDocument();
    expect(screen.queryByText(/Restore finished/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Restore \d+ changes?$/ })).toBeEnabled();
  });

  it("test13_an_applied_restore_reports_what_is_still_yours", async () => {
    resetPlusFixture({ setup: PLAIN });
    const { user } = open("Export");
    await planned(user);
    await user.click(screen.getByRole("button", { name: /^Restore \d+ changes?$/ }));
    expect(await screen.findByText("Still to do by hand")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Plan" })).not.toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Undo this restore" }),
    ).toBeInTheDocument();
  });
});

describe("the Undo panel states", () => {
  it("test14_with_nothing_restored_undo_says_there_is_nothing_to_undo", async () => {
    const { user, go } = open("Restore");
    await go();
    await user.click(
      await screen.findByRole("button", { name: "Undo the last restore" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not undo: nothing to undo",
    );
    expect(screen.queryByText("What Undo would do")).not.toBeInTheDocument();
  });

  it("test15_a_failed_undo_says_so_and_the_restore_stays_to_undo_again", async () => {
    resetPlusFixture({ setup: { ...PLAIN, broken: ["undo"] } });
    const { user } = open("Export");
    await planned(user);
    await user.click(screen.getByRole("button", { name: /^Restore \d+ changes?$/ }));
    await user.click(await screen.findByRole("button", { name: "Undo this restore" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not undo: undo failed: the disk is read-only",
    );
    expect(screen.getByRole("button", { name: "Undo this restore" })).toBeEnabled();
  });

  it("test16_undo_shows_its_steps_first_and_nothing_is_undone_until_it_is_confirmed", async () => {
    resetPlusFixture({ setup: PLAIN });
    const { user } = open("Export");
    await planned(user);
    await user.click(screen.getByRole("button", { name: /^Restore \d+ changes?$/ }));
    await user.click(await screen.findByRole("button", { name: "Undo this restore" }));
    expect(await screen.findByText("What Undo would do")).toBeInTheDocument();
    const undos = ranArgv().filter((a) => a[1] === "undo");
    expect(undos.every((a) => a.includes("--dry-run"))).toBe(true);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText("What Undo would do")).not.toBeInTheDocument();
  });
});

describe("the reset checklist states", () => {
  it("test17_the_checklist_shows_a_checking_line_until_the_answer_is_in", async () => {
    holdBack("check");
    const { go } = open("Before a reset");
    await go();
    expect(await screen.findByText("Checking…")).toBeInTheDocument();
    expect(screen.queryByText("Not ready for a reset")).not.toBeInTheDocument();
    hold.release();
    expect(await screen.findByText("Not ready for a reset")).toBeInTheDocument();
    expect(screen.queryByText("Checking…")).not.toBeInTheDocument();
  });

  it("test18_a_failed_check_says_why_and_never_reads_as_ready", async () => {
    resetPlusFixture({ setup: { broken: ["check"] } });
    const { user, go } = open("Before a reset");
    await go();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not check the repositories: check failed: the disk is read-only",
    );
    expect(screen.queryByText("Ready for a reset")).not.toBeInTheDocument();
    const before = count("check");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(count("check")).toBe(before + 1));
  });

  it("test19_nothing_found_reads_clean_without_claiming_a_reset_is_safe", async () => {
    resetPlusFixture({ setup: { empty: true } });
    const { go } = open("Before a reset");
    await go();
    expect(
      await screen.findByText("All 0 repositories are clean and pushed."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No past conversations were found for these repositories."),
    ).toBeInTheDocument();
    expect(screen.getByText("No export yet")).toBeInTheDocument();
    expect(screen.getByText("Not ready for a reset")).toBeInTheDocument();
  });

  it("test20_only_amber_rows_can_be_acknowledged", async () => {
    const { go } = open("Before a reset");
    await go();
    await screen.findByText("Not ready for a reset");
    const buttons = screen.getAllByRole("button", { name: "Acknowledge" });
    expect(buttons).toHaveLength(1);
    expect(buttons[0].closest("li")).toHaveTextContent("1 stashes");
  });
});

describe("at 390 px", () => {
  const NARROW = 390;
  const FIXED_WIDTH = /(^|\s)(min-|max-)?w-\[\d+px\]/;

  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: NARROW });
  });

  function noFixedWidths(root: HTMLElement) {
    const offenders = [...root.querySelectorAll("[class]")].filter((el) =>
      FIXED_WIDTH.test(el.getAttribute("class") ?? ""),
    );
    expect(offenders).toEqual([]);
  }

  function rowsWrap(root: HTMLElement) {
    const rows = [...root.querySelectorAll("button")]
      .map((b) => b.parentElement!)
      .filter((p) => p.classList.contains("flex") && !p.getAttribute("role"));
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) expect(row).toHaveClass("flex-wrap");
  }

  it("test21_the_tabs_and_the_export_rows_wrap_and_carry_no_fixed_width", async () => {
    const { container } = render(<SetupPage onBack={() => {}} />);
    await screen.findByLabelText("Repositories");
    expect(screen.getByRole("tablist")).toHaveClass("flex-wrap");
    noFixedWidths(container);
    for (const path of container.querySelectorAll("li > span.min-w-0"))
      expect(path.className).toMatch(/break-/);
    for (const input of screen.getAllByRole("textbox"))
      expect(input.className).not.toMatch(FIXED_WIDTH);
  });

  it("test22_the_restore_plan_has_a_two_column_summary_that_widens_with_its_container", async () => {
    resetPlusFixture({ setup: PLAIN });
    const { user } = open("Export");
    await planned(user);
    const summary = screen.getByLabelText("Plan summary");
    expect(summary).toHaveClass("grid-cols-2", "@sm:grid-cols-3");
    expect(summary.closest(".\\@container")).not.toBeNull();
    const view = screen.getByRole("region", { name: "Plan" }).closest(".\\@container")!;
    noFixedWidths(view as HTMLElement);
    rowsWrap(view as HTMLElement);
  });

  it("test23_the_checklist_keeps_long_paths_and_commands_inside_the_column", async () => {
    const { go } = open("Before a reset");
    await go();
    await screen.findByText("Not ready for a reset");
    const command = screen.getByText("git -C ~/code/web push origin main");
    expect(command).toHaveClass("break-all", "whitespace-pre-wrap");
    expect(command.parentElement).toHaveClass("flex");
    expect(command).toHaveClass("min-w-0");
  });
});
