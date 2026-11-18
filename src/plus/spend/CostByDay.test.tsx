import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SPEND_NOW } from "../fixtures/spendWorld";
import { CostByDay } from "./CostByDay";
import { bars } from "./model";
import { readWorld } from "./testkit";

function setup(
  options = {},
  props: Partial<Parameters<typeof CostByDay>[0]> = {},
  ...argv: string[]
) {
  const { summary } = readWorld(options, ...argv);
  const onTable = vi.fn();
  render(
    <CostByDay
      bars={bars(summary, SPEND_NOW)}
      showCost
      table={false}
      badge="Measured"
      rangeLabel="7 days"
      onTable={onTable}
      {...props}
    />,
  );
  return { onTable, summary };
}

describe("CostByDay", () => {
  it("test01_every_bar_has_a_text_label_with_the_day_and_the_amount", () => {
    setup();
    const column = screen.getByRole("img", { name: /^Tue 11-17: \$56\.20$/ });
    expect(column).toBeInTheDocument();
    expect(
      screen.getAllByRole("img").filter((el) => el.hasAttribute("data-estimate")),
    ).toHaveLength(2);
  });

  it("test02_days_from_transcripts_are_hatched_labelled_and_explained", () => {
    setup();
    expect(
      screen.getByRole("img", { name: "Sat 11-14: $6.90, estimated from transcripts" }),
    ).toHaveAttribute("data-estimate", "true");
    expect(screen.getByText("Sat and Sun are hatched:")).toBeInTheDocument();
    expect(screen.getByText(/the Toolport app was closed/)).toBeInTheDocument();
    expect(screen.getByText("6.9 est.", { exact: false })).toBeInTheDocument();
  });

  it("test03_the_legend_names_the_models_and_the_hatching", () => {
    setup();
    const legend = screen.getByRole("list", { name: "Legend" });
    for (const name of ["Opus", "Sonnet", "Haiku"])
      expect(within(legend).getByText(name)).toBeInTheDocument();
    expect(legend).toHaveTextContent(
      "Hatched = from transcripts, cost estimated from list prices",
    );
  });

  it("test04_the_table_button_toggles_and_reports_its_state", async () => {
    const { onTable } = setup();
    const button = screen.getByRole("button", { name: /Table/ });
    expect(button).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(button);
    expect(onTable).toHaveBeenCalledWith(true);
  });

  it("test05_the_table_view_lists_every_day_with_cost_tokens_and_basis", () => {
    setup({}, { table: true });
    const table = screen.getByRole("table", { name: "Cost by day, 7 days" });
    expect(within(table).getAllByRole("row")).toHaveLength(8);
    const saturday = within(table).getByRole("row", { name: /Sat 11-14/ });
    expect(saturday).toHaveTextContent("Estimate");
    expect(
      within(table).getByRole("row", { name: /Wed 11-18 \(so far\)/ }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /Tue 11-17/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Table/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("test06_thirty_days_draw_thin_bars_without_value_labels", () => {
    const { summary } = readWorld({}, "--range", "30d");
    render(
      <CostByDay
        bars={bars(summary, SPEND_NOW)}
        showCost
        table={false}
        badge="Measured"
        rangeLabel="30 days"
        onTable={() => {}}
      />,
    );
    expect(screen.getAllByRole("img").length).toBeGreaterThanOrEqual(30);
    expect(screen.queryByText("$56.2")).toBeNull();
    expect(document.querySelector("[data-slot=cost-chart]")).toHaveClass("gap-0.5");
  });

  it("test07_without_dollar_figures_the_chart_is_tokens_by_day", () => {
    const { summary } = readWorld({ hideCost: true });
    render(
      <CostByDay
        bars={bars(summary, SPEND_NOW, false)}
        showCost={false}
        table
        badge="Measured"
        rangeLabel="7 days"
        onTable={() => {}}
      />,
    );
    expect(screen.getByRole("heading", { name: "Tokens by day" })).toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "Cost" })).toBeNull();
    expect(screen.getByRole("columnheader", { name: "Tokens" })).toBeInTheDocument();
  });

  it("test08_when_every_day_comes_from_transcripts_the_footnote_says_so", () => {
    setup({ scenario: "off" });
    expect(screen.getByText("Every day is hatched:")).toBeInTheDocument();
  });
});
