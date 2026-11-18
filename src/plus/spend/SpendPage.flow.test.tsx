import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { SPEND_NOW, type SpendWorldOptions } from "../fixtures/spendWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import { PLUS_NAVIGATE_EVENT } from "../shell/navigate";
import { setClock } from "../tasks/clock";
import { NO_FILTER } from "./model";
import SpendPage, { type SpendPageProps } from "./SpendPage";

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

function setup(spend: SpendWorldOptions = {}, props: SpendPageProps = {}) {
  resetPlusFixture({ spend });
  return render(<SpendPage {...props} />);
}

/** The action of src/plus/gui-parity/ this file proves: the page itself (spend.overview). */
const COVERS = ["spend.overview"] as const;

const summaries = () =>
  ranArgv()
    .filter((argv) => argv[1] === "summary")
    .map((argv) => argv.slice(2).join(" "));

const ready = () => screen.findByRole("radiogroup", { name: "Range" });
const card = (name: string) => screen.getByRole("region", { name });

beforeEach(() => {
  invokeLog.length = 0;
  setClock(SPEND_NOW);
});

afterEach(() => setClock(null));

describe("the Spend page, collecting", () => {
  it("test01_a_healthy_page_has_every_card_in_the_order_of_the_design", async () => {
    setup();
    await ready();
    const names = [
      "Cost by day",
      "Cache vs fresh tokens",
      "By project folder",
      "By model",
      "By task",
      "MCP part",
      "Collecting",
    ];
    const regions = names.map((name) => card(name));
    const order = regions.map((region) =>
      Array.from(document.querySelectorAll("section")).indexOf(region as HTMLElement),
    );
    expect(order).toEqual([...order].sort((a, b) => a - b));
    expect(screen.getByRole("radio", { name: /7 days/ })).toBeChecked();
  });

  it("test02_a_tile_picks_the_range_and_the_report_is_read_again", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("radio", { name: /30 days/ }));
    await waitFor(() => expect(summaries()).toContain("--range 30d"));
    expect(screen.getByRole("radio", { name: /30 days/ })).toBeChecked();
  });

  it("test03_the_keys_1_2_3_pick_today_7_days_and_30_days", async () => {
    setup();
    await ready();
    await userEvent.keyboard("1");
    expect(screen.getByRole("radio", { name: /Today/ })).toBeChecked();
    await userEvent.keyboard("3");
    expect(screen.getByRole("radio", { name: /30 days/ })).toBeChecked();
    await userEvent.keyboard("2");
    expect(screen.getByRole("radio", { name: /7 days/ })).toBeChecked();
  });

  it("test04_t_shows_the_chart_as_a_table_and_back", async () => {
    setup();
    await ready();
    expect(screen.queryByRole("table", { name: /Cost by day/ })).toBeNull();
    await userEvent.keyboard("t");
    expect(
      screen.getByRole("table", { name: "Cost by day, 7 days" }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Table/ }));
    expect(screen.queryByRole("table", { name: /Cost by day/ })).toBeNull();
  });

  it("test05_export_without_a_dialog_reads_the_csv_through_the_export_command", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Export CSV E" }));
    await waitFor(() =>
      expect(ranArgv().some((a) => a.join(" ") === "spend export-csv --range 7d")).toBe(
        true,
      ),
    );
  });

  it("test06_e_and_the_button_hand_the_range_to_the_export_handler_instead", async () => {
    const onExport = vi.fn();
    setup({}, { onExport });
    await ready();
    await userEvent.keyboard("3");
    await userEvent.keyboard("e");
    expect(onExport).toHaveBeenLastCalledWith("30d", NO_FILTER);
    await userEvent.click(screen.getByRole("button", { name: "Export CSV…" }));
    expect(onExport).toHaveBeenCalledTimes(2);
    expect(ranArgv().some((a) => a[1] === "export-csv")).toBe(false);
  });

  it("test07_keys_typed_in_a_dialog_or_a_menu_do_not_reach_the_page", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /Folder: All folders/ }));
    await userEvent.keyboard("3");
    expect(screen.getByRole("radio", { name: /7 days/, hidden: true })).toBeChecked();
  });
});

