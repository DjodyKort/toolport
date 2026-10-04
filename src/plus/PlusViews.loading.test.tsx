import { describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

const gate = vi.hoisted(() => {
  let open!: () => void;
  const promise = new Promise<void>((resolve) => (open = resolve));
  return { promise, open };
});

vi.mock("./allcommands/AllCommandsPage", async () => {
  await gate.promise;
  return { AllCommandsPage: () => <p>All commands loaded</p> };
});

import { PlusViews } from "./PlusViews";

describe("PlusViews while a screen is being fetched", () => {
  it("shows a loading state, never nothing, then the screen", async () => {
    render(<PlusViews view="commands" onSelectView={vi.fn()} />);
    expect(screen.getByRole("status", { name: "Loading screen" })).toBeInTheDocument();
    await act(async () => gate.open());
    expect(await screen.findByText("All commands loaded")).toBeInTheDocument();
    expect(
      screen.queryByRole("status", { name: "Loading screen" }),
    ).not.toBeInTheDocument();
  });
});
