import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { McpCardView } from "./McpCard";
import { readWorld } from "./testkit";

function setup(options = {}) {
  const { summary } = readWorld(options);
  const onRetry = vi.fn();
  const onOpenActivity = vi.fn();
  render(
    <McpCardView
      part={summary.mcpPart}
      requests={summary.totals.requests}
      onRetry={onRetry}
      onOpenActivity={onOpenActivity}
    />,
  );
  return { onRetry, onOpenActivity, summary };
}

afterEach(() => vi.restoreAllMocks());

describe("McpCardView", () => {
  it("test01_tool_definitions_compare_what_is_exposed_with_every_tool_listed", () => {
    setup();
    const card = screen.getByRole("region", { name: "MCP part" });
    expect(card).toHaveTextContent("Tool definitions per session");
    expect(card).toHaveTextContent("4.1k");
    expect(card).toHaveTextContent("median of 142 sessions");
    expect(card).toHaveTextContent("61.3k");
    expect(card).toHaveTextContent("cl100k_base");
    expect(card.querySelectorAll("[data-kind=Counted]").length).toBeGreaterThanOrEqual(2);
  });

  it("test02_the_share_is_an_estimate_without_a_dollar_figure", () => {
    setup();
    const card = screen.getByRole("region", { name: "MCP part" });
    expect(card).toHaveTextContent("Share of your input-side tokens");
    expect(card).toHaveTextContent("≈ 6%");
    expect(card).toHaveTextContent("Billing does not split a request into parts");
    expect(within(card).getByText("How this is estimated")).toBeInTheDocument();
    expect(card).not.toHaveTextContent("$");
  });

  it("test03_compression_is_counted_at_the_hook_with_its_window", () => {
    setup();
    const card = screen.getByRole("region", { name: "MCP part" });
    expect(card).toHaveTextContent("Compression, counted at the hook");
    expect(card).toHaveTextContent("1.31M");
    expect(card).toHaveTextContent("2 hooks, 2,154 calls, 11-05 to 11-18");
  });

  it("test04_the_first_request_shows_its_parts_with_the_one_nobody_itemises_hatched", () => {
    setup();
    const meter = screen.getByRole("img", { name: /Instruction files 7,910 Estimate/ });
    expect(meter).toHaveAccessibleName(
      "Instruction files 7,910 Estimate; Tools (MCP) 4,100 Counted; Everything else, not itemised 19,230 Not itemised",
    );
    const request = document.querySelector("[data-slot=first-request]") as HTMLElement;
    expect(request).toHaveTextContent("31,240 tokens");
    expect(request).toHaveTextContent("~/work/clients/alpha/web-app");
    expect(request).toHaveTextContent("context measure 11-14, claude-haiku-5");
    expect(request.querySelector("[data-kind='Not itemised']")).not.toBeNull();
    expect(request).toHaveTextContent("Claude Code does not report them separately");
  });

  it("test05_the_saved_tokens_number_of_upstream_is_named_and_never_added", () => {
    setup();
    const card = screen.getByRole("region", { name: "MCP part" });
    expect(card).toHaveTextContent("Upstream's number stays separate.");
    expect(card).toHaveTextContent("11.4M tokens saved");
    expect(card).toHaveTextContent("never added to or subtracted from the spend");
  });

  it("test06_the_link_to_activity_asks_the_shell_to_open_it", async () => {
    const { onOpenActivity } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Savings in Activity" }));
    expect(onOpenActivity).toHaveBeenCalled();
  });

  it("test07_a_failed_savings_log_fails_this_card_alone_with_retry", async () => {
    const { onRetry } = setup({ mcpError: true });
    const card = screen.getByRole("region", { name: "MCP part" });
    expect(card).toHaveTextContent("Upstream's savings log could not be read.");
    expect(card).toHaveTextContent("savings-v4.jsonl: line 812 is not valid JSON.");
    expect(card).toHaveTextContent("The other cards are not affected");
    await userEvent.click(within(card).getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });

  it("test08_the_three_columns_stack_on_a_narrow_page", () => {
    setup();
    const columns = document.querySelector("[class*='grid-cols-3']") as HTMLElement;
    expect(columns).toHaveClass("@min-[780px]:grid-cols-3");
  });
});
