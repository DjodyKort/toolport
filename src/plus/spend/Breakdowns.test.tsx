import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PROJECT_WEB } from "../fixtures/spendWorld";
import { BreakdownCard, Breakdowns } from "./Breakdowns";
import { NO_FILTER, type Filter } from "./model";
import { readWorld } from "./testkit";

function card(
  kind: "project" | "model" | "task",
  options = {},
  props: Partial<Parameters<typeof BreakdownCard>[0]> = {},
) {
  const { summary } = readWorld(options);
  const onPick = vi.fn();
  render(
    <BreakdownCard
      kind={kind}
      summary={summary}
      filter={NO_FILTER}
      badge="Measured"
      showCost
      onPick={onPick}
      {...props}
    />,
  );
  return { onPick, summary };
}

describe("BreakdownCard", () => {
  it("test01_project_rows_show_the_folder_cost_and_sessions_and_roll_up_the_rest", () => {
    card("project");
    const region = screen.getByRole("region", { name: "By project folder" });
    const first = within(region).getAllByRole("button")[0];
    expect(first).toHaveTextContent("~/work/clients/alpha/web-app");
    expect(first).toHaveTextContent("41 sessions");
    expect(
      within(region).getByRole("button", { name: /3 other folders/ }),
    ).toBeDisabled();
    expect(region).toHaveTextContent("Grouped by the folder Claude Code ran in.");
  });

  it("test02_a_click_picks_the_row_and_enter_does_the_same_from_the_keyboard", async () => {
    const { onPick } = card("project");
    await userEvent.click(screen.getByRole("button", { name: /web-app/ }));
    expect(onPick).toHaveBeenLastCalledWith("project", PROJECT_WEB);
    screen.getByRole("button", { name: /alpha\/api/ }).focus();
    await userEvent.keyboard("{Enter}");
    expect(onPick).toHaveBeenLastCalledWith(
      "project",
      expect.stringContaining("alpha/api"),
    );
  });

  it("test03_the_active_row_is_pressed", () => {
    card("project", {}, { filter: { ...NO_FILTER, project: PROJECT_WEB } as Filter });
    expect(screen.getByRole("button", { name: /web-app/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: /alpha\/api/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("test04_model_rows_name_the_family_with_the_model_and_requests_beneath", () => {
    card("model");
    const opus = screen.getByRole("button", { name: /Opus/ });
    expect(opus).toHaveTextContent("claude-opus-5 · 3,120 requests");
    expect(opus).toHaveTextContent("64%");
    expect(screen.getByRole("region", { name: "By model" })).toHaveTextContent(
      "Requests counted from api_request events.",
    );
  });

  it("test05_the_task_card_is_badged_estimate_and_names_interactive_work", () => {
    card("task");
    const region = screen.getByRole("region", { name: "By task" });
    expect(within(region).getByText("Estimate")).toBeInTheDocument();
    expect(region).toHaveTextContent("Interactive (no task)");
    expect(region).toHaveTextContent("Unattributed");
    expect(region).toHaveTextContent("history without a session id");
    expect(region).toHaveTextContent("task · 7 sessions");
  });

  it("test06_without_otel_the_task_card_is_locked_and_offers_the_setup", async () => {
    const onSetup = vi.fn();
    card("task", { scenario: "off" }, { locked: true, onSetup });
    expect(screen.getByText("Per task needs collecting")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Interactive/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Set up collecting…" }));
    expect(onSetup).toHaveBeenCalled();
  });

  it("test07_without_dollar_figures_rows_show_tokens", () => {
    card("project", { hideCost: true }, { showCost: false });
    expect(screen.getAllByRole("button")[0]).toHaveTextContent(/\d(M|B)/);
    expect(screen.getAllByRole("button")[0]).not.toHaveTextContent("$");
  });

  it("test08_each_row_has_a_bar_against_the_largest_row", () => {
    card("project");
    const bars = document.querySelectorAll<HTMLElement>("li i");
    expect(bars[0].style.width).toBe("100%");
    expect(Number.parseFloat(bars[1].style.width)).toBeLessThan(100);
  });
});

describe("Breakdowns", () => {
  function all(props: Partial<Parameters<typeof Breakdowns>[0]> = {}) {
    const { summary } = readWorld();
    const onPick = vi.fn();
    render(
      <Breakdowns
        kinds={["project", "model", "task"]}
        summary={summary}
        filter={NO_FILTER}
        badge="Measured"
        showCost
        taskLocked={false}
        onPick={onPick}
        {...props}
      />,
    );
    return { onPick };
  }

  it("test09_a_narrow_page_gets_tabs_and_one_open_card_a_wide_page_gets_all_three", () => {
    all();
    const tabs = screen.getByRole("tablist", { name: "Break down by" });
    expect(tabs).toHaveClass("@min-[680px]:hidden");
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((t) => t.textContent),
    ).toEqual(["Project", "Model", "Task"]);
    const panels = screen.getAllByRole("tabpanel", { hidden: true });
    expect(panels[0]).not.toHaveClass("hidden");
    expect(panels[1]).toHaveClass("hidden", "@min-[680px]:block");
    expect(panels[2]).toHaveClass("hidden", "@min-[680px]:block");
  });

  it("test10_the_tabs_move_with_arrow_keys_and_open_the_matching_card", async () => {
    all();
    screen.getByRole("tab", { name: "Project" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    const model = screen.getByRole("tab", { name: "Model" });
    expect(model).toHaveAttribute("aria-selected", "true");
    expect(model).toHaveFocus();
    const panels = screen.getAllByRole("tabpanel", { hidden: true });
    expect(panels[1]).not.toHaveClass("hidden");
    expect(panels[0]).toHaveClass("hidden");
  });

  it("test11_a_filtered_list_drops_its_card_and_its_tab", () => {
    all({ kinds: ["model", "task"] });
    expect(screen.queryByRole("tab", { name: "Project" })).toBeNull();
    expect(screen.queryByRole("region", { name: "By project folder" })).toBeNull();
  });

  it("test12_a_three_card_row_changes_columns_at_the_container_widths_of_the_design", () => {
    all();
    const panels = screen.getAllByRole("tabpanel", { hidden: true });
    expect(panels[0].parentElement).toHaveClass(
      "@min-[680px]:grid-cols-2",
      "@min-[780px]:grid-cols-3",
    );
  });
});
