import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { SPEND_NOW, type SpendWorldOptions } from "../fixtures/spendWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import { PLUS_NAVIGATE_EVENT } from "../shell/navigate";
import { setClock } from "../tasks/clock";
import SpendSettings from "./SpendSettings";

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

/** The action of src/plus/gui-parity/ this file proves: port, retention, size cap and the
 * Show cost switch of Settings > Toolport+ > Spend. */
const COVERS = ["spend.settings"] as const;

function open(spend: SpendWorldOptions = {}) {
  resetPlusFixture({ spend });
  invokeLog.length = 0;
  return render(<SpendSettings />);
}

const config = () =>
  ranArgv()
    .map((argv) => argv.join(" "))
    .filter((line) => line.startsWith("spend config set"));

beforeEach(() => {
  vi.clearAllMocks();
  setClock(SPEND_NOW);
});

describe("Settings > Toolport+ > Spend", () => {
  it("test01_the_rows_show_the_stored_settings_and_collecting_state", async () => {
    open();
    expect(await screen.findByRole("textbox", { name: "Receiver port" })).toHaveValue(
      "4318",
    );
    expect(screen.getByRole("textbox", { name: "Days to keep events" })).toHaveValue(
      "30",
    );
    expect(screen.getByRole("textbox", { name: "Size cap in MB" })).toHaveValue("64");
    expect(screen.getByRole("switch", { name: "Show cost" })).toBeChecked();
    expect(screen.getByText("On")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Undo setup…" })).toBeInTheDocument();
  });

  it("test02_show_cost_is_written_at_once_and_says_so", async () => {
    open();
    await userEvent.click(await screen.findByRole("switch", { name: "Show cost" }));
    await waitFor(() => expect(config()).toEqual(["spend config set --show-cost off"]));
    expect(toast.success).toHaveBeenCalledWith("Cost is hidden");
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Show cost" })).not.toBeChecked(),
    );
  });

  it("test03_retention_and_size_cap_stay_inside_the_bounds_the_command_takes", async () => {
    open();
    const days = await screen.findByRole("textbox", { name: "Days to keep events" });
    const cap = screen.getByRole("textbox", { name: "Size cap in MB" });
    const save = screen.getAllByRole("button", { name: "Save" })[1];
    expect(save).toBeDisabled();
    await userEvent.clear(days);
    await userEvent.type(days, "100");
    expect(days).toHaveAttribute("aria-invalid", "true");
    expect(save).toBeDisabled();
    await userEvent.clear(days);
    await userEvent.type(days, "14");
    await userEvent.clear(cap);
    await userEvent.type(cap, "8");
    expect(cap).toHaveAttribute("aria-invalid", "true");
    expect(save).toBeDisabled();
    await userEvent.clear(cap);
    await userEvent.type(cap, "128");
    await userEvent.click(save);
    await waitFor(() =>
      expect(config()).toEqual(["spend config set --retention-days 14 --max-mb 128"]),
    );
    expect(toast.success).toHaveBeenCalledWith("Retention saved");
  });

  it("test04_a_port_with_nothing_set_up_is_saved_without_a_plan", async () => {
    open({ scenario: "off" });
    const field = await screen.findByRole("textbox", { name: "Receiver port" });
    await userEvent.clear(field);
    await userEvent.type(field, "4400");
    await userEvent.click(screen.getAllByRole("button", { name: "Save" })[0]);
    await waitFor(() => expect(config()).toEqual(["spend config set --port 4400"]));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("test05_a_port_with_profiles_set_up_opens_the_plan_to_move_claude_code_too", async () => {
    open();
    const field = await screen.findByRole("textbox", { name: "Receiver port" });
    await userEvent.clear(field);
    await userEvent.type(field, "4400");
    await userEvent.click(screen.getAllByRole("button", { name: "Save" })[0]);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Use another port");
    expect(config()).toEqual([]);
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Use port 4400" }),
    );
    await waitFor(() => expect(config()).toEqual(["spend config set --port 4400"]));
  });

  it("test06_set_up_opens_the_plan_when_collecting_is_off", async () => {
    open({ scenario: "off" });
    await userEvent.click(await screen.findByRole("button", { name: "Set up…" }));
    expect(await screen.findByRole("dialog")).toHaveTextContent(
      "Turn on spend collecting",
    );
  });

  it("test07_the_link_opens_the_spend_page", async () => {
    const seen = vi.fn();
    window.addEventListener(PLUS_NAVIGATE_EVENT, (event) =>
      seen((event as CustomEvent).detail),
    );
    open();
    await userEvent.click(await screen.findByRole("button", { name: "Open Spend" }));
    expect(seen).toHaveBeenCalledWith("spend");
  });

  it("test08_a_status_that_cannot_be_read_offers_retry", async () => {
    resetPlusFixture({ spend: {}, down: true });
    render(<SpendSettings />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Spend settings could not be read",
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("test10_daily_totals_switch_off_and_delete_totals_opens_the_prune_choice", async () => {
    open();
    const totals = await screen.findByRole("switch", { name: "Keep daily totals" });
    expect(totals).toBeChecked();
    await userEvent.click(totals);
    await waitFor(() =>
      expect(config()).toEqual(["spend config set --daily-totals off"]),
    );
    expect(toast.success).toHaveBeenCalledWith("Daily totals are no longer kept");
    await userEvent.click(screen.getByRole("button", { name: "Delete totals…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByRole("checkbox", {
        name: /Also delete the daily totals/,
      }),
    ).toBeChecked();
    expect(dialog).toHaveTextContent("Daily totals are off");
  });

  it("test11_the_weekly_measure_row_turns_on_through_a_plan_dialog_and_off_at_once", async () => {
    open();
    const row = await screen.findByRole("switch", { name: "Weekly measure" });
    expect(row).not.toBeChecked();
    await userEvent.click(row);
    const dialog = await screen.findByRole("dialog");
    await waitFor(() =>
      expect(
        within(dialog).getByRole("textbox", { name: "Folder to measure" }),
      ).toHaveValue("/Users/example/work/clients/alpha/web-app"),
    );
    expect(dialog).toHaveTextContent("Every Monday at 09:00");
    expect(config()).toEqual(["spend config set --weekly-measure on --dry-run"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Turn on" }));
    await waitFor(() =>
      expect(config()).toContain(
        "spend config set --weekly-measure on --measure-folder /Users/example/work/clients/alpha/web-app",
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Weekly measure" })).toBeChecked(),
    );
    await userEvent.click(screen.getByRole("switch", { name: "Weekly measure" }));
    await waitFor(() =>
      expect(config()).toContain("spend config set --weekly-measure off"),
    );
    expect(toast.success).toHaveBeenCalledWith("The weekly measure is off");
  });

  it("test09_the_parity_row_names_this_file_as_its_proof", () => {
    for (const id of COVERS)
      expect(guiParity.actions[id].test).toBe("src/plus/spend/SpendSettings.test.tsx");
  });
});
