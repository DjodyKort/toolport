import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import {
  PROFILE_PERSONAL,
  PROFILE_WORK,
  SPEND_NOW,
  type SpendWorldOptions,
} from "../fixtures/spendWorld";
import { invokeLog, ranArgv } from "../servers/testkit";
import { setClock } from "../tasks/clock";
import { ExportDialog } from "./ExportDialog";
import SpendScreen from "./SpendScreen";

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
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    message: vi.fn(),
  }),
}));

/** The actions of src/plus/gui-parity/ this file proves: the plan with its conflict choice,
 * apply and Undo (spend.setup), Prune now with its preview (spend.prune) and the export dialog
 * (spend.export). */
const COVERS = ["spend.setup", "spend.prune", "spend.export"] as const;

function open(spend: SpendWorldOptions = {}) {
  resetPlusFixture({ spend });
  invokeLog.length = 0;
  return render(<SpendScreen />);
}

const joined = () => ranArgv().map((argv) => argv.join(" "));
const wrote = (word: string) =>
  joined().filter(
    (line) => line.startsWith(`spend ${word}`) && !line.includes("--dry-run"),
  );
const dialog = () => screen.findByRole("dialog");
const page = () => screen.findByRole("radiogroup", { name: "Range" });

beforeEach(() => {
  vi.clearAllMocks();
  setClock(SPEND_NOW);
});

async function openPlan() {
  const buttons = await screen.findAllByRole("button", { name: "Set up collecting…" });
  await userEvent.click(buttons[0]);
  return dialog();
}