describe("the Spend page, filters", () => {
  it("test08_a_folder_row_filters_the_page_and_adds_the_session_list", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /web-app/ }));
    await waitFor(() =>
      expect(summaries().some((s) => s.includes("--project"))).toBe(true),
    );
    expect(
      await screen.findByRole("heading", { name: "Largest sessions in this folder" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Clear filter: ~/work/clients/alpha/web-app" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText(/Filtered\. \d+ of \d+ sessions, 39% of the spend\./),
    ).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "By project folder" })).toBeNull();
  });

  it("test09_escape_clears_the_filter_and_the_page_is_whole_again", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /web-app/ }));
    await screen.findByRole("heading", { name: "Largest sessions in this folder" });
    await userEvent.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: /Largest sessions/ })).toBeNull(),
    );
    expect(screen.getByRole("region", { name: "By project folder" })).toBeInTheDocument();
  });

  it("test10_model_and_task_rows_filter_too_and_the_filters_combine", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /Opus/ }));
    await screen.findByRole("button", { name: "Clear filter: Opus" });
    await userEvent.click(
      screen.getByRole("button", { name: /Interactive \(no task\)/ }),
    );
    await waitFor(() =>
      expect(
        summaries().some(
          (s) => s.includes("--model claude-opus-5") && s.includes("--task Interactive"),
        ),
      ).toBe(true),
    );
    expect(
      await screen.findByRole("button", { name: "Clear filter: Interactive (no task)" }),
    ).toBeInTheDocument();
    const rows = within(await screen.findByRole("table", { name: "" })).getAllByRole(
      "row",
    );
    expect(rows.length).toBeGreaterThan(1);
  });

  it("test11_the_clear_button_of_a_chip_removes_only_that_filter", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /Opus/ }));
    await userEvent.click(
      screen.getByRole("button", { name: /Interactive \(no task\)/ }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Clear filter: Opus" }),
    );
    expect(screen.queryByRole("button", { name: "Clear filter: Opus" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Clear filter: Interactive (no task)" }),
    ).toBeInTheDocument();
  });

  it("test12_the_folder_menu_in_the_header_filters_like_the_row_does", async () => {
    setup();
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /Folder: All folders/ }));
    await userEvent.click(
      screen.getByRole("menuitemradio", { name: "~/work/clients/alpha/api" }),
    );
    await waitFor(() =>
      expect(summaries().some((s) => s.includes("alpha/api"))).toBe(true),
    );
    expect(
      await screen.findByRole("button", {
        name: "Folder: ~/work/clients/alpha/api. Change folder",
      }),
    ).toBeInTheDocument();
  });
});

