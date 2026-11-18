import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SPEND_NOW } from "../fixtures/spendWorld";
import { spendState } from "./model";
import { SpendStrip } from "./SpendStrip";
import { readWorld } from "./testkit";

function setup(options = {}, argv: string[] = [], summaryReady = true) {
  const { status, summary } = readWorld(options);
  void argv;
  const handlers = {
    onCancel: vi.fn(),
    onRetry: vi.fn(),
    onSetup: vi.fn(),
    onUsePort: vi.fn(),
  };
  const state = spendState(status, summaryReady);
  render(
    <SpendStrip
      state={state}
      status={status}
      summary={summary}
      now={SPEND_NOW}
      elapsedMs={14_000}
      {...handlers}
    />,
  );
  return { status, state, ...handlers };
}

const strip = () => screen.getByTestId("spend-strip");

describe("SpendStrip", () => {
  it("test01_a_collecting_machine_shows_receiver_last_event_size_and_since", () => {
    setup();
    expect(strip()).toHaveTextContent(
      "Receiver on 127.0.0.1:4318 · last event 14 s ago · 18.4 of 64 MB · since 11-02",
    );
    expect(within(strip()).getByText("Collecting")).toBeInTheDocument();
    expect(within(strip()).getByText("OTel primary")).toBeInTheDocument();
    expect(
      within(strip()).getByRole("button", { name: "Collecting details" }),
    ).toBeInTheDocument();
  });

  it("test02_never_set_up_has_the_primary_action_and_no_other", async () => {
    const { onSetup } = setup({ scenario: "never" });
    expect(strip()).toHaveTextContent("Collecting is off");
    await userEvent.click(
      within(strip()).getByRole("button", { name: "Set up collecting…" }),
    );
    expect(onSetup).toHaveBeenCalled();
  });

  it("test03_off_with_transcripts_explains_what_the_numbers_are", () => {
    setup({ scenario: "off" });
    expect(screen.getByText("Transcripts only.")).toBeInTheDocument();
    expect(
      screen.getByText(/No per-task split and no tool or MCP server detail/),
    ).toBeInTheDocument();
  });

  it("test04_waiting_for_the_first_event_asks_to_restart_open_sessions", () => {
    setup({ scenario: "waiting" });
    expect(strip()).toHaveTextContent("Waiting for the first event");
    expect(
      screen.getByText("Restart Claude Code sessions that are already open."),
    ).toBeInTheDocument();
  });

  it("test05_a_port_in_use_names_the_port_and_offers_the_next_one_and_retry", async () => {
    const { onUsePort, onRetry } = setup({ scenario: "cannot-listen" });
    expect(strip()).toHaveTextContent("Port 4318 is used by another program.");
    expect(screen.getByText(/Claude Code is exporting to 4318/)).toBeInTheDocument();
    await userEvent.click(
      within(strip()).getByRole("button", { name: "Use port 4319…" }),
    );
    expect(onUsePort).toHaveBeenCalledWith(4319);
    await userEvent.click(within(strip()).getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });

  it("test06_computing_shows_the_file_count_a_progress_bar_and_cancel", async () => {
    const { onCancel } = setup({ scenario: "computing" }, [], false);
    expect(strip()).toHaveTextContent("Reading transcripts");
    expect(strip()).toHaveTextContent(
      "of 461 files · 14 s on the first run, then cached per file",
    );
    expect(screen.getByRole("progressbar", { name: "Transcripts read" })).toHaveAttribute(
      "aria-valuenow",
    );
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
  });

  it("test07_not_exporting_lists_the_causes_that_were_checked_and_opens_the_plan", async () => {
    const { onSetup } = setup({ stale: true });
    const notice = screen.getByTestId("spend-notice-not-exporting");
    expect(notice).toHaveTextContent("Claude Code is not exporting");
    expect(notice).toHaveTextContent("12 sessions ran since");
    expect(notice).toHaveTextContent("receiver is listening");
    expect(notice).toHaveTextContent(
      "~/work/clients/alpha/api/.claude/settings.json sets OTEL_EXPORTER_OTLP_ENDPOINT",
    );
    expect(notice).toHaveTextContent("3 sessions started before setup (restart them)");
    await userEvent.click(within(notice).getByRole("button", { name: "Open plan…" }));
    expect(onSetup).toHaveBeenCalled();
  });
});