describe("the setup plan dialog", () => {
  it("test01_the_plan_opens_on_its_heading_and_enter_applies_nothing", async () => {
    open({ scenario: "never" });
    const plan = await openPlan();
    const heading = within(plan).getByRole("heading", {
      name: "Turn on spend collecting",
    });
    await waitFor(() => expect(heading).toHaveFocus());
    await userEvent.keyboard("{Enter}");
    expect(wrote("setup apply")).toEqual([]);
    expect(
      await within(plan).findAllByText(/\+ "CLAUDE_CODE_ENABLE_TELEMETRY": "1"/),
    ).toHaveLength(2);
    expect(plan).toHaveTextContent("~/.claude/settings.json");
    expect(plan).toHaveTextContent("~/.claude-work/settings.json");
    expect(plan).toHaveTextContent("OTEL_LOG_USER_PROMPTS");
    expect(plan).toHaveTextContent("stay unset");
    expect(within(plan).getByRole("button", { name: "Apply 10 changes" })).toBeEnabled();
  });

  it("test02_apply_writes_the_plan_and_the_page_waits_for_the_first_event", async () => {
    open({ scenario: "never" });
    const plan = await openPlan();
    await userEvent.click(await within(plan).findByRole("button", { name: /^Apply/ }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(wrote("setup apply")).toEqual(["spend setup apply"]);
    expect(toast.success).toHaveBeenCalledWith(
      "Collecting is on",
      expect.objectContaining({
        description: expect.stringContaining("Restart Claude Code sessions"),
        action: expect.objectContaining({ label: "Undo setup…" }),
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("spend-strip")).toHaveTextContent(
        "Waiting for the first event",
      ),
    );
  });

  it("test03_a_profile_can_be_left_out_and_only_the_others_are_written", async () => {
    open({ scenario: "never" });
    const plan = await openPlan();
    await userEvent.click(
      await within(plan).findByRole("checkbox", { name: "~/.claude/settings.json" }),
    );
    await userEvent.click(within(plan).getByRole("button", { name: "Apply 5 changes" }));
    await waitFor(() => expect(wrote("setup apply")).toHaveLength(1));
    expect(wrote("setup apply")[0]).toBe(`spend setup apply --profile ${PROFILE_WORK}`);
  });

  it("test04_no_profile_means_no_apply", async () => {
    open({ scenario: "never" });
    const plan = await openPlan();
    for (const box of await within(plan).findAllByRole("checkbox"))
      await userEvent.click(box);
    expect(within(plan).getByRole("alert")).toHaveTextContent(
      "Choose at least one profile",
    );
    expect(within(plan).getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  it("test05_a_key_that_holds_another_value_is_skipped_unless_the_person_replaces_it", async () => {
    open({ scenario: "never", conflict: true });
    const plan = await openPlan();
    const choice = await within(plan).findByRole("group", {
      name: "OTEL_METRICS_EXPORTER",
    });
    expect(choice).toHaveTextContent("prometheus");
    expect(within(choice).getByRole("radio", { name: /Skip this key/ })).toBeChecked();
    await userEvent.click(within(plan).getByRole("button", { name: "Apply 9 changes" }));
    await waitFor(() => expect(wrote("setup apply")).toHaveLength(1));
    expect(wrote("setup apply")[0]).toBe("spend setup apply");
    expect(toast.warning).toHaveBeenCalledWith(
      "Collecting is on, but a key was kept",
      expect.anything(),
    );
  });

  it("test06_replacing_a_conflicting_key_sends_overwrite_for_that_key_only", async () => {
    open({ scenario: "never", conflict: true });
    const plan = await openPlan();
    const choice = await within(plan).findByRole("group", {
      name: "OTEL_METRICS_EXPORTER",
    });
    await userEvent.click(within(choice).getByRole("radio", { name: /Replace it/ }));
    await userEvent.click(within(plan).getByRole("button", { name: "Apply 10 changes" }));
    await waitFor(() => expect(wrote("setup apply")).toHaveLength(1));
    expect(wrote("setup apply")[0]).toBe(
      "spend setup apply --overwrite OTEL_METRICS_EXPORTER",
    );
    expect(toast.success).toHaveBeenCalledWith("Collecting is on", expect.anything());
  });

  it("test07_a_failed_apply_stays_open_with_the_cause_and_reads_the_plan_again", async () => {
    open({ scenario: "never", applyError: "settings.json is read-only" });
    const plan = await openPlan();
    await userEvent.click(await within(plan).findByRole("button", { name: /^Apply/ }));
    expect(await within(plan).findByRole("alert")).toHaveTextContent(
      "settings.json is read-only",
    );
    expect(within(plan).getByRole("button", { name: /^Try again/ })).toBeEnabled();
    await waitFor(() =>
      expect(joined().filter((line) => line === "spend setup plan").length).toBe(2),
    );
    expect(screen.getByTestId("spend-strip")).not.toHaveTextContent("Waiting");
  });

  it("test23_at_390_px_the_plan_is_a_bottom_sheet_with_its_footer_outside_the_scroll", async () => {
    open({ scenario: "never" });
    const plan = await openPlan();
    expect(plan).toHaveClass("max-sm:bottom-0", "max-sm:rounded-b-none");
    const footer = plan.querySelector("[data-slot=dialog-footer]");
    const body = plan.querySelector("[data-slot=dialog-body]");
    expect(footer).not.toBeNull();
    expect(body?.contains(footer)).toBe(false);
  });

  it("test08_escape_closes_the_plan_without_writing", async () => {
    open({ scenario: "never" });
    await openPlan();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(wrote("setup apply")).toEqual([]);
  });
});

async function openUndo() {
  await page();
  await userEvent.click(screen.getByRole("button", { name: "Undo setup…" }));
  return dialog();
}

describe("the undo dialog", () => {
  it("test09_undo_keeps_the_collected_data_by_default", async () => {
    open();
    const undo = await openUndo();
    const keep = await within(undo).findByRole("radio", { name: /Keep it/ });
    expect(keep).toBeChecked();
    expect(undo).toHaveTextContent("41,206 events");
    await userEvent.click(within(undo).getByRole("button", { name: "Undo setup" }));
    expect(await within(undo).findByText("Collecting is off.")).toBeInTheDocument();
    expect(wrote("setup undo")).toEqual(["spend setup undo"]);
    expect(undo).toHaveTextContent("The collected data is kept");
    expect(toast.success).toHaveBeenCalledWith("Collecting is off", expect.anything());
    await userEvent.click(within(undo).getByRole("button", { name: "Done" }));
    await waitFor(() =>
      expect(screen.getByTestId("spend-strip")).toHaveTextContent("Collecting is off"),
    );
  });

  it("test10_deleting_the_data_needs_a_second_confirmation_and_sends_yes", async () => {
    open();
    const undo = await openUndo();
    await userEvent.click(await within(undo).findByRole("radio", { name: /Delete it/ }));
    const confirm = within(undo).getByRole("button", {
      name: "Undo setup and delete data",
    });
    expect(confirm).toBeDisabled();
    await userEvent.click(within(undo).getByRole("checkbox", { name: /I understand/ }));
    await userEvent.click(confirm);
    expect(await within(undo).findByText(/Deleted 41,206 events/)).toBeInTheDocument();
    expect(wrote("setup undo")).toEqual(["spend setup undo --delete-data --yes"]);
  });

  it("test11_a_key_changed_since_setup_is_listed_as_left", async () => {
    open();
    const undo = await openUndo();
    await userEvent.click(
      await within(undo).findByRole("button", { name: "Undo setup" }),
    );
    expect(await within(undo).findByText(/5 keys removed/)).toBeInTheDocument();
  });
});

async function openPrune() {
  await page();
  await userEvent.click(screen.getByRole("button", { name: "Prune now…" }));
  return dialog();
}

describe("the prune dialog", () => {
  it("test12_prune_shows_what_goes_and_what_stays_before_it_runs", async () => {
    open();
    const prune = await openPrune();
    const run = await within(prune).findByRole("button", { name: "Prune 5,120 events" });
    expect(prune).toHaveTextContent("5,120 events, 2.0 MB");
    expect(prune).toHaveTextContent("41,206 events, 18.4 MB");
    expect(wrote("prune")).toEqual([]);
    await userEvent.click(run);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(wrote("prune")).toEqual(["spend prune"]);
    expect(toast.success).toHaveBeenCalledWith("Pruned 5,120 events");
  });

  it("test13_the_old_282_mb_events_file_is_named_before_it_goes", async () => {
    open({ oldEventsFile: true });
    const prune = await openPrune();
    await within(prune).findByRole("button", { name: "Prune 1,930,000 events" });
    expect(prune).toHaveTextContent("1,930,000 events, 282.0 MB");
  });

  it("test14_deleting_the_daily_totals_is_its_own_checked_choice", async () => {
    open();
    const prune = await openPrune();
    await within(prune).findByRole("button", { name: /^Prune 5,120/ });
    await userEvent.click(
      within(prune).getByRole("checkbox", { name: /Also delete the daily totals/ }),
    );
    await userEvent.click(
      within(prune).getByRole("button", { name: "Prune events and delete totals" }),
    );
    await waitFor(() => expect(wrote("prune")).toEqual(["spend prune --totals --yes"]));
    expect(toast.success).toHaveBeenCalledWith(
      "Pruned the events and deleted the daily totals",
    );
  });
});

describe("the export dialog", () => {
  it("test15_e_opens_the_export_with_the_range_and_the_row_count", async () => {
    vi.mocked(save).mockResolvedValue("/Users/example/Downloads/spend.csv");
    open();
    await page();
    await userEvent.keyboard("3");
    await userEvent.keyboard("e");
    const exporting = await dialog();
    expect(within(exporting).getByRole("radio", { name: "30 days" })).toBeChecked();
    const run = await within(exporting).findByRole("button", { name: "Export 1 row" });
    await userEvent.click(run);
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: "toolport-spend-2026-11-18.csv" }),
    );
    await waitFor(() =>
      expect(joined()).toContain(
        "spend export-csv --range 30d --out /Users/example/Downloads/spend.csv",
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(toast.success).toHaveBeenCalledWith(
      "Exported 1 row",
      expect.objectContaining({ description: "/Users/example/Downloads/spend.csv" }),
    );
  });

  it("test16_cancelling_the_save_dialog_exports_nothing_and_keeps_the_dialog", async () => {
    vi.mocked(save).mockResolvedValue(null);
    open();
    await page();
    await userEvent.click(screen.getByRole("button", { name: "Export CSV E" }));
    const exporting = await dialog();
    await userEvent.click(
      await within(exporting).findByRole("button", { name: /^Export 1/ }),
    );
    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(joined().filter((line) => line.includes("--out"))).toEqual([]);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("test17_changing_the_range_counts_the_rows_again", async () => {
    open();
    await page();
    await userEvent.click(screen.getByRole("button", { name: "Export CSV E" }));
    const exporting = await dialog();
    await userEvent.click(within(exporting).getByRole("radio", { name: "Today" }));
    await waitFor(() => expect(joined()).toContain("spend export-csv --range today"));
  });

  it("test18_a_filtered_page_says_the_file_is_not_narrowed", async () => {
    render(<ExportDialog range="7d" filtered onClose={vi.fn()} onExported={vi.fn()} />);
    expect(await screen.findByRole("dialog")).toHaveTextContent(
      "does not narrow the file",
    );
  });
});

describe("the port dialog", () => {
  async function openPort() {
    await page();
    await userEvent.click(await screen.findByRole("button", { name: "Use port 4319…" }));
    return dialog();
  }

  it("test19_another_port_shows_the_endpoint_change_then_moves_the_receiver", async () => {
    open({ scenario: "cannot-listen" });
    const port = await openPort();
    await waitFor(() =>
      expect(port).toHaveTextContent(
        '"OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4319"',
      ),
    );
    await userEvent.click(within(port).getByRole("button", { name: "Use port 4319" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(joined().filter((l) => l.startsWith("spend config set"))).toEqual([
      "spend config set --port 4319",
    ]);
    expect(wrote("setup apply")).toEqual([
      `spend setup apply --port 4319 --profile ${PROFILE_PERSONAL}`,
    ]);
    expect(toast.success).toHaveBeenCalledWith(
      "The receiver listens on 127.0.0.1:4319",
      expect.anything(),
    );
    await waitFor(() =>
      expect(screen.getByTestId("spend-strip")).not.toHaveTextContent("not listening"),
    );
  });

  it("test20_a_busy_port_changes_nothing_and_another_can_be_tried", async () => {
    open({ scenario: "cannot-listen", busyPorts: [4318, 4319] });
    const port = await openPort();
    await userEvent.click(
      await within(port).findByRole("button", { name: "Use port 4319" }),
    );
    expect(await within(port).findByRole("alert")).toHaveTextContent(
      "Port 4319 cannot be used",
    );
    expect(joined().filter((l) => l.startsWith("spend config set"))).toEqual([
      "spend config set --port 4319",
      "spend config set --port 4318",
    ]);
    expect(wrote("setup apply")).toEqual([]);
    const field = within(port).getByRole("textbox", { name: "Receiver port" });
    await userEvent.clear(field);
    await userEvent.type(field, "4320");
    await userEvent.click(within(port).getByRole("button", { name: "Use port 4320" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(wrote("setup apply")).toEqual([
      `spend setup apply --port 4320 --profile ${PROFILE_PERSONAL}`,
    ]);
  });

  it("test21_a_port_below_1024_is_refused_before_anything_runs", async () => {
    open({ scenario: "cannot-listen" });
    const port = await openPort();
    const field = within(port).getByRole("textbox", { name: "Receiver port" });
    await userEvent.clear(field);
    await userEvent.type(field, "80");
    expect(within(port).getByRole("alert")).toHaveTextContent("1024 to 65535");
    expect(within(port).getByRole("button", { name: "Use port" })).toBeDisabled();
  });
});

describe("the dialogs together", () => {
  it("test22_the_parity_rows_name_this_file_as_their_proof", () => {
    for (const id of COVERS)
      expect(guiParity.actions[id].test).toBe("src/plus/spend/SpendDialogs.test.tsx");
  });
});
