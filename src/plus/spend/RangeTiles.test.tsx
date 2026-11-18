import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RangeTiles } from "./RangeTiles";
import { readWorld } from "./testkit";

function setup(options = {}, props: Partial<Parameters<typeof RangeTiles>[0]> = {}) {
  const { summary } = readWorld(options);
  const onRange = vi.fn();
  render(
    <RangeTiles
      summary={summary}
      range="7d"
      showCost
      cacheBadge="Measured"
      onRange={onRange}
      {...props}
    />,
  );
  return { onRange, summary };
}

describe("RangeTiles", () => {
  it("test01_the_three_ranges_are_one_radio_group_with_the_selected_one_in_the_tab_order", () => {
    setup();
    const group = screen.getByRole("radiogroup", { name: "Range" });
    const radios = screen.getAllByRole("radio");
    expect(group).toContainElement(radios[0]);
    expect(radios.map((r) => r.textContent?.slice(0, 7))).toEqual([
      "Today$3",
      "7 days$",
      "30 days",
    ]);
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual([
      "false",
      "true",
      "false",
    ]);
    expect(radios.map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("test02_a_click_picks_the_range", async () => {
    const { onRange } = setup();
    await userEvent.click(screen.getByRole("radio", { name: /30 days/ }));
    expect(onRange).toHaveBeenCalledWith("30d");
  });

  it("test03_arrow_keys_move_between_ranges_and_wrap_around", async () => {
    const { onRange } = setup();
    screen.getByRole("radio", { name: /7 days/ }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onRange).toHaveBeenLastCalledWith("30d");
    await userEvent.keyboard("{ArrowLeft}");
    expect(onRange).toHaveBeenLastCalledWith("7d");
    screen.getByRole("radio", { name: /Today/ }).focus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(onRange).toHaveBeenLastCalledWith("30d");
  });

  it("test04_each_tile_shows_cost_tokens_and_the_change_against_the_period_before", () => {
    setup();
    const week = screen.getByRole("radio", { name: /7 days/ });
    expect(week).toHaveTextContent("$");
    expect(week).toHaveTextContent("vs the 7 days before");
    expect(screen.getByRole("radio", { name: /Today/ })).toHaveTextContent(
      "Yesterday $56.20 in total",
    );
    expect(screen.getByRole("radio", { name: /Today/ })).toHaveTextContent("so far");
  });

  it("test05_days_from_transcripts_are_named_on_the_tile_and_badged_estimate", () => {
    setup();
    expect(screen.getByRole("radio", { name: /7 days/ })).toHaveTextContent(
      "2 of 7 days from transcripts (estimate)",
    );
    setup({ scenario: "off" });
    expect(
      screen.getAllByText("from transcripts only (estimate)").length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText("Estimate").length).toBeGreaterThan(0);
  });

  it("test05b_a_range_with_only_some_transcript_days_is_not_badged_estimate_as_a_whole", () => {
    setup();
    expect(screen.getByRole("radio", { name: /7 days/ })).not.toHaveTextContent(
      /Estimate$/,
    );
    expect(
      document.querySelector("[data-slot=range-tiles] [data-kind=Estimate]"),
    ).toBeNull();
  });

  it("test06_the_cache_tile_is_not_a_choice_and_carries_its_kind", () => {
    setup();
    const tile = document.querySelector("[data-slot=cache-tile]") as HTMLElement;
    expect(tile).toHaveTextContent("Cache read share");
    expect(tile).toHaveTextContent("96%");
    expect(tile).toHaveTextContent("Fresh tokens: 4% (7 days)");
    expect(tile.querySelector("[data-kind=Measured]")).not.toBeNull();
    expect(tile.getAttribute("role")).toBeNull();
  });

  it("test07_without_dollar_figures_the_headline_is_tokens", () => {
    setup({ hideCost: true }, { showCost: false });
    const week = screen.getByRole("radio", { name: /7 days/ });
    expect(week).toHaveTextContent(/tokens/);
    expect(week).not.toHaveTextContent("$");
  });

  it("test08_the_tiles_are_two_by_two_on_a_narrow_page_and_four_across_when_wide", () => {
    setup();
    const grid = document.querySelector("[data-slot=range-tiles]") as HTMLElement;
    expect(grid).toHaveClass("grid-cols-2", "@min-[680px]:grid-cols-4");
  });
});
