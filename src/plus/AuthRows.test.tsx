import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AuthRows } from "./AuthRows";
import type { AuthRow } from "./api";

const rows: AuthRow[] = [
  {
    server: "beta",
    state: "needs_reauth",
    reason: "invalid_grant",
    since: 1,
    expiresAt: null,
    ttlSecs: null,
    lastProbe: 2,
    fix: {
      action: "reauth",
      server: "beta",
      label: "Sign in to beta again",
      command: "toolportctl auth login beta",
      ipc: null,
    },
  },
  {
    server: "alpha",
    state: "ok",
    reason: "ok",
    since: 1,
    expiresAt: null,
    ttlSecs: null,
    lastProbe: 2,
    fix: null,
  },
];

describe("AuthRows", () => {
  it("renders state and a fix button only for unhealthy rows", async () => {
    const onFix = vi.fn();
    render(<AuthRows rows={rows} onFix={onFix} />);
    expect(screen.getByText("Needs sign-in")).toBeInTheDocument();
    expect(screen.getByText("Signed in")).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Sign in to beta again" }));
    expect(onFix).toHaveBeenCalledWith(rows[0]);
  });
});
