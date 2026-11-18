import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { SPEND_NOW, type SpendWorldOptions } from "../fixtures/spendWorld";
import { invokeLog, ranArgv } from "../servers/testkit";
import { setClock } from "../tasks/clock";
import SpendPage from "./SpendPage";

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

function setup(spend: SpendWorldOptions = {}) {
  resetPlusFixture({ spend });
  return render(<SpendPage />);
}

const strip = () => screen.findByTestId("spend-strip");

beforeEach(() => {
  invokeLog.length = 0;
  setClock(SPEND_NOW);
});

afterEach(() => setClock(null));

describe("the Spend page", () => {
  it("test01_a_collecting_machine_shows_the_receiver_and_the_last_event", async () => {
    setup();
    const bar = await strip();
    expect(within(bar).getByText("Collecting")).toBeInTheDocument();
    expect(bar).toHaveTextContent("Receiver on 127.0.0.1:4318");
    expect(bar).toHaveTextContent("last event 14 s ago");
    expect(screen.getByTestId("spend-strip").closest("[data-state]")).toHaveAttribute(
      "data-state",
      "healthy",
    );
  });

  it("test02_it_reads_the_status_first_and_then_the_report_for_seven_days", async () => {
    setup();
    await strip();
    await waitFor(() => expect(ranArgv().length).toBeGreaterThanOrEqual(2));
    expect(ranArgv()[0]).toEqual(["spend", "status"]);
    await waitFor(() =>
      expect(ranArgv().some((a) => a.join(" ") === "spend summary --range 7d")).toBe(
        true,
      ),
    );
  });

  it("test03_collecting_never_set_up_offers_the_plan", async () => {
    setup({ scenario: "never" });
    const bar = await strip();
    expect(bar).toHaveTextContent("Collecting is off");
    expect(bar).toHaveTextContent("Numbers come from Claude Code's transcripts only.");
    expect(bar.closest("[data-state]")).toHaveAttribute("data-state", "neverSetUp");
  });

  it("test04_off_with_transcripts_is_a_working_page_not_a_failure", async () => {
    setup({ scenario: "off" });
    const bar = await strip();
    await waitFor(() =>
      expect(bar.closest("[data-state]")).toHaveAttribute(
        "data-state",
        "offWithTranscripts",
      ),
    );
  });

  it("test05_waiting_for_the_first_event_says_the_receiver_is_on", async () => {
    setup({ scenario: "waiting" });
    const bar = await strip();
    expect(bar).toHaveTextContent("Waiting for the first event");
    expect(bar).toHaveTextContent("Receiver on 127.0.0.1:4318 · 0 events");
  });

  it("test06_a_port_in_use_is_named_in_the_error_strip", async () => {
    setup({ scenario: "cannot-listen" });
    const bar = await strip();
    expect(bar).toHaveTextContent("Receiver is not listening");
    expect(bar).toHaveTextContent("Port 4318 is used by another program.");
  });

  it("test07_the_first_read_shows_the_file_count_until_the_report_arrives", async () => {
    setup({ scenario: "computing", scanMs: 3000 });
    const bar = await strip();
    expect(bar).toHaveTextContent("Reading transcripts");
    expect(bar).toHaveTextContent("of 461 files");
    expect(bar.closest("[data-state]")).toHaveAttribute("data-state", "computing");
  });

  it("test08_cancel_stops_waiting_and_leaves_the_status_strip", async () => {
    setup({ scenario: "computing", scanMs: 600_000 });
    await strip();
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.getByTestId("spend-strip").closest("[data-state]")).toHaveAttribute(
        "data-state",
        "offWithTranscripts",
      ),
    );
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("test09_a_failed_report_keeps_the_strip_and_offers_retry", async () => {
    setup({ summaryError: "events.jsonl cannot be read" });
    await strip();
    expect(await screen.findByText("The report could not be read")).toBeInTheDocument();
    expect(screen.getByText("events.jsonl cannot be read")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("test10_a_stale_export_is_flagged_under_the_strip", async () => {
    setup({ stale: true });
    expect(await screen.findByText("Claude Code is not exporting")).toBeInTheDocument();
    expect(screen.getByText(/12 sessions ran since/)).toBeInTheDocument();
  });
});
