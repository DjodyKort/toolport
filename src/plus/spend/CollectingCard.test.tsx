import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SPEND_NOW } from "../fixtures/spendWorld";
import { CollectingCardView } from "./CollectingCard";
import { readWorld } from "./testkit";

function setup(options = {}, closedDays: string[] = []) {
  const { status } = readWorld(options);
  const handlers = { onExport: vi.fn(), onPrune: vi.fn(), onUndo: vi.fn() };
  render(
    <CollectingCardView
      status={status}
      now={SPEND_NOW}
      closedDays={closedDays}
      {...handlers}
    />,
  );
  return { status, ...handlers };
}

const card = () => screen.getByRole("region", { name: "Collecting" });

describe("CollectingCardView", () => {
  it("test01_it_says_where_the_receiver_listens_and_how_full_the_store_is", () => {
    setup({}, ["2026-11-14", "2026-11-15"]);
    expect(within(card()).getByText("On")).toBeInTheDocument();
    expect(card()).toHaveTextContent(
      "Listening on 127.0.0.1:4318 (this computer only) · started with the Toolport app · 0 events dropped",
    );
    expect(card()).toHaveTextContent("18.4 of 64 MB · 41,206 events");
    expect(card()).toHaveTextContent("16 of 30 days · oldest 11-02");
    expect(card()).toHaveTextContent(
      "transcripts fill the days before and the days the app was closed (11-14, 11-15)",
    );
    expect(screen.getByRole("img", { name: /18\.4 of 64 MB/ })).toBeInTheDocument();
  });

  it("test02_it_says_which_cap_is_reached_first", () => {
    setup();
    expect(card()).toHaveTextContent("At this rate the 30-day limit is reached first");
  });

  it("test03_it_says_what_is_stored_and_what_never_is", () => {
    setup();
    expect(card()).toHaveTextContent("Stored per event:");
    expect(card()).toHaveTextContent(
      "Never stored: prompts, tool arguments or results, file contents.",
    );
    expect(card()).toHaveTextContent("62 lines kept after events are pruned");
  });

  it("test04_export_prune_and_undo_call_their_handlers", async () => {
    const { onExport, onPrune, onUndo } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Export CSV…" }));
    await userEvent.click(screen.getByRole("button", { name: "Prune now…" }));
    await userEvent.click(screen.getByRole("button", { name: "Undo setup…" }));
    expect([onExport, onPrune, onUndo].map((f) => f.mock.calls.length)).toEqual([
      1, 1, 1,
    ]);
  });

  it("test05_show_lists_the_settings_files_that_were_written", async () => {
    setup();
    expect(card()).toHaveTextContent(
      "5 settings written in ~/.claude/settings.json on 11-02",
    );
    const show = screen.getByRole("button", { name: "Show" });
    expect(show).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(show);
    expect(within(card()).getByRole("list")).toHaveTextContent("~/.claude/settings.json");
    expect(screen.getByRole("button", { name: "Hide" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("test06_when_collecting_was_never_set_up_it_is_off_with_nothing_to_export_or_undo", () => {
    setup({ scenario: "never" });
    expect(within(card()).getByText("Off")).toBeInTheDocument();
    expect(card()).toHaveTextContent("Not running.");
    expect(card()).toHaveTextContent("No OpenTelemetry settings written by Toolport.");
    expect(card()).toHaveTextContent("No spend data on this computer.");
    expect(within(card()).queryByRole("button")).toBeNull();
  });

  it("test07_a_receiver_that_cannot_listen_says_so_and_keeps_its_stored_data", () => {
    setup({ scenario: "cannot-listen" });
    expect(within(card()).getByText("Not listening")).toBeInTheDocument();
    expect(card()).toHaveTextContent("Port 4318 is used by another program.");
    expect(card()).toHaveTextContent("41,206 events");
    expect(screen.getByRole("button", { name: "Export CSV…" })).toBeInTheDocument();
  });

  it("test08_waiting_for_the_first_event_is_a_warning_badge", () => {
    setup({ scenario: "waiting" });
    expect(within(card()).getByText("Waiting")).toBeInTheDocument();
  });
});
