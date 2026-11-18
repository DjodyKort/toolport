import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { CacheCard } from "./CacheCard";
import { readWorld } from "./testkit";

describe("CacheCard", () => {
  it("test01_it_splits_all_tokens_into_cache_read_write_fresh_and_output", () => {
    const { summary } = readWorld();
    render(<CacheCard summary={summary} badge="Measured" />);
    const meter = screen.getByRole("img", { name: /Cache read 95\.8%/ });
    expect(meter).toHaveAccessibleName(
      "Cache read 95.8%, Cache write 3.4%, Fresh input 0.5%, Output 0.3%",
    );
    const card = screen.getByRole("region", { name: "Cache vs fresh tokens" });
    for (const name of ["Cache read", "Cache write", "Fresh input", "Output"])
      expect(within(card).getByText(name)).toBeInTheDocument();
    expect(card).toHaveTextContent("96%");
    expect(card).toHaveTextContent("of input-side tokens were cache reads (7 days)");
  });

  it("test02_it_counts_requests_and_sessions_and_shows_no_cost_per_row", () => {
    const { summary } = readWorld();
    render(<CacheCard summary={summary} badge="Counted" />);
    expect(screen.getByText(/in 142 sessions/)).toBeInTheDocument();
    expect(screen.getByText("Counted")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Claude Code reports cost per request, not per token kind, so no cost is shown per row.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/\$/)).toBeNull();
  });
});