describe("the Spend page, states", () => {
  it("test14_never_set_up_is_an_empty_state_with_the_mcp_and_collecting_cards", async () => {
    const onSetup = vi.fn();
    setup({ scenario: "never" }, { onSetup });
    expect(await screen.findByText("Nothing to show yet")).toBeInTheDocument();
    expect(await screen.findByRole("region", { name: "MCP part" })).toBeInTheDocument();
    expect(card("Collecting")).toHaveTextContent("No spend data on this computer.");
    expect(screen.queryByRole("radiogroup", { name: "Range" })).toBeNull();
    await userEvent.click(
      within(screen.getByRole("status", { name: "" }) ?? document.body).getByRole(
        "button",
        {
          name: "Set up collecting…",
        },
      ),
    );
    expect(onSetup).toHaveBeenCalled();
  });

  it("test15_off_with_transcripts_marks_estimates_locks_the_task_card_and_offers_setup", async () => {
    const onSetup = vi.fn();
    setup({ scenario: "off" }, { onSetup });
    await ready();
    expect(screen.getByText("Transcripts only.")).toBeInTheDocument();
    expect(screen.getByText("Per task needs collecting")).toBeInTheDocument();
    expect(card("Cost by day")).toHaveTextContent("Estimate");
    expect(
      screen.getByText("Want exact cost, per-task and per-folder detail?"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getAllByRole("button", { name: "Set up collecting…" }).at(-1) as HTMLElement,
    );
    expect(onSetup).toHaveBeenCalled();
  });

  it("test16_waiting_for_the_first_event_shows_transcript_numbers_and_the_restart_hint", async () => {
    setup({ scenario: "waiting" });
    await ready();
    expect(
      screen.getByText("Restart Claude Code sessions that are already open."),
    ).toBeInTheDocument();
    expect(card("Collecting")).toHaveTextContent("Waiting");
  });

  it("test17_a_receiver_that_cannot_listen_keeps_the_stored_data_on_the_page", async () => {
    const onUsePort = vi.fn();
    setup({ scenario: "cannot-listen" }, { onUsePort });
    await ready();
    expect(screen.getByTestId("spend-strip")).toHaveTextContent(
      "Port 4318 is used by another program.",
    );
    expect(card("Cost by day")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Use port 4319…" }));
    expect(onUsePort).toHaveBeenCalledWith(4319, 4318);
  });

  it("test18_computing_shows_skeleton_tiles_with_the_count_and_fills_in_when_done", async () => {
    setup({ scenario: "computing", scanMs: 400 });
    expect(await screen.findByTestId("spend-strip")).toHaveTextContent(
      "Reading transcripts",
    );
    expect(document.querySelector("[data-slot=range-tiles-skeleton]")).not.toBeNull();
    expect(document.querySelector("[data-slot=cost-chart-skeleton]")).not.toBeNull();
    await ready();
    expect(document.querySelector("[data-slot=range-tiles-skeleton]")).toBeNull();
    expect(screen.getByTestId("spend-strip")).not.toHaveTextContent(
      "Reading transcripts",
    );
  });

  it("test19_cancel_leaves_a_card_that_offers_to_read_again", async () => {
    setup({ scenario: "computing", scanMs: 600_000 });
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(
      await screen.findByText("Reading the transcripts was stopped"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Read again" })).toBeInTheDocument();
  });

  it("test20_one_failing_card_leaves_the_rest_of_the_page_unchanged", async () => {
    setup({ mcpError: true });
    await ready();
    expect(card("MCP part")).toHaveTextContent(
      "Upstream's savings log could not be read.",
    );
    expect(card("Cost by day")).toBeInTheDocument();
    expect(card("Collecting")).toHaveTextContent("Listening on 127.0.0.1:4318");
    const before = summaries().length;
    await userEvent.click(
      within(card("MCP part")).getByRole("button", { name: "Retry" }),
    );
    await waitFor(() => expect(summaries().length).toBeGreaterThan(before));
  });

  it("test21_a_stale_export_lists_the_causes_under_the_strip", async () => {
    setup({ stale: true });
    await ready();
    const notice = await screen.findByTestId("spend-notice-not-exporting");
    expect(notice).toHaveTextContent("receiver is listening");
    expect(notice).toHaveTextContent("3 sessions started before setup");
  });

  it("test22_without_dollar_figures_the_page_speaks_in_tokens", async () => {
    setup({ hideCost: true });
    await ready();
    expect(card("Tokens by day")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /7 days/ })).not.toHaveTextContent("$");
    expect(
      within(card("By project folder")).getAllByRole("button")[0],
    ).not.toHaveTextContent("$");
  });
});

describe("the Spend page, narrow layout", () => {
  it("test23_the_page_is_a_container_and_every_layout_switch_is_a_container_width", async () => {
    setup();
    await ready();
    const root = document.querySelector("[data-state]") as HTMLElement;
    expect(root).toHaveClass("@container");
    expect(document.querySelector("[data-slot=range-tiles]")).toHaveClass(
      "grid-cols-2",
      "@min-[680px]:grid-cols-4",
    );
    expect(screen.getByRole("tablist", { name: "Break down by" })).toHaveClass(
      "@min-[680px]:hidden",
    );
    expect(screen.getByRole("button", { name: "Export CSV E" })).toHaveClass("hidden");
    expect(
      screen.getByRole("button", { name: "More: Export CSV, Prune now" }),
    ).toHaveClass("@min-[680px]:hidden");
  });

  it("test24_the_chart_and_cache_cards_stack_until_the_page_is_wide_enough", async () => {
    setup();
    await ready();
    const row = card("Cost by day").parentElement as HTMLElement;
    expect(row).toHaveClass(
      "grid",
      "@min-[680px]:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]",
    );
    expect(row).not.toHaveClass("grid-cols-2");
  });
});

describe("the Spend page, small controls", () => {
  it("test25_how_it_works_explains_the_plan_first_rule_on_the_empty_state", async () => {
    setup({ scenario: "never" });
    const how = await screen.findByRole("button", { name: "How it works" });
    expect(how).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/Toolport's settings writer/)).toBeNull();
    await userEvent.click(how);
    expect(how).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(/Toolport's settings writer/)).toBeInTheDocument();
    await userEvent.click(how);
    expect(screen.queryByText(/Toolport's settings writer/)).toBeNull();
  });

  it("test26_open_measurements_opens_the_context_page", async () => {
    const seen = vi.fn();
    const listener = (event: Event) => seen((event as CustomEvent).detail);
    window.addEventListener(PLUS_NAVIGATE_EVENT, listener);
    setup();
    await ready();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open measurements" }),
    );
    window.removeEventListener(PLUS_NAVIGATE_EVENT, listener);
    expect(seen).toHaveBeenCalledWith("context");
  });

  it("test27_a_failed_mcp_card_keeps_its_cause_behind_show_details", async () => {
    setup({ mcpError: true });
    await ready();
    const mcp = card("MCP part");
    expect(mcp).not.toHaveTextContent("line 812");
    await userEvent.click(within(mcp).getByRole("button", { name: "Show details" }));
    expect(mcp).toHaveTextContent("line 812");
    expect(card("Cost by day")).toBeInTheDocument();
  });
});

describe("the Spend page, parity", () => {
  it("test28_the_parity_row_names_this_file_as_its_proof", () => {
    for (const id of COVERS)
      expect(guiParity.actions[id].test).toBe("src/plus/spend/SpendPage.flow.test.tsx");
  });
});
