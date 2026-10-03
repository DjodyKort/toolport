import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WhatLoads } from "./WhatLoads";
import { plusWhatLoadsFixture } from "./fixtures/whatLoads";

describe("WhatLoads", () => {
  it("groups items by kind with loaded state and tokens", () => {
    render(<WhatLoads data={plusWhatLoadsFixture} />);
    expect(screen.getByTestId("total-tokens").textContent).toBe("~1738 tokens");
    const mcp = screen.getByRole("list", { name: "MCP servers" });
    const rows = within(mcp).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute("data-loaded")).toBe("true");
    expect(rows[1].getAttribute("data-loaded")).toBe("false");
    expect(rows[1].textContent).toContain("Skipped");
    expect(screen.getByRole("list", { name: "Overrides" }).textContent).toContain(
      "profile settings_overrides",
    );
  });

  it("shows the default profile label when none is set", () => {
    render(
      <WhatLoads
        data={{ ...plusWhatLoadsFixture, profile: null, items: [], clobbers: [] }}
      />,
    );
    expect(screen.getByText(/default in/)).toBeTruthy();
  });
});
