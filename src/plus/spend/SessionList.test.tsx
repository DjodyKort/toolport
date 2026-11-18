import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { PROJECT_WEB } from "../fixtures/spendWorld";
import { SessionList } from "./SessionList";
import { readWorld } from "./testkit";

describe("SessionList", () => {
  it("test01_it_lists_start_task_model_length_tokens_and_cost_largest_first", () => {
    const { summary } = readWorld({}, "--project", PROJECT_WEB);
    render(<SessionList summary={summary} showCost />);
    const table = screen.getByRole("table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual(["Started", "Task", "Model", "Length", "Tokens", "Cost"]);
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("Opus");
    expect(rows[0]).toHaveTextContent("3 h 05 m");
    expect(rows[0]).toHaveTextContent("118.4M");
    expect(rows[0]).toHaveTextContent("$14.20");
    expect(rows[0]).toHaveTextContent("interactive");
    expect(rows[3]).toHaveTextContent("nightly-review");
    expect(rows[3]).toHaveTextContent("18 min");
  });

  it("test02_the_footer_counts_listed_against_all_sessions_of_the_filter", () => {
    const { summary } = readWorld({}, "--project", PROJECT_WEB);
    render(<SessionList summary={summary} showCost />);
    expect(screen.getByText(/^4 of \d+ sessions, largest first\.$/)).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Largest sessions in this folder" }),
    ).toBeInTheDocument();
  });

  it("test03_a_model_or_task_filter_titles_it_without_the_folder", () => {
    const { summary } = readWorld({}, "--model", "claude-sonnet-5-5");
    render(<SessionList summary={summary} showCost />);
    expect(screen.getByRole("heading", { name: "Largest sessions" })).toBeInTheDocument();
  });

  it("test04_without_dollar_figures_there_is_no_cost_column", () => {
    const { summary } = readWorld({ hideCost: true }, "--project", PROJECT_WEB);
    render(<SessionList summary={summary} showCost={false} />);
    expect(screen.queryByRole("columnheader", { name: "Cost" })).toBeNull();
    expect(screen.queryByText("$14.20")).toBeNull();
  });

  it("test05_a_filter_that_matches_nothing_says_so", () => {
    const { summary } = readWorld({}, "--task", "docs-sync");
    render(<SessionList summary={summary} showCost />);
    expect(
      screen.getByText("No session in this range matches the filter."),
    ).toBeInTheDocument();
  });
});
