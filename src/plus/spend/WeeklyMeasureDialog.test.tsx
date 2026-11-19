import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { PROJECT_WEB, SPEND_NOW, type SpendWorldOptions } from "../fixtures/spendWorld";
import { invokeLog, ranArgv } from "../servers/testkit";
import { setClock } from "../tasks/clock";
import SpendScreen from "./SpendScreen";
import SpendSettings from "./SpendSettings";

const planGate = vi.hoisted(() => ({ wait: null as Promise<void> | null }));

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      if (planGate.wait && (args?.argv as string[] | undefined)?.includes("--dry-run"))
        await planGate.wait;
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

function openSettings(spend: SpendWorldOptions = {}) {
  resetPlusFixture({ spend });
  invokeLog.length = 0;
  return render(<SpendSettings />);
}

const config = () =>
  ranArgv()
    .map((argv) => argv.join(" "))
    .filter((line) => line.startsWith("spend config set"));

async function openDialog() {
  await userEvent.click(await screen.findByRole("switch", { name: "Weekly measure" }));
  return screen.findByRole("dialog");
}

beforeEach(() => {
  vi.clearAllMocks();
  setClock(SPEND_NOW);
  planGate.wait = null;
});

describe("the weekly measure dialog", () => {
  it("test01_it_opens_on_its_heading_and_shows_the_plan_before_anything_is_written", async () => {
    openSettings();
    const dialog = await openDialog();
    const heading = within(dialog).getByRole("heading", {
      name: "Measure my context every week",
    });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(
      await within(dialog).findByRole("textbox", { name: "Folder to measure" }),
    ).toHaveValue(PROJECT_WEB);
    expect(dialog).toHaveTextContent("spend-weekly-measure");
    expect(dialog).toHaveTextContent("Every Monday at 09:00");
    expect(dialog).toHaveTextContent("haiku");
    expect(dialog).toHaveTextContent("toolportctl spend config set --weekly-measure on");
    await userEvent.keyboard("{Enter}");
    expect(config()).toEqual(["spend config set --weekly-measure on --dry-run"]);
  });

  it("test02_while_the_plan_is_made_turn_on_waits", async () => {
    let release = () => {};
    planGate.wait = new Promise<void>((resolve) => (release = resolve));
    openSettings();
    const dialog = await openDialog();
    expect(within(dialog).getByRole("status")).toHaveTextContent("Preparing the plan");
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeDisabled();
    release();
    await within(dialog).findByRole("textbox", { name: "Folder to measure" });
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeEnabled();
  });

  it("test03_a_plan_that_cannot_be_made_says_why_and_offers_no_turn_on", async () => {
    openSettings({ weeklyPlanError: "the Tasks folder is not writable" });
    const dialog = await openDialog();
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "the Tasks folder is not writable",
    );
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeDisabled();
    expect(within(dialog).queryByRole("textbox")).toBeNull();
    expect(config().filter((line) => !line.includes("--dry-run"))).toEqual([]);
  });

  it("test04_an_empty_folder_cannot_be_turned_on", async () => {
    openSettings();
    const dialog = await openDialog();
    const folder = await within(dialog).findByRole("textbox", {
      name: "Folder to measure",
    });
    await userEvent.clear(folder);
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeDisabled();
    await userEvent.type(folder, "   ");
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeDisabled();
  });

  it("test05_a_typed_folder_is_what_the_task_measures", async () => {
    openSettings();
    const dialog = await openDialog();
    const folder = await within(dialog).findByRole("textbox", {
      name: "Folder to measure",
    });
    await userEvent.clear(folder);
    await userEvent.type(folder, "/Users/example/work/other");
    await userEvent.click(within(dialog).getByRole("button", { name: "Turn on" }));
    await waitFor(() =>
      expect(config()).toContain(
        "spend config set --weekly-measure on --measure-folder /Users/example/work/other",
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(toast.success).toHaveBeenCalledWith(
      "The weekly measure is on",
      expect.objectContaining({
        description: expect.stringContaining("/Users/example/work/other"),
      }),
    );
  });

  it("test06_a_refused_turn_on_keeps_the_dialog_open_and_names_the_cause", async () => {
    openSettings({ weeklyError: "the task file could not be written" });
    const dialog = await openDialog();
    await userEvent.click(await within(dialog).findByRole("button", { name: "Turn on" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("The weekly measure was not turned on");
    expect(alert).toHaveTextContent("the task file could not be written");
    expect(screen.getByRole("dialog")).toBe(dialog);
    expect(within(dialog).getByRole("button", { name: "Turn on" })).toBeEnabled();
    expect(toast.success).not.toHaveBeenCalled();
    expect(
      screen.getByRole("switch", { name: "Weekly measure", hidden: true }),
    ).not.toBeChecked();
  });

  it("test07_cancel_and_escape_leave_the_switch_off_and_write_nothing", async () => {
    openSettings();
    const dialog = await openDialog();
    await within(dialog).findByRole("textbox", { name: "Folder to measure" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await openDialog();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("switch", { name: "Weekly measure" })).not.toBeChecked();
    expect(config().filter((line) => !line.includes("--dry-run"))).toEqual([]);
  });

  it("test08_at_390_px_it_is_a_bottom_sheet_with_both_buttons_in_the_footer", async () => {
    openSettings();
    const dialog = await openDialog();
    expect(dialog).toHaveClass("max-sm:bottom-0", "max-sm:rounded-b-none");
    const footer = dialog.querySelector("[data-slot=dialog-footer]") as HTMLElement;
    expect(footer).not.toBeNull();
    expect(within(footer).getByRole("button", { name: "Turn on" })).toBeInTheDocument();
    expect(within(footer).getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(footer.contains(within(dialog).getByRole("heading"))).toBe(false);
  });
});

describe("the weekly measure row of Settings", () => {
  it("test09_a_measure_that_is_on_names_its_folder_and_turns_off_without_a_dialog", async () => {
    openSettings({ weeklyFolder: "/Users/example/work/clients/alpha/web-app" });
    const row = await screen.findByRole("switch", { name: "Weekly measure" });
    expect(row).toBeChecked();
    expect(
      screen.getByText(/Runs context measure for ~\/work\/clients\/alpha\/web-app/),
    ).toBeInTheDocument();
    await userEvent.click(row);
    await waitFor(() =>
      expect(config()).toEqual(["spend config set --weekly-measure off"]),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("the weekly measure step when collecting is set up", () => {
  async function openPlanTicked() {
    resetPlusFixture({
      spend: { scenario: "never", weeklyApplyError: "no Tasks folder" },
    });
    invokeLog.length = 0;
    render(<SpendScreen />);
    const buttons = await screen.findAllByRole("button", { name: "Set up collecting…" });
    await userEvent.click(buttons[0]);
    const plan = await screen.findByRole("dialog");
    await userEvent.click(
      await within(plan).findByRole("checkbox", { name: /Add a weekly task/ }),
    );
    return plan;
  }

  it("test10_collecting_stays_on_when_only_the_weekly_task_could_not_be_added", async () => {
    const plan = await openPlanTicked();
    await userEvent.click(within(plan).getByRole("button", { name: /^Apply/ }));
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith(
        "Collecting is on, but the weekly measure was not added",
        expect.objectContaining({
          description: "no Tasks folder",
          action: expect.objectContaining({ label: "Undo setup…" }),
        }),
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByTestId("spend-strip")).toHaveTextContent(
        "Waiting for the first event",
      ),
    );
  });
});
